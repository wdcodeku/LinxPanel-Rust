use axum::{
    extract::{ConnectInfo, State},
    routing::post,
    Json, Router,
};
use jsonwebtoken::{encode, EncodingKey, Header};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::models::*;
use crate::AppState;

pub fn public_routes() -> Router<Arc<AppState>> {
    Router::new().route("/auth/login", post(login))
}

/// Failed attempts allowed per client address within `LOGIN_WINDOW`.
const LOGIN_MAX_FAILURES: u32 = 5;
const LOGIN_WINDOW: Duration = Duration::from_secs(15 * 60);
/// A bcrypt hash compared against when the username does not exist, so unknown
/// and known usernames take the same time to reject.
fn dummy_hash() -> String {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| bcrypt::hash("linxpanel-dummy", 10).unwrap_or_default())
        .clone()
}

/// In-memory brute-force guard for the login endpoint, keyed by client address.
#[derive(Default)]
pub struct LoginGuard {
    failures: Mutex<HashMap<IpAddr, (u32, Instant)>>,
}

impl LoginGuard {
    /// Seconds until the address may try again, or None when it is not locked out.
    fn locked_for(&self, ip: IpAddr) -> Option<u64> {
        let mut failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        let (count, since) = *failures.get(&ip)?;
        let elapsed = since.elapsed();
        if elapsed >= LOGIN_WINDOW {
            failures.remove(&ip);
            return None;
        }
        (count >= LOGIN_MAX_FAILURES).then(|| (LOGIN_WINDOW - elapsed).as_secs().max(1))
    }

    fn record_failure(&self, ip: IpAddr) {
        let mut failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        // Bound memory under a spray from many addresses.
        if failures.len() > 10_000 {
            failures.retain(|_, (_, since)| since.elapsed() < LOGIN_WINDOW);
        }
        let entry = failures.entry(ip).or_insert((0, Instant::now()));
        if entry.1.elapsed() >= LOGIN_WINDOW {
            *entry = (0, Instant::now());
        }
        entry.0 += 1;
    }

    fn clear(&self, ip: IpAddr) {
        self.failures
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&ip);
    }
}

async fn login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<LoginRequest>,
) -> Json<ApiResponse<LoginResponse>> {
    let ip = peer.ip();
    if let Some(wait) = state.login_guard.locked_for(ip) {
        tracing::warn!(
            "Login blocked for {} ({} failed attempts)",
            ip,
            LOGIN_MAX_FAILURES
        );
        return Json(ApiResponse::err(&format!(
            "Too many failed login attempts, try again in {} minutes",
            wait.div_ceil(60)
        )));
    }

    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = ?")
        .bind(&req.username)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();

    let hash = user.as_ref().map(|u| u.password_hash.clone());
    let password = req.password.clone();
    let verified = tokio::task::spawn_blocking(move || {
        bcrypt::verify(&password, &hash.unwrap_or_else(dummy_hash))
    })
    .await
    .ok()
    .and_then(|result| result.ok())
    .unwrap_or(false);

    let user = match user {
        Some(u) if verified => u,
        _ => {
            state.login_guard.record_failure(ip);
            tracing::warn!("Failed login for '{}' from {}", req.username, ip);
            return Json(ApiResponse::err("Invalid credentials"));
        }
    };
    state.login_guard.clear(ip);

    let token = match issue_token(&state, &user).await {
        Ok(token) => token,
        Err(err) => return Json(ApiResponse::err(&format!("Login failed: {}", err))),
    };

    Json(ApiResponse::ok(LoginResponse {
        token,
        user: UserInfo {
            id: user.id,
            username: user.username,
            role: user.role,
        },
    }))
}

/// Signs a session token bound to the user's current `session_version`.
pub async fn issue_token(state: &AppState, user: &User) -> anyhow::Result<String> {
    let version = crate::db::session_version(&state.db, user.id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("user no longer exists"))?;

    // `session_timeout` is a panel setting in hours; it was previously ignored.
    let hours = session_timeout_hours(state).await;
    let exp = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::hours(hours))
        .unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::hours(24))
        .timestamp() as usize;

    let claims = Claims {
        sub: user.id,
        username: user.username.clone(),
        role: user.role.clone(),
        exp,
        ver: version,
    };
    Ok(encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )?)
}

/// Reads the configured session length, falling back to 24h when unset or invalid.
async fn session_timeout_hours(state: &AppState) -> i64 {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind("session_timeout")
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();

    row.and_then(|(value,)| value.trim().parse::<i64>().ok())
        .filter(|hours| (1..=8760).contains(hours))
        .unwrap_or(24)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_guard_locks_after_max_failures() {
        let guard = LoginGuard::default();
        let ip: IpAddr = "203.0.113.7".parse().unwrap();
        let other: IpAddr = "203.0.113.8".parse().unwrap();
        for _ in 0..LOGIN_MAX_FAILURES - 1 {
            guard.record_failure(ip);
        }
        assert!(guard.locked_for(ip).is_none());
        guard.record_failure(ip);
        assert!(guard.locked_for(ip).is_some());
        assert!(guard.locked_for(other).is_none());
        guard.clear(ip);
        assert!(guard.locked_for(ip).is_none());
    }
}
