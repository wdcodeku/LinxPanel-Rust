//! Active defense: settings, kernel bans, events and system hardening.

use axum::{
    extract::{ConnectInfo, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use crate::models::ApiResponse;
use crate::services::defense::{self, BanRecord, DefenseConfig, Event, SysctlStatus};
use crate::services::{ipban, openresty};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/defense", get(overview).post(save))
        .route("/defense/bans", get(bans))
        .route("/defense/ban", post(ban))
        .route("/defense/unban", post(unban))
        .route("/defense/events", get(events))
        .route("/defense/install-nft", post(install_nft))
        .route("/defense/sysctl", post(sysctl))
}

#[derive(Serialize)]
struct Status {
    /// nftables is installed (kernel bans possible).
    nft_available: bool,
    /// Kernel bans currently in force; None when nft cannot be read.
    kernel_bans: Option<usize>,
    open_connections: usize,
    tracked_addresses: usize,
    sysctl: SysctlStatus,
    your_ip: String,
}

#[derive(Serialize)]
struct Overview {
    config: DefenseConfig,
    status: Status,
}

async fn overview(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Json<ApiResponse<Overview>> {
    let config = defense::load_config(&state.db).await;
    let (nft_available, kernel_bans, sysctl) = tokio::task::spawn_blocking(|| {
        let available = ipban::available();
        let bans = available
            .then(|| ipban::list().ok().map(|bans| bans.len()))
            .flatten();
        (available, bans, defense::sysctl_status())
    })
    .await
    .unwrap_or((false, None, defense::sysctl_status()));
    Json(ApiResponse::ok(Overview {
        config,
        status: Status {
            nft_available,
            kernel_bans,
            open_connections: state.connections.open_connections(),
            tracked_addresses: state.defense.tracked(),
            sysctl,
            your_ip: peer.ip().to_canonical().to_string(),
        },
    }))
}

async fn save(
    State(state): State<Arc<AppState>>,
    Json(config): Json<DefenseConfig>,
) -> Json<ApiResponse<String>> {
    if let Err(err) = config.validate() {
        return Json(ApiResponse::err(&err));
    }
    let before = defense::load_config(&state.db).await;
    if let Err(err) = defense::save_config(&state.db, &config).await {
        return Json(ApiResponse::err(&err.to_string()));
    }
    // Only the CC settings live in nginx; skip the reload otherwise.
    let nginx_changed = (
        before.cc_enabled,
        before.cc_rate,
        before.cc_burst,
        before.cc_conn,
    ) != (
        config.cc_enabled,
        config.cc_rate,
        config.cc_burst,
        config.cc_conn,
    );
    if !nginx_changed {
        return Json(ApiResponse::ok("Saved".into()));
    }
    match openresty::apply_defense(&state).await {
        Ok(message) => Json(ApiResponse::ok(message)),
        Err(err) => {
            // The settings are kept; nginx is unchanged.
            Json(ApiResponse::err(&format!(
                "Saved, but nginx was not updated: {}",
                err
            )))
        }
    }
}

async fn bans(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<BanRecord>>> {
    Json(ApiResponse::ok(defense::list_bans(&state.db).await))
}

#[derive(Deserialize)]
struct BanRequest {
    ip: String,
    /// 0 = permanent; omitted = the configured ban duration.
    #[serde(default)]
    minutes: Option<u64>,
    #[serde(default)]
    reason: String,
}

fn parse_ip(raw: &str) -> Result<IpAddr, String> {
    raw.trim()
        .parse::<IpAddr>()
        .map(|ip| ip.to_canonical())
        .map_err(|_| "Enter a single IPv4 or IPv6 address".to_string())
}

async fn ban(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(req): Json<BanRequest>,
) -> Json<ApiResponse<()>> {
    let ip = match parse_ip(&req.ip) {
        Ok(ip) => ip,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    if ip == peer.ip().to_canonical() {
        return Json(ApiResponse::err(
            "That is your own address; banning it would lock you out",
        ));
    }
    if let Some(why) = defense::never_ban(&ip) {
        return Json(ApiResponse::err(&format!("Cannot ban {}: {}", ip, why)));
    }
    let minutes = match req.minutes {
        Some(minutes) => minutes,
        None => defense::load_config(&state.db).await.ban_minutes,
    };
    if minutes > 525_600 {
        return Json(ApiResponse::err("Ban duration is at most one year"));
    }
    let reason = match req.reason.trim() {
        "" => "Banned manually",
        reason => reason,
    };
    let reason: String = reason.chars().take(200).collect();
    match defense::ban(
        &state.db,
        ip,
        (minutes > 0).then_some(minutes * 60),
        "manual",
        &reason,
    )
    .await
    {
        Ok(()) => Json(ApiResponse::ok_msg("Banned")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

#[derive(Deserialize)]
struct UnbanRequest {
    ip: String,
}

async fn unban(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UnbanRequest>,
) -> Json<ApiResponse<()>> {
    let ip = match parse_ip(&req.ip) {
        Ok(ip) => ip,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    match defense::unban(&state.db, ip).await {
        Ok(()) => Json(ApiResponse::ok_msg("Unbanned")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default)]
    limit: Option<i64>,
}

async fn events(
    State(state): State<Arc<AppState>>,
    Query(query): Query<EventsQuery>,
) -> Json<ApiResponse<Vec<Event>>> {
    let limit = query.limit.unwrap_or(200).clamp(1, 2000);
    Json(ApiResponse::ok(
        defense::recent_events(&state.db, limit).await,
    ))
}

async fn install_nft() -> Json<ApiResponse<String>> {
    let result = tokio::task::spawn_blocking(|| {
        let mut log = String::new();
        if !ipban::available()
            && !crate::services::runtime::install_packages(&["nftables"], &mut log)
        {
            return Err(log);
        }
        ipban::ensure_table()?;
        log.push_str("[LinxPanel] nftables is ready\n");
        Ok(log)
    })
    .await;
    match result {
        Ok(Ok(log)) => Json(ApiResponse::ok(log)),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Deserialize)]
struct SysctlRequest {
    enabled: bool,
}

async fn sysctl(Json(req): Json<SysctlRequest>) -> Json<ApiResponse<String>> {
    let result = tokio::task::spawn_blocking(move || {
        if req.enabled {
            defense::apply_sysctl()
        } else {
            // Values stay until reboot; only the profile is removed.
            defense::remove_sysctl()
                .map(|()| "Removed; the kernel defaults return after a reboot".into())
        }
    })
    .await;
    match result {
        Ok(Ok(text)) => Json(ApiResponse::ok(text)),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}
