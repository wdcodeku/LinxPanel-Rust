use axum::{extract::State, routing::get, Json, Router};
use std::sync::Arc;

use crate::models::*;
use crate::services;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/system/info", get(system_info))
}

async fn system_info(State(_state): State<Arc<AppState>>) -> Json<ApiResponse<SystemInfo>> {
    let info = services::system::get_system_info();
    Json(ApiResponse::ok(info))
}