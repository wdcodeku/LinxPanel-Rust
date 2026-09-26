use axum::{extract::State, routing::post, Json, Router};
use jsonwebtoken::{encode, EncodingKey, Header};
use std::sync::Arc;

use crate::models::*;
use crate::AppState;

pub fn public_routes() -> Router<Arc<AppState>> {
    Router::new().route("/auth/login", post(login))
}

async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> Json<ApiResponse<LoginResponse>> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = ?")
        .bind(&req.username)
        .fetch_optional(&state.db)
        .await;

    let user = match user {
        Ok(Some(u)) => u,
        _ => return Json(ApiResponse::err("Invalid credentials")),
    };

    if !bcrypt::verify(&req.password, &user.password_hash).unwrap_or(false) {
        return Json(ApiResponse::err("Invalid credentials"));
    }

    // `session_timeout` is a panel setting in hours; it was previously ignored.
    let hours = session_timeout_hours(&state).await;
    let exp = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::hours(hours))
        .unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::hours(24))
        .timestamp() as usize;

    let claims = Claims {
        sub: user.id,
        username: user.username.clone(),
        role: user.role.clone(),
        exp,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(state.jwt_secret.as_bytes()),
    )
    .unwrap_or_default();

    Json(ApiResponse::ok(LoginResponse {
        token,
        user: UserInfo {
            id: user.id,
            username: user.username,
            role: user.role,
        },
    }))
}

/// Reads the configured session length, falling back to 24h when unset or invalid.
async fn session_timeout_hours(state: &Arc<AppState>) -> i64 {
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
