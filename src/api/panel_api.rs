//! Panel access settings: security entrance, panel HTTPS, restart and health.

use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::ApiResponse;
use crate::services::{cert, panel_access};
use crate::AppState;

pub fn public_routes() -> Router<Arc<AppState>> {
    Router::new().route("/health", get(health))
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/settings/panel-access", get(status))
        .route("/settings/entrance", post(set_entrance))
        .route("/settings/panel-ssl", post(set_panel_ssl))
        .route("/settings/panel/restart", post(restart))
}

/// Liveness probe for the installer; exempt from the security entrance for
/// loopback callers only.
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

#[derive(Serialize)]
struct CertSummary {
    subject: String,
    issuer: String,
    not_after: String,
    days_left: Option<i64>,
    domains: Vec<String>,
}

#[derive(Serialize)]
struct PanelAccessStatus {
    entrance: Option<String>,
    /// HTTPS as configured; takes effect on the next panel start.
    ssl_enabled: bool,
    /// Whether this running process is serving HTTPS.
    ssl_active: bool,
    ssl_cert_id: Option<i64>,
    self_signed: bool,
    cert: Option<CertSummary>,
    restart_required: bool,
    /// Under systemd the panel can restart itself; otherwise `Lp restart` is needed.
    restart_supported: bool,
}

fn restart_supported() -> bool {
    std::env::var_os("INVOCATION_ID").is_some()
}

async fn build_status(state: &AppState) -> PanelAccessStatus {
    let ssl_enabled = panel_access::ssl_enabled(&state.db).await;
    let ssl_cert_id = crate::db::get_setting(&state.db, panel_access::SETTING_PANEL_SSL_CERT_ID)
        .await
        .ok()
        .flatten()
        .and_then(|value| value.trim().parse::<i64>().ok());
    let (cert_path, _, self_signed) = panel_access::ssl_paths(&state.db, &state.data_dir).await;
    let cert = if cert_path.exists() {
        let path = cert_path.clone();
        tokio::task::spawn_blocking(move || cert::inspect(&path))
            .await
            .ok()
            .and_then(|result| result.ok())
            .map(|meta| CertSummary {
                days_left: cert::days_until(&meta.not_after),
                subject: meta.subject,
                issuer: meta.issuer,
                not_after: meta.not_after,
                domains: meta.domains,
            })
    } else {
        None
    };

    PanelAccessStatus {
        entrance: state.entrance(),
        ssl_enabled,
        ssl_active: state.tls_active,
        ssl_cert_id,
        self_signed,
        cert,
        restart_required: ssl_enabled != state.tls_active,
        restart_supported: restart_supported(),
    }
}

async fn status(State(state): State<Arc<AppState>>) -> Json<ApiResponse<PanelAccessStatus>> {
    Json(ApiResponse::ok(build_status(&state).await))
}

#[derive(Deserialize)]
struct EntranceRequest {
    /// Empty disables the entrance.
    #[serde(default)]
    entrance: String,
}

async fn set_entrance(
    State(state): State<Arc<AppState>>,
    Json(req): Json<EntranceRequest>,
) -> Response {
    let entrance = match panel_access::normalize_entrance(&req.entrance) {
        Ok(entrance) => entrance,
        Err(message) => return Json(ApiResponse::<()>::err(&message)).into_response(),
    };
    if let Err(err) = panel_access::save_entrance(&state.db, entrance.as_deref()).await {
        return Json(ApiResponse::<()>::err(&format!("Failed to save: {}", err))).into_response();
    }
    state.set_entrance(entrance.clone());
    tracing::info!(
        "Security entrance {}",
        if entrance.is_some() {
            "changed"
        } else {
            "disabled"
        }
    );

    let body = Json(ApiResponse::ok(serde_json::json!({ "entrance": entrance })));
    match entrance {
        // Admit the browser that made the change, or it would lock itself out.
        Some(entrance) => (
            [(
                header::SET_COOKIE,
                panel_access::entrance_set_cookie(&entrance, &state.jwt_secret, state.tls_active),
            )],
            body,
        )
            .into_response(),
        None => body.into_response(),
    }
}

#[derive(Deserialize)]
struct PanelSslRequest {
    enabled: bool,
    /// A certificate from the certificate store; None uses the self-signed one.
    #[serde(default)]
    cert_id: Option<i64>,
    /// Replace the self-signed certificate even when one exists.
    #[serde(default)]
    regenerate: bool,
}

async fn set_panel_ssl(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<PanelSslRequest>,
) -> Json<ApiResponse<PanelAccessStatus>> {
    if req.enabled {
        if let Some(id) = req.cert_id {
            if let Err(err) = cert::get(&state.db, id).await {
                return Json(ApiResponse::err(&err));
            }
        }
        let cert_id = req.cert_id.map(|id| id.to_string()).unwrap_or_default();
        if let Err(err) =
            crate::db::set_setting(&state.db, panel_access::SETTING_PANEL_SSL_CERT_ID, &cert_id)
                .await
        {
            return Json(ApiResponse::err(&err.to_string()));
        }

        let (cert_path, key_path, self_signed) =
            panel_access::ssl_paths(&state.db, &state.data_dir).await;
        if self_signed && (req.regenerate || !cert_path.exists() || !key_path.exists()) {
            // Cover the address the admin is using right now, so it matches the URL.
            let names: Vec<String> = request_host(&headers).into_iter().collect();
            if let Err(err) = panel_access::generate_self_signed(&state.data_dir, &names) {
                return Json(ApiResponse::err(&format!(
                    "Cannot create certificate: {}",
                    err
                )));
            }
        }
        // Refuse a pair that would fail to load, rather than find out at restart.
        if let Err(err) = panel_access::load_tls_config(&cert_path, &key_path) {
            return Json(ApiResponse::err(&format!("{:#}", err)));
        }
    }

    let value = if req.enabled { "on" } else { "off" };
    if let Err(err) =
        crate::db::set_setting(&state.db, panel_access::SETTING_PANEL_SSL, value).await
    {
        return Json(ApiResponse::err(&err.to_string()));
    }
    Json(ApiResponse::ok(build_status(&state).await))
}

/// The hostname from the Host header, when it is a plain name or IP.
fn request_host(headers: &HeaderMap) -> Option<String> {
    let host = headers.get(header::HOST)?.to_str().ok()?.trim();
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next()?
    } else {
        host.rsplit_once(':').map_or(host, |(name, _)| name)
    };
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'));
    valid.then(|| name.to_string())
}

async fn restart(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    if !restart_supported() {
        return (
            StatusCode::OK,
            Json(ApiResponse::<()>::err(
                "Automatic restart needs systemd; run `Lp restart` on the server",
            )),
        );
    }
    tracing::info!("Restart requested from the panel");
    let notify = state.clone();
    tokio::spawn(async move {
        // Let this response reach the browser first.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        notify.restart.notify_one();
    });
    (
        StatusCode::OK,
        Json(ApiResponse::ok_msg("Panel is restarting")),
    )
}
