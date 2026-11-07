//! File manager recycle bin: list, restore, delete for good, empty, settings.

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::ApiResponse;
use crate::services::recycle;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/files/recycle", get(list))
        .route("/files/recycle/restore", post(restore))
        .route("/files/recycle/delete", post(purge))
        .route("/files/recycle/empty", post(empty))
        .route("/files/recycle/settings", get(settings).post(save_settings))
}

#[derive(Serialize)]
struct Listing {
    items: Vec<recycle::Item>,
    settings: recycle::Settings,
}

async fn list(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Listing>> {
    match recycle::list(&state.db).await {
        Ok(items) => Json(ApiResponse::ok(Listing {
            items,
            settings: recycle::load_settings(&state.db).await,
        })),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

#[derive(Deserialize)]
struct Ids {
    ids: Vec<i64>,
}

async fn restore(
    State(state): State<Arc<AppState>>,
    Json(req): Json<Ids>,
) -> Json<ApiResponse<Vec<String>>> {
    match recycle::restore(&state.db, &req.ids).await {
        Ok(paths) => Json(ApiResponse::ok(paths)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn purge(
    State(state): State<Arc<AppState>>,
    Json(req): Json<Ids>,
) -> Json<ApiResponse<usize>> {
    match recycle::purge(&state.db, &req.ids).await {
        Ok(n) => Json(ApiResponse::ok(n)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn empty(State(state): State<Arc<AppState>>) -> Json<ApiResponse<usize>> {
    match recycle::empty(&state.db).await {
        Ok(n) => Json(ApiResponse::ok(n)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn settings(State(state): State<Arc<AppState>>) -> Json<ApiResponse<recycle::Settings>> {
    Json(ApiResponse::ok(recycle::load_settings(&state.db).await))
}

async fn save_settings(
    State(state): State<Arc<AppState>>,
    Json(req): Json<recycle::Settings>,
) -> Json<ApiResponse<()>> {
    match recycle::save_settings(&state.db, &req).await {
        Ok(()) => Json(ApiResponse::ok_msg("Saved")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}
