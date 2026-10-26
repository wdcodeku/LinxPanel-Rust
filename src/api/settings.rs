use crate::models::*;
use crate::services::openresty;
use crate::AppState;
use axum::{
    extract::State,
    routing::{get, post},
    Extension, Json, Router,
};
use std::sync::Arc;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/settings", get(get_settings).put(update_settings))
        .route("/settings/password", post(change_password))
        .route("/settings/openresty/status", get(get_openresty_status))
        .route("/settings/openresty/install", post(quick_install_openresty))
}

/// Keys that live in the settings table but must never reach or be set from the browser.
/// `jwt_secret` signs every session: leaking it lets anyone mint admin tokens.
const PRIVATE_KEYS: &[&str] = &[
    "jwt_secret",
    crate::services::database::SETTING_MYSQL_ADMIN_USER,
    crate::services::database::SETTING_MYSQL_ADMIN_PASSWORD,
];

/// Readable here, but only writable through their dedicated, validating endpoints.
const MANAGED_KEYS: &[&str] = crate::services::panel_access::MANAGED_SETTINGS;

fn is_private_key(key: &str) -> bool {
    PRIVATE_KEYS.contains(&key)
}

async fn get_settings(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<std::collections::HashMap<String, String>>> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
    let map: std::collections::HashMap<String, String> = rows
        .into_iter()
        .filter(|(key, _)| !is_private_key(key))
        .collect();
    Json(ApiResponse::ok(map))
}

async fn update_settings(
    State(state): State<Arc<AppState>>,
    Json(req): Json<std::collections::HashMap<String, serde_json::Value>>,
) -> Json<ApiResponse<()>> {
    if let Some(key) = req
        .keys()
        .find(|key| is_private_key(key) || MANAGED_KEYS.contains(&key.as_str()))
    {
        return Json(ApiResponse::err(&format!(
            "Setting '{}' cannot be changed here",
            key
        )));
    }
    for (key, value) in req {
        let value = match value {
            serde_json::Value::String(v) => v,
            serde_json::Value::Bool(v) => v.to_string(),
            serde_json::Value::Number(v) => v.to_string(),
            serde_json::Value::Null => String::new(),
            other => other.to_string(),
        };
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at"
        )
        .bind(&key)
        .bind(&value)
        .execute(&state.db)
        .await
        .ok();
    }
    Json(ApiResponse::ok_msg("Settings updated"))
}

async fn change_password(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<ChangePasswordRequest>,
) -> Json<ApiResponse<serde_json::Value>> {
    // The account comes from the session, not the request body, so one user
    // cannot target another account's password.
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?")
        .bind(claims.sub)
        .fetch_optional(&state.db)
        .await;

    let user = match user {
        Ok(Some(u)) => u,
        _ => return Json(ApiResponse::err("User not found")),
    };

    if !bcrypt::verify(&req.old_password, &user.password_hash).unwrap_or(false) {
        return Json(ApiResponse::err("Current password is incorrect"));
    }

    if let Err(message) = crate::db::validate_password(&req.new_password) {
        return Json(ApiResponse::err(message));
    }

    let new_hash = match bcrypt::hash(&req.new_password, 10) {
        Ok(h) => h,
        Err(_) => return Json(ApiResponse::err("Failed to hash password")),
    };

    // Bumping session_version signs out every other session of this user.
    if let Err(err) = sqlx::query(
        "UPDATE users SET password_hash = ?, session_version = session_version + 1,
         updated_at = datetime('now') WHERE id = ?",
    )
    .bind(&new_hash)
    .bind(user.id)
    .execute(&state.db)
    .await
    {
        return Json(ApiResponse::err(&format!(
            "Failed to save password: {}",
            err
        )));
    }

    // A fresh token keeps the browser that made the change signed in.
    match crate::api::auth::issue_token(&state, &user).await {
        Ok(token) => Json(ApiResponse {
            success: true,
            message: "Password changed".into(),
            data: Some(serde_json::json!({ "token": token })),
        }),
        Err(_) => Json(ApiResponse::ok_msg("Password changed")),
    }
}

async fn get_openresty_status(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<openresty::OpenRestyStatus>> {
    match openresty::get_status(state.as_ref()).await {
        Ok(status) => Json(ApiResponse::ok(status)),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn quick_install_openresty(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<openresty::OpenRestyInstallResult>> {
    match openresty::quick_install(state.as_ref()).await {
        Ok(mut result) => {
            match openresty::apply_defense(state.as_ref()).await {
                Ok(message) => result.log.push_str(&format!("[LinxPanel] {}\n", message)),
                Err(err) => result.log.push_str(&format!(
                    "[LinxPanel] Defense settings not applied: {}\n",
                    err
                )),
            }
            Json(ApiResponse::ok(result))
        }
        Err(e) => Json(ApiResponse::err(&format!(
            "OpenResty install failed: {}",
            e
        ))),
    }
}
