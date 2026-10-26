//! The signed-in user's own account security: two-factor login and username.

use axum::{extract::State, routing::post, Extension, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::{ApiResponse, Claims, User};
use crate::services::totp;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/settings/mfa/setup", post(mfa_setup))
        .route("/settings/mfa/enable", post(mfa_enable))
        .route("/settings/mfa/disable", post(mfa_disable))
        .route("/settings/username", post(change_username))
}

#[derive(Serialize)]
struct MfaSetup {
    /// Base32 secret for manual entry.
    secret: String,
    /// `otpauth://` URI, rendered as a QR code by the UI.
    uri: String,
}

/// Starts setup: a new secret is stored as pending until a code from it is confirmed,
/// so a half-finished setup never locks the account.
async fn mfa_setup(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
) -> Json<ApiResponse<MfaSetup>> {
    let secret = match totp::generate_secret() {
        Ok(secret) => secret,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    if let Err(err) = crate::db::set_totp_pending(&state.db, claims.sub, &secret).await {
        return Json(ApiResponse::err(&err.to_string()));
    }
    let issuer = crate::db::get_setting(&state.db, "panel_name")
        .await
        .ok()
        .flatten()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "LinxPanel".to_string());
    let uri = totp::otpauth_uri(&secret, &claims.username, &issuer);
    Json(ApiResponse::ok(MfaSetup { secret, uri }))
}

#[derive(Deserialize)]
struct CodeRequest {
    code: String,
}

async fn mfa_enable(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<CodeRequest>,
) -> Json<ApiResponse<()>> {
    let (_, pending, _) = match crate::db::totp_state(&state.db, claims.sub).await {
        Ok(totp) => totp,
        Err(err) => return Json(ApiResponse::err(&err.to_string())),
    };
    let Some(pending) = pending else {
        return Json(ApiResponse::err("Start two-factor setup first"));
    };
    let Some(step) = totp::verify(&pending, &req.code, 0) else {
        return Json(ApiResponse::err("Invalid two-factor code"));
    };
    if let Err(err) = crate::db::activate_totp(&state.db, claims.sub, step).await {
        return Json(ApiResponse::err(&err.to_string()));
    }
    tracing::info!("Two-factor login enabled for {}", claims.username);
    Json(ApiResponse::ok_msg("Two-factor login enabled"))
}

/// Turning it off needs a current code, so a stolen session alone cannot do it.
async fn mfa_disable(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<CodeRequest>,
) -> Json<ApiResponse<()>> {
    let (secret, _, last_step) = match crate::db::totp_state(&state.db, claims.sub).await {
        Ok(totp) => totp,
        Err(err) => return Json(ApiResponse::err(&err.to_string())),
    };
    let Some(secret) = secret else {
        return Json(ApiResponse::ok_msg("Two-factor login disabled"));
    };
    if totp::verify(&secret, &req.code, last_step).is_none() {
        return Json(ApiResponse::err("Invalid two-factor code"));
    }
    if let Err(err) = crate::db::disable_totp(&state.db, &claims.username).await {
        return Json(ApiResponse::err(&err.to_string()));
    }
    tracing::info!("Two-factor login disabled for {}", claims.username);
    Json(ApiResponse::ok_msg("Two-factor login disabled"))
}

#[derive(Deserialize)]
struct UsernameRequest {
    username: String,
    /// The current password, confirming it is really the account owner.
    password: String,
}

async fn change_username(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<UsernameRequest>,
) -> Json<ApiResponse<serde_json::Value>> {
    let username = req.username.trim().to_string();
    if let Err(message) = crate::db::validate_username(&username) {
        return Json(ApiResponse::err(message));
    }

    let user = match sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(claims.sub)
        .fetch_optional(&state.db)
        .await
    {
        Ok(Some(user)) => user,
        _ => return Json(ApiResponse::err("User not found")),
    };
    let hash = user.password_hash.clone();
    let password = req.password.clone();
    let verified = tokio::task::spawn_blocking(move || bcrypt::verify(&password, &hash))
        .await
        .ok()
        .and_then(|result| result.ok())
        .unwrap_or(false);
    if !verified {
        return Json(ApiResponse::err("Current password is incorrect"));
    }
    if username == user.username {
        return Json(ApiResponse::err("That is already your username"));
    }

    let result =
        sqlx::query("UPDATE users SET username = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(&username)
            .bind(user.id)
            .execute(&state.db)
            .await;
    if let Err(err) = result {
        let message = if err.to_string().contains("UNIQUE") {
            "That username is already taken".to_string()
        } else {
            format!("Failed to save: {}", err)
        };
        return Json(ApiResponse::err(&message));
    }
    tracing::info!("Username changed from {} to {}", user.username, username);

    // The token carries the username, so hand the browser a fresh one.
    let renamed = User {
        username: username.clone(),
        ..user
    };
    match crate::api::auth::issue_token(&state, &renamed).await {
        Ok(token) => Json(ApiResponse {
            success: true,
            message: "Username changed".into(),
            data: Some(serde_json::json!({ "token": token, "username": username })),
        }),
        Err(_) => Json(ApiResponse::ok_msg("Username changed")),
    }
}
