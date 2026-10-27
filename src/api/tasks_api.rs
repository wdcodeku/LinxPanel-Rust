//! Background tasks: list, and follow one task's log.

use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::models::ApiResponse;
use crate::services::tasks::{Progress, Summary};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tasks", get(list))
        .route("/tasks/{id}", get(progress))
}

async fn list(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<Summary>>> {
    Json(ApiResponse::ok(state.tasks.list()))
}

#[derive(Deserialize)]
struct ProgressQuery {
    #[serde(default)]
    offset: usize,
}

async fn progress(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<ProgressQuery>,
) -> Json<ApiResponse<Progress>> {
    match state.tasks.progress(&id, query.offset) {
        Some(progress) => Json(ApiResponse::ok(progress)),
        None => Json(ApiResponse::err("Task not found")),
    }
}
