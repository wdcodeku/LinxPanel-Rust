//! Firewall and listening-port endpoints.

use axum::{
    extract::Path,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::models::ApiResponse;
use crate::services::firewall::{
    self, FirewallRule, FirewallRuleRequest, FirewallStatus, ListeningPort,
};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/firewall/status", get(status))
        .route("/firewall/rules", get(list_rules).post(add_rule))
        .route("/firewall/rules/{id}", axum::routing::delete(delete_rule))
        .route("/firewall/toggle", post(toggle))
        .route("/network/ports", get(ports))
}

async fn status() -> Json<ApiResponse<FirewallStatus>> {
    match tokio::task::spawn_blocking(firewall::status).await {
        Ok(value) => Json(ApiResponse::ok(value)),
        Err(_) => Json(ApiResponse::err("Failed to read firewall status")),
    }
}

async fn list_rules() -> Json<ApiResponse<Vec<FirewallRule>>> {
    match blocking(firewall::list_rules).await {
        Ok(rules) => Json(ApiResponse::ok(rules)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn add_rule(Json(body): Json<FirewallRuleRequest>) -> Json<ApiResponse<String>> {
    match blocking(move || firewall::add_rule(&body)).await {
        Ok(message) => Json(ApiResponse::ok(message)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn delete_rule(Path(id): Path<String>) -> Json<ApiResponse<String>> {
    match blocking(move || firewall::delete_rule(&id)).await {
        Ok(message) => Json(ApiResponse::ok(message)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

#[derive(Debug, Deserialize)]
struct ToggleRequest {
    enabled: bool,
}

async fn toggle(Json(body): Json<ToggleRequest>) -> Json<ApiResponse<String>> {
    match blocking(move || firewall::set_enabled(body.enabled)).await {
        Ok(message) => Json(ApiResponse::ok(message)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn ports() -> Json<ApiResponse<Vec<ListeningPort>>> {
    match blocking(firewall::listening_ports).await {
        Ok(rows) => Json(ApiResponse::ok(rows)),
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
