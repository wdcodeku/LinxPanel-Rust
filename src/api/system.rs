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
        .route("/system/counts", get(counts))
        .route("/system/ip", get(public_ip))
}

#[derive(serde::Serialize, Clone, Default)]
struct PublicIp {
    /// The server's address as seen from the internet; empty when no
    /// lookup service answered.
    ip: String,
}

/// Lookup services, domestic first; each answers with the bare address.
const IP_SERVICES: &[&str] = &[
    "https://4.ipw.cn",
    "https://api.ipify.org",
    "https://ifconfig.me/ip",
    "https://ip.3322.net",
];

fn lookup_public_ip() -> String {
    for url in IP_SERVICES {
        let Ok(output) = crate::services::shell::run_with_timeout(
            "curl",
            &["-fsS", "--connect-timeout", "3", "--max-time", "5", url],
            std::time::Duration::from_secs(7),
        ) else {
            continue;
        };
        let text = output.stdout.trim();
        if output.ok() && text.parse::<std::net::IpAddr>().is_ok() {
            return text.to_string();
        }
    }
    String::new()
}

/// The public address, looked up at most every 30 minutes (5 after a miss).
async fn public_ip() -> Json<ApiResponse<PublicIp>> {
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    static CACHE: Mutex<Option<(Instant, String)>> = Mutex::new(None);
    if let Some((at, ip)) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        let ttl = if ip.is_empty() { 300 } else { 1800 };
        if at.elapsed() < Duration::from_secs(ttl) {
            return Json(ApiResponse::ok(PublicIp { ip }));
        }
    }
    let ip = tokio::task::spawn_blocking(lookup_public_ip)
        .await
        .unwrap_or_default();
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), ip.clone()));
    Json(ApiResponse::ok(PublicIp { ip }))
}

#[derive(serde::Serialize, Default)]
struct Counts {
    sites: i64,
    sites_running: i64,
    databases: i64,
    cron_jobs: i64,
    /// Website programs and service apps from the app store, and managed
    /// applications under Runtimes.
    apps: i64,
}

/// What the dashboard's overview card counts. Cheap: the panel's own
/// records plus the crontab, never the database servers themselves.
async fn counts(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Counts>> {
    let count = |sql: &'static str| {
        let db = state.db.clone();
        async move {
            sqlx::query_as::<_, (i64,)>(sql)
                .fetch_one(&db)
                .await
                .map(|(n,)| n)
                .unwrap_or(0)
        }
    };
    let cron_jobs = tokio::task::spawn_blocking(|| {
        crate::services::cron::list()
            .map(|jobs| jobs.len() as i64)
            .unwrap_or(0)
    })
    .await
    .unwrap_or(0);
    Json(ApiResponse::ok(Counts {
        sites: count("SELECT COUNT(*) FROM sites").await,
        sites_running: count("SELECT COUNT(*) FROM sites WHERE status = 'running'").await,
        databases: count("SELECT COUNT(*) FROM databases").await,
        cron_jobs,
        apps: count(
            "SELECT (SELECT COUNT(*) FROM app_installs) + (SELECT COUNT(*) FROM service_installs) + (SELECT COUNT(*) FROM apps)",
        )
        .await,
    }))
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
