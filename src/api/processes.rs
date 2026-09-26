//! Process listing and signalling.

use axum::{
    extract::{Path, Query},
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;

use crate::models::*;
use crate::services::metrics;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/processes", get(list_processes))
        .route("/processes/{pid}/signal", post(signal_process))
}

async fn list_processes(
    Query(query): Query<ProcessQuery>,
) -> Json<ApiResponse<ProcessListResponse>> {
    // sysinfo's refresh is blocking; keep it off the async runtime's worker threads.
    let result = tokio::task::spawn_blocking(move || metrics::list_processes(&query))
        .await
        .unwrap_or(ProcessListResponse {
            total: 0,
            rows: Vec::new(),
        });
    Json(ApiResponse::ok(result))
}

async fn signal_process(
    Path(pid): Path<u32>,
    Json(body): Json<KillProcessRequest>,
) -> Json<ApiResponse<()>> {
    let signal = if body.signal.trim().is_empty() {
        "TERM".to_string()
    } else {
        body.signal.trim().to_string()
    };

    match tokio::task::spawn_blocking(move || metrics::signal_process(pid, &signal)).await {
        Ok(Ok(())) => Json(ApiResponse::ok_msg("Signal sent")),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(_) => Json(ApiResponse::err("Failed to signal process")),
    }
}
