//! Panel security settings, as in the "panel settings" pages of BaoTa / 1Panel:
//! listen port, security entrance, HTTPS, allowed IPs, bound domain, the
//! response unauthorized visitors get, plus restart and health.
//!
//! Each setting has its own endpoint so it can be validated - in particular so
//! an admin cannot lock out the very browser making the change.

use axum::{
    extract::{ConnectInfo, State},
    http::{header, HeaderMap},
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;

use crate::models::{ApiResponse, Claims};
use crate::services::{cert, firewall, panel_access};
use crate::AppState;

pub fn public_routes() -> Router<Arc<AppState>> {
    Router::new().route("/health", get(health))
}

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/settings/panel-access", get(status))
        .route("/settings/panel-port", post(set_port))
        .route("/settings/entrance", post(set_entrance))
        .route("/settings/panel-ssl", post(set_panel_ssl))
        .route("/settings/allowed-ips", post(set_allowed_ips))
        .route("/settings/bind-domain", post(set_bind_domain))
        .route("/settings/deny-response", post(set_deny_response))
        .route("/settings/panel/restart", post(restart))
}

/// Liveness probe for the installer; exempt from the access guard for
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
    /// Port as configured; takes effect on the next panel start.
    port: u16,
    /// Port this running process listens on.
    active_port: u16,
    entrance: Option<String>,
    /// HTTPS as configured; takes effect on the next panel start.
    ssl_enabled: bool,
    /// Whether this running process is serving HTTPS.
    ssl_active: bool,
    ssl_cert_id: Option<i64>,
    self_signed: bool,
    cert: Option<CertSummary>,
    allowed_ips: String,
    bind_domain: Option<String>,
    deny_response: &'static str,
    /// The address this request came from, so the UI can offer to allow it.
    client_ip: String,
    session_timeout: i64,
    username: String,
    mfa_enabled: bool,
    restart_required: bool,
    /// Under systemd the panel can restart itself; otherwise `Lp restart` is needed.
    restart_supported: bool,
}

fn restart_supported() -> bool {
    std::env::var_os("INVOCATION_ID").is_some()
}

