//! Runtime environments and managed application endpoints.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::ApiResponse;
use crate::services::appproc::{self, AppView, SaveAppRequest};
use crate::services::runtime::{self, RuntimeInstallRequest, RuntimeInstallResult, RuntimeStatus};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/runtimes", get(list_runtimes))
        .route("/runtimes/install", post(install_runtime))
        .route("/apps", get(list_apps).post(create_app))
        .route("/apps/{id}", post(update_app).delete(delete_app))
        .route("/apps/{id}/start", post(start_app))
        .route("/apps/{id}/stop", post(stop_app))
        .route("/apps/{id}/restart", post(restart_app))
        .route("/apps/{id}/install-deps", post(install_deps))
        .route("/apps/{id}/logs", get(app_logs).delete(clear_app_logs))
}

// ===== Runtimes =====

async fn list_runtimes() -> Json<ApiResponse<Vec<RuntimeStatus>>> {
    // Detection shells out to several binaries, so keep it off the async workers.
    match tokio::task::spawn_blocking(runtime::list).await {
        Ok(list) => Json(ApiResponse::ok(list)),
        Err(_) => Json(ApiResponse::err("Failed to detect runtimes")),
    }
}

async fn install_runtime(
    Json(body): Json<RuntimeInstallRequest>,
) -> Json<ApiResponse<RuntimeInstallResult>> {
    match tokio::task::spawn_blocking(move || runtime::install(&body.kind)).await {
        Ok(Ok(result)) => Json(ApiResponse::ok(result)),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(_) => Json(ApiResponse::err("Runtime install task failed")),
    }
}

// ===== Applications =====

async fn list_apps(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<AppView>>> {
    let records = match appproc::list(&state.db).await {
        Ok(records) => records,
        Err(err) => return Json(ApiResponse::err(&err)),
    };

    let mut views = Vec::with_capacity(records.len());
    for record in records {
        let runtime_state = state.supervisor.state_of(record.id).await;
        views.push(AppView {
            record,
            state: runtime_state,
        });
    }
    Json(ApiResponse::ok(views))
}

async fn create_app(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SaveAppRequest>,
) -> Json<ApiResponse<i64>> {
    match appproc::create(&state.db, &body).await {
        Ok(id) => Json(ApiResponse::ok(id)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn update_app(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<SaveAppRequest>,
) -> Json<ApiResponse<()>> {
    match appproc::update(&state.db, id, &body).await {
        Ok(()) => Json(ApiResponse::ok_msg("Application updated")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn delete_app(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<()>> {
    // Stop first so deleting never orphans a running process.
    let _ = appproc::stop(state.supervisor.clone(), id).await;
    match appproc::delete(&state.db, id).await {
        Ok(()) => Json(ApiResponse::ok_msg("Application deleted")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn start_app(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<()>> {
    match appproc::start(
        state.supervisor.clone(),
        state.db.clone(),
        state.data_dir.clone(),
        id,
    )
    .await
    {
        Ok(()) => Json(ApiResponse::ok_msg("Application started")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn stop_app(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<()>> {
    match appproc::stop(state.supervisor.clone(), id).await {
        Ok(()) => Json(ApiResponse::ok_msg("Application stopped")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn restart_app(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<()>> {
    match appproc::restart(
        state.supervisor.clone(),
        state.db.clone(),
        state.data_dir.clone(),
        id,
    )
    .await
    {
        Ok(()) => Json(ApiResponse::ok_msg("Application restarted")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

#[derive(Debug, Deserialize, Default)]
struct InstallDepsRequest {
    /// Node: also run the `build` script when package.json has one.
    #[serde(default)]
    build: bool,
}

#[derive(Serialize)]
struct InstallDepsResult {
    success: bool,
    log: String,
}

/// pip / npm / pnpm / yarn / composer install in the app's working directory.
async fn install_deps(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    body: Option<Json<InstallDepsRequest>>,
) -> Json<ApiResponse<InstallDepsResult>> {
    let record = match appproc::get(&state.db, id).await {
        Ok(record) => record,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    let build = body.map(|Json(b)| b.build).unwrap_or(false);
    let result =
        tokio::task::spawn_blocking(move || appproc::install_dependencies(&record, build)).await;
    match result {
        Ok(Ok(log)) => Json(ApiResponse::ok(InstallDepsResult { success: true, log })),
        Ok(Err(log)) => Json(ApiResponse::ok(InstallDepsResult {
            success: false,
            log,
        })),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Debug, Deserialize)]
struct LogQuery {
    lines: Option<usize>,
}

async fn app_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Query(query): Query<LogQuery>,
) -> Json<ApiResponse<String>> {
    let lines = query.lines.unwrap_or(300);
    let data_dir = state.data_dir.clone();
    match tokio::task::spawn_blocking(move || appproc::read_log(&data_dir, id, lines)).await {
        Ok(text) => Json(ApiResponse::ok(text)),
        Err(_) => Json(ApiResponse::err("Failed to read application log")),
    }
}

async fn clear_app_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<()>> {
    match appproc::clear_log(&state.data_dir, id) {
        Ok(()) => Json(ApiResponse::ok_msg("Log cleared")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}
