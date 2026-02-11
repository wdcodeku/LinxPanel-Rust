use crate::models::*;
use crate::services::openresty;
use crate::AppState;
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/settings", get(get_settings).put(update_settings))
        .route("/settings/password", post(change_password))
        .route("/settings/openresty/status", get(get_openresty_status))
        .route("/settings/openresty/install", post(quick_install_openresty))
}

async fn get_settings(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<std::collections::HashMap<String, String>>> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
    let map: std::collections::HashMap<String, String> = rows.into_iter().collect();
    Json(ApiResponse::ok(map))
}

async fn update_settings(
    State(state): State<Arc<AppState>>,
    Json(req): Json<std::collections::HashMap<String, serde_json::Value>>,
) -> Json<ApiResponse<()>> {
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
    Json(req): Json<ChangePasswordRequest>,
) -> Json<ApiResponse<()>> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = ?")
        .bind(&req.username)
        .fetch_optional(&state.db)
        .await;

    let user = match user {
        Ok(Some(u)) => u,
        _ => return Json(ApiResponse::err("User not found")),
    };

    if !bcrypt::verify(&req.old_password, &user.password_hash).unwrap_or(false) {
        return Json(ApiResponse::err("Current password is incorrect"));
    }

    let new_hash = match bcrypt::hash(&req.new_password, 10) {
        Ok(h) => h,
        Err(_) => return Json(ApiResponse::err("Failed to hash password")),
    };

    sqlx::query("UPDATE users SET password_hash = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(&new_hash)
        .bind(user.id)
        .execute(&state.db)
        .await
        .ok();

    Json(ApiResponse::ok_msg("Password changed"))
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
        Ok(result) => Json(ApiResponse::ok(result)),
        Err(e) => Json(ApiResponse::err(&format!(
            "OpenResty install failed: {}",
            e
        ))),
    }
}
