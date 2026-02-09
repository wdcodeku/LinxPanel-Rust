use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::models::*;
use crate::services;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/system/info", get(system_info))
        .route("/system/processes", get(system_processes))
}

async fn system_info(State(_state): State<Arc<AppState>>) -> Json<ApiResponse<SystemInfo>> {
    let info = services::system::get_system_info();
    Json(ApiResponse::ok(info))
}

#[derive(Debug, Deserialize)]
struct ProcessQuery {
    limit: Option<usize>,
}

async fn system_processes(
    State(_state): State<Arc<AppState>>,
    Query(query): Query<ProcessQuery>,
) -> Json<ApiResponse<Vec<ProcessInfo>>> {
    let limit = query.limit.unwrap_or(15).clamp(1, 64);
    let processes = services::system::get_top_processes(limit);
    Json(ApiResponse::ok(processes))
}
