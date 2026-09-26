//! System overview endpoints backed by the realtime metrics engine.

use axum::{extract::State, routing::get, Json, Router};
use std::sync::Arc;

use crate::models::*;
use crate::services::metrics;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/system/info", get(system_info))
        .route("/system/overview", get(system_overview))
}

/// Static host facts plus the most recent sample — the REST equivalent of the WS snapshot.
async fn system_info(State(state): State<Arc<AppState>>) -> Json<ApiResponse<MetricsSnapshot>> {
    Json(ApiResponse::ok(state.metrics.snapshot()))
}

#[derive(serde::Serialize)]
struct SystemOverview {
    static_info: StaticInfo,
    latest: Option<MetricsSample>,
    disks: Vec<DiskInfo>,
    interfaces: Vec<NetworkInterface>,
    process_count: usize,
}

/// Same data as `/system/info` but with disks pre-merged per physical device.
async fn system_overview(State(state): State<Arc<AppState>>) -> Json<ApiResponse<SystemOverview>> {
    let snapshot = state.metrics.snapshot();
    let (disks, interfaces, process_count) = match snapshot.detail {
        Some(detail) => (
            metrics::merge_disks(&detail.disks),
            detail.interfaces,
            detail.process_count,
        ),
        None => (Vec::new(), Vec::new(), 0),
    };
    Json(ApiResponse::ok(SystemOverview {
        static_info: snapshot.static_info,
        latest: snapshot.latest,
        disks,
        interfaces,
        process_count,
    }))
}
