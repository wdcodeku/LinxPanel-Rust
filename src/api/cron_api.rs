//! Crontab endpoints.

use axum::{
    extract::Path,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::models::ApiResponse;
use crate::services::cron::{self, CronBackend, CronJob, CronJobRequest};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/cron", get(list).post(create))
        .route("/cron/backend", get(backend))
        .route("/cron/{id}", post(update).delete(delete))
        .route("/cron/{id}/toggle", post(toggle))
}

async fn backend() -> Json<ApiResponse<CronBackend>> {
    Json(ApiResponse::ok(cron::backend()))
}

async fn list() -> Json<ApiResponse<Vec<CronJob>>> {
    match blocking(cron::list).await {
        Ok(jobs) => Json(ApiResponse::ok(jobs)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn create(Json(body): Json<CronJobRequest>) -> Json<ApiResponse<()>> {
    match blocking(move || cron::create(&body)).await {
        Ok(()) => Json(ApiResponse::ok_msg("Cron job created")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn update(
    Path(id): Path<usize>,
    Json(body): Json<CronJobRequest>,
) -> Json<ApiResponse<()>> {
    match blocking(move || cron::update(id, &body)).await {
        Ok(()) => Json(ApiResponse::ok_msg("Cron job updated")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn delete(Path(id): Path<usize>) -> Json<ApiResponse<()>> {
    match blocking(move || cron::delete(id)).await {
        Ok(()) => Json(ApiResponse::ok_msg("Cron job deleted")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

#[derive(Debug, Deserialize)]
struct ToggleRequest {
    enabled: bool,
}

async fn toggle(
    Path(id): Path<usize>,
    Json(body): Json<ToggleRequest>,
) -> Json<ApiResponse<()>> {
    match blocking(move || cron::toggle(id, body.enabled)).await {
        Ok(()) => Json(ApiResponse::ok_msg("Cron job updated")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn blocking<T, F>(task: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(task)
        .await
        .unwrap_or_else(|_| Err("Task failed".into()))
}
