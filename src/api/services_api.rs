//! System service (systemd) endpoints.

use axum::{
    extract::{Path, Query},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::models::ApiResponse;
use crate::services::servicectl::{self, ServiceActionRequest, ServiceBackend, ServiceRow};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/services", get(list))
        .route("/services/backend", get(backend))
        .route("/services/action", post(action))
        .route("/services/{name}/status", get(status))
        .route("/services/{name}/logs", get(logs))
}

async fn backend() -> Json<ApiResponse<ServiceBackend>> {
    Json(ApiResponse::ok(servicectl::backend()))
}

async fn list() -> Json<ApiResponse<Vec<ServiceRow>>> {
    match blocking(servicectl::list).await {
        Ok(rows) => Json(ApiResponse::ok(rows)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn action(Json(body): Json<ServiceActionRequest>) -> Json<ApiResponse<String>> {
    match blocking(move || servicectl::action(&body)).await {
        Ok(message) => Json(ApiResponse::ok(message)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn status(Path(name): Path<String>) -> Json<ApiResponse<String>> {
    match blocking(move || servicectl::status_text(&name)).await {
        Ok(text) => Json(ApiResponse::ok(text)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

#[derive(Debug, Deserialize)]
struct LogQuery {
    lines: Option<usize>,
}

async fn logs(
    Path(name): Path<String>,
    Query(query): Query<LogQuery>,
) -> Json<ApiResponse<String>> {
    let lines = query.lines.unwrap_or(200);
    match blocking(move || servicectl::logs(&name, lines)).await {
        Ok(text) => Json(ApiResponse::ok(text)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

/// Every servicectl call shells out, so it runs on the blocking pool.
async fn blocking<T, F>(task: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .unwrap_or_else(|_| Err("Task failed".into()))
}