async fn build_status(state: &AppState, claims: &Claims, peer: SocketAddr) -> PanelAccessStatus {
    let ssl_enabled = panel_access::ssl_enabled(&state.db).await;
    let port = panel_access::effective_port(&state.db).await;
    let ssl_cert_id = crate::db::get_setting(&state.db, panel_access::SETTING_PANEL_SSL_CERT_ID)
        .await
        .ok()
        .flatten()
        .and_then(|value| value.trim().parse::<i64>().ok());
    let (cert_path, _, self_signed) = panel_access::ssl_paths(&state.db, &state.data_dir).await;
    let cert = if ssl_enabled && cert_path.exists() {
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
    let session_timeout = crate::db::get_setting(&state.db, "session_timeout")
        .await
        .ok()
        .flatten()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(24);
    let account = crate::db::account_security(&state.db, claims.sub)
        .await
        .ok()
        .flatten();

    let policy = state.access();
    PanelAccessStatus {
        port,
        active_port: state.listen_port,
        entrance: policy.entrance.clone(),
        ssl_enabled,
        ssl_active: state.tls_active,
        ssl_cert_id,
        self_signed,
        cert,
        allowed_ips: panel_access::format_ip_list(&policy.allowed_ips),
        bind_domain: policy.domain.clone(),
        deny_response: policy.deny.as_str(),
        client_ip: peer.ip().to_canonical().to_string(),
        session_timeout,
        username: account
            .as_ref()
            .map_or_else(|| claims.username.clone(), |a| a.username.clone()),
        mfa_enabled: account.is_some_and(|a| a.mfa_enabled),
        restart_required: ssl_enabled != state.tls_active || port != state.listen_port,
        restart_supported: restart_supported(),
    }
}

type StatusResponse = Json<ApiResponse<PanelAccessStatus>>;

async fn status(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> StatusResponse {
    Json(ApiResponse::ok(build_status(&state, &claims, peer).await))
}

/// Saves a managed setting, reloads the live policy and answers with fresh status.
async fn save_and_report(
    state: &AppState,
    claims: &Claims,
    peer: SocketAddr,
    key: &str,
    value: &str,
) -> StatusResponse {
    if let Err(err) = crate::db::set_setting(&state.db, key, value).await {
        return Json(ApiResponse::err(&format!("Failed to save: {}", err)));
    }
    state.reload_access().await;
    tracing::info!("Panel setting '{}' changed by {}", key, claims.username);
    Json(ApiResponse::ok(build_status(state, claims, peer).await))
}

#[derive(Deserialize)]
struct PortRequest {
    port: i64,
}

async fn set_port(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<PortRequest>,
) -> StatusResponse {
    let port = match panel_access::validate_port(req.port) {
        Ok(port) => port,
        Err(message) => return Json(ApiResponse::err(&message)),
    };
    // The current port is in use by this very process, so only probe new ones.
    if port != state.listen_port
        && std::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).is_err()
    {
        return Json(ApiResponse::err(&format!(
            "Port {} is already in use",
            port
        )));
    }
    // Open the new port so the panel stays reachable after the restart.
    let opened = tokio::task::spawn_blocking(move || open_firewall_port(port))
        .await
        .unwrap_or(false);
    if opened {
        tracing::info!("Opened port {}/tcp in the firewall", port);
    }
    save_and_report(
        &state,
        &claims,
        peer,
        panel_access::SETTING_LISTEN_PORT,
        &port.to_string(),
    )
    .await
}

/// Best effort: allows the port when a manageable firewall is active.
fn open_firewall_port(port: u16) -> bool {
    let status = firewall::status();
    if !status.available || !status.manageable || !status.enabled {
        return false;
    }
    firewall::add_rule(&firewall::FirewallRuleRequest {
        port: port.to_string(),
        protocol: "tcp".into(),
        source: String::new(),
        action: "allow".into(),
    })
    .is_ok()
}

#[derive(Deserialize)]
struct EntranceRequest {
    /// Empty disables the entrance.
    #[serde(default)]
    entrance: String,
}

async fn set_entrance(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<EntranceRequest>,
) -> Response {
    let entrance = match panel_access::normalize_entrance(&req.entrance) {
        Ok(entrance) => entrance,
        Err(message) => return Json(ApiResponse::<()>::err(&message)).into_response(),
    };
    let body = save_and_report(
        &state,
        &claims,
        peer,
        panel_access::SETTING_ENTRANCE,
        entrance.as_deref().unwrap_or(""),
    )
    .await;
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
    Extension(claims): Extension<Claims>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<PanelSslRequest>,
) -> StatusResponse {
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
            let mut names: Vec<String> = request_host(&headers).into_iter().collect();
            names.extend(state.access().domain.clone());
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
    save_and_report(
        &state,
        &claims,
        peer,
        panel_access::SETTING_PANEL_SSL,
        value,
    )
    .await
}

/// The hostname from the Host header, when it is a plain name or IP.
fn request_host(headers: &HeaderMap) -> Option<String> {
    let host = headers.get(header::HOST)?.to_str().ok()?;
    let name = panel_access::host_name(host);
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'));
    valid.then_some(name)
}

#[derive(Deserialize)]
struct AllowedIpsRequest {
    /// Comma/newline separated IPs or CIDR blocks; empty allows every address.
    #[serde(default)]
    ips: String,
}

async fn set_allowed_ips(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<AllowedIpsRequest>,
) -> StatusResponse {
    let list = match panel_access::parse_ip_list(&req.ips) {
        Ok(list) => list,
        Err(message) => return Json(ApiResponse::err(&message)),
    };
    let client = peer.ip().to_canonical();
    if !list.is_empty() && !list.iter().any(|net| net.contains(client)) {
        return Json(ApiResponse::err(&format!(
            "Your current IP {} must be in the list, or you would lock yourself out",
            client
        )));
    }
    let value = panel_access::format_ip_list(&list);
    save_and_report(
        &state,
        &claims,
        peer,
        panel_access::SETTING_ALLOWED_IPS,
        &value,
    )
    .await
}

#[derive(Deserialize)]
struct BindDomainRequest {
    /// Empty removes the binding.
    #[serde(default)]
    domain: String,
}

async fn set_bind_domain(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<BindDomainRequest>,
) -> StatusResponse {
    let domain = match panel_access::normalize_domain(&req.domain) {
        Ok(domain) => domain,
        Err(message) => return Json(ApiResponse::err(&message)),
    };
    if let Some(domain) = &domain {
        let current = headers
            .get(header::HOST)
            .and_then(|value| value.to_str().ok())
            .map(panel_access::host_name)
            .unwrap_or_default();
        if current != *domain {
            return Json(ApiResponse::err(&format!(
                "Open the panel through {} first, then bind it, or you would lock yourself out",
                domain
            )));
        }
    }
    save_and_report(
        &state,
        &claims,
        peer,
        panel_access::SETTING_BIND_DOMAIN,
        domain.as_deref().unwrap_or(""),
    )
    .await
}

#[derive(Deserialize)]
struct DenyResponseRequest {
    mode: String,
}

async fn set_deny_response(
    State(state): State<Arc<AppState>>,
    Extension(claims): Extension<Claims>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<DenyResponseRequest>,
) -> StatusResponse {
    let Some(mode) = panel_access::DenyResponse::parse(&req.mode) else {
        return Json(ApiResponse::err("Unknown response mode"));
    };
    save_and_report(
        &state,
        &claims,
        peer,
        panel_access::SETTING_DENY_RESPONSE,
        mode.as_str(),
    )
    .await
}

async fn restart(State(state): State<Arc<AppState>>) -> Json<ApiResponse<()>> {
    if !restart_supported() {
        return Json(ApiResponse::err(
            "Automatic restart needs systemd; run `Lp restart` on the server",
        ));
    }
    tracing::info!("Restart requested from the panel");
    let notify = state.clone();
    tokio::spawn(async move {
        // Let this response reach the browser first.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        notify.restart.notify_one();
    });
    Json(ApiResponse::ok_msg("Panel is restarting"))
}
