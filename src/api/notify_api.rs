//! Alert notification channels and events, test sends, alert history; and
//! the operation audit log.

use axum::{
    extract::{Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Serialize;
use std::sync::Arc;

use crate::models::ApiResponse;
use crate::services::{audit, notify};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/notify", get(config).post(save))
        .route("/notify/test", post(test))
        .route("/notify/history", get(history))
        .route("/audit", get(audit_list))
}

async fn config(State(state): State<Arc<AppState>>) -> Json<ApiResponse<notify::Config>> {
    Json(ApiResponse::ok(notify::masked(
        notify::load(&state.db).await,
    )))
}

async fn save(
    State(state): State<Arc<AppState>>,
    Json(req): Json<notify::Config>,
) -> Json<ApiResponse<()>> {
    match notify::save(&state.db, req).await {
        Ok(()) => Json(ApiResponse::ok_msg("Saved")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn test(
    State(state): State<Arc<AppState>>,
    Json(req): Json<notify::Channel>,
) -> Json<ApiResponse<()>> {
    match notify::test(&state.db, req).await {
        Ok(()) => Json(ApiResponse::ok_msg("Sent")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

#[derive(Serialize, sqlx::FromRow)]
struct AlertRow {
    id: i64,
    created_at: String,
    title: String,
    body: String,
    result: String,
}

async fn history(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<AlertRow>>> {
    match sqlx::query_as::<_, AlertRow>("SELECT * FROM alert_log ORDER BY id DESC LIMIT 100")
        .fetch_all(&state.db)
        .await
    {
        Ok(rows) => Json(ApiResponse::ok(rows)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

async fn audit_list(
    State(state): State<Arc<AppState>>,
    Query(q): Query<audit::Query>,
) -> Json<ApiResponse<audit::Page>> {
    match audit::list(&state.db, &q).await {
        Ok(page) => Json(ApiResponse::ok(page)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}
