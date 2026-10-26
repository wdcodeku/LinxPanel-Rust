//! Panel updates: release check, one-click upgrade, auto-update settings.

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::ApiResponse;
use crate::services::updater::{self, CheckResult, RunState, UpdateConfig};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/update", get(status))
        .route("/update/check", post(check))
        .route("/update/settings", post(save_settings))
        .route("/update/run", post(run))
}

#[derive(Serialize)]
struct Status {
    current: String,
    check: Option<CheckResult>,
    config: UpdateConfig,
    /// Set while an update runs (the panel restarts during it).
    running: Option<RunState>,
    log: String,
    last_result: Option<serde_json::Value>,
    /// False for development runs that the installer did not set up.
    can_update: bool,
}

async fn build_status(state: &AppState) -> Status {
    let (running, log) = updater::progress(&state.db, &state.data_dir).await;
    Status {
        current: updater::CURRENT_VERSION.into(),
        check: state.updates.cached(),
        config: updater::load_config(&state.db).await,
        running,
        log,
        last_result: updater::last_result(&state.db).await,
        can_update: updater::install_root(&state.data_dir).is_some(),
    }
}

async fn status(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Status>> {
    Json(ApiResponse::ok(build_status(&state).await))
}

async fn check(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Status>> {
    let config = updater::load_config(&state.db).await;
    let result = state.updates.check(&config).await;
    if !result.error.is_empty() && result.latest.is_none() {
        return Json(ApiResponse::err(&format!(
            "Could not check for updates: {}",
            result.error
        )));
    }
    Json(ApiResponse::ok(build_status(&state).await))
}

async fn save_settings(
    State(state): State<Arc<AppState>>,
    Json(config): Json<UpdateConfig>,
) -> Json<ApiResponse<()>> {
    if let Err(err) = config.validate() {
        return Json(ApiResponse::err(&err));
    }
    match updater::save_config(&state.db, &config).await {
        Ok(()) => Json(ApiResponse::ok_msg("Settings updated")),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Deserialize)]
struct RunRequest {
    /// The release the user saw and confirmed; refused if it is no longer the latest.
    tag: String,
}

async fn run(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RunRequest>,
) -> Json<ApiResponse<()>> {
    let config = updater::load_config(&state.db).await;
    // Always act on a fresh lookup, never on what the client sends.
    let result = state.updates.check(&config).await;
    let Some(release) = result.latest.filter(|_| result.has_update) else {
        return Json(ApiResponse::err(if result.error.is_empty() {
            "Already up to date"
        } else {
            "Could not check for updates"
        }));
    };
    if release.tag != req.tag {
        return Json(ApiResponse::err(&format!(
            "A newer release ({}) is available; check again before updating",
            release.tag
        )));
    }
    let data_dir = state.data_dir.clone();
    let started =
        tokio::task::spawn_blocking(move || updater::start(&data_dir, &config, &release, false))
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
    match started {
        Ok(()) => Json(ApiResponse::ok_msg("Update started")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}
