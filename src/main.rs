use axum::{
    body::Body,
    extract::Path,
    http::{header, Response, StatusCode},
    routing::get,
};
use include_dir::{include_dir, Dir, File};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
mod cli;
mod db;
mod middleware;
mod models;
mod services;
mod utils;

pub struct AppState {
    pub db: sqlx::SqlitePool,
    pub jwt_secret: String,
    pub openresty_lock: tokio::sync::Mutex<()>,
    pub metrics: Arc<services::metrics::MetricsEngine>,
    pub supervisor: Arc<services::appproc::Supervisor>,
    pub data_dir: std::path::PathBuf,
    /// Flipped to true on shutdown so long-lived WebSocket handlers close themselves.
    pub shutdown: tokio::sync::watch::Sender<bool>,
    pub login_guard: api::auth::LoginGuard,
    /// Entrance, allowed IPs, bound domain and deny response; swapped on change.
    pub access: std::sync::RwLock<Arc<services::panel_access::AccessPolicy>>,
    /// Whether this process is serving HTTPS (decided once at startup).
    pub tls_active: bool,
    /// The port this process listens on (decided once at startup).
    pub listen_port: u16,
    /// Signalled to restart the panel (systemd brings it back up).
    pub restart: tokio::sync::Notify,
    /// Active defense counters (auto-ban engine).
    pub defense: Arc<services::defense::Engine>,
    /// Open panel connections, for the defense status page.
    pub connections: Arc<services::server::Tracker>,
}

impl AppState {
    pub fn access(&self) -> Arc<services::panel_access::AccessPolicy> {
        self.access
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn entrance(&self) -> Option<String> {
        self.access().entrance.clone()
    }

    /// Re-reads the access policy after one of its settings changed.
    pub async fn reload_access(&self) {
        let policy = services::panel_access::AccessPolicy::load(&self.db).await;
        *self.access.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(policy);
    }
}

static EMBEDDED_WEB: Dir<'_> = include_dir!("$OUT_DIR/embedded-web");

/// How long to let in-flight connections drain before exiting anyway.
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(5);

fn resolve_path(relative: &str) -> std::path::PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap());
    for base in &[
        exe_dir.join(".."),
        exe_dir.join("../.."),
        std::env::current_dir().unwrap_or_default(),
    ] {
        let candidate = base.join(relative);
        if candidate.exists() {
            return candidate;
        }
    }
    std::path::PathBuf::from(relative)
}

/// Gzip of an embedded file, compressed once and kept. Compressing per request
/// (as a compression layer does) let a flood of asset requests burn the CPU.
fn gzip_cached(file: &'static File<'static>) -> Option<axum::body::Bytes> {
    use std::collections::HashMap;
    use std::io::Write;
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<HashMap<&'static std::path::Path, axum::body::Bytes>>,
    > = std::sync::OnceLock::new();

    let compressible = file.contents().len() > 1024
        && matches!(
            file.path().extension().and_then(|e| e.to_str()),
            Some("js" | "css" | "html" | "svg" | "json" | "txt" | "map")
        );
    if !compressible {
        return None;
    }
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(file.path())
    {
        return Some(hit.clone());
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(file.contents()).ok()?;
    let bytes = axum::body::Bytes::from(encoder.finish().ok()?);
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(file.path(), bytes.clone());
    Some(bytes)
}

fn accepts_gzip(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .any(|enc| enc.trim().starts_with("gzip") && !enc.contains("q=0"))
        })
}

fn web_file_response(path: &str, file: &'static File<'static>, gzip: bool) -> Response<Body> {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let cache_control = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, cache_control)
        .header(header::VARY, "Accept-Encoding");
    // Embedded bytes are 'static: served without copying.
    let body = match gzip.then(|| gzip_cached(file)).flatten() {
        Some(compressed) => {
            response = response.header(header::CONTENT_ENCODING, "gzip");
            Body::from(compressed)
        }
        None => Body::from(axum::body::Bytes::from_static(file.contents())),
    };
    response
        .body(body)
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn index_response(gzip: bool) -> Response<Body> {
    if let Some(file) = EMBEDDED_WEB.get_file("index.html") {
        return web_file_response("index.html", file, gzip);
    }
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from("<h3>LinxPanel UI is not embedded.</h3>"))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn not_found_response() -> Response<Body> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from("Not Found"))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

async fn serve_root(headers: axum::http::HeaderMap) -> Response<Body> {
    index_response(accepts_gzip(&headers))
}

async fn serve_embedded_path(
    Path(path): Path<String>,
    headers: axum::http::HeaderMap,
) -> Response<Body> {
    let gzip = accepts_gzip(&headers);
    let normalized = path.trim_start_matches('/').trim();
    if normalized.is_empty() {
        return index_response(gzip);
    }

    if let Some(file) = EMBEDDED_WEB.get_file(normalized) {
        return web_file_response(normalized, file, gzip);
    }

    if !normalized.contains('.') {
        return index_response(gzip);
    }

    not_found_response()
}

/// Opens (and migrates) the panel database, returning it with the data directory.
pub(crate) async fn open_database() -> anyhow::Result<(sqlx::SqlitePool, std::path::PathBuf)> {
    let data_dir = resolve_path("data");
    if !data_dir.exists() {
        std::fs::create_dir_all(&data_dir)?;
    }
    // Certificate paths from this directory end up in nginx config, so resolve the
    // `../..` that `resolve_path` can produce into a clean absolute path.
    let data_dir = std::fs::canonicalize(&data_dir).unwrap_or(data_dir);
    let db_path = data_dir.join("linxpanel.db");
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| format!("sqlite:{}?mode=rwc", db_path.display()));

    let pool = sqlx::SqlitePool::connect(&db_url).await?;
    db::migrate(&pool).await?;
    db::seed_admin(&pool).await?;
    Ok((pool, data_dir))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Package installs run from the panel have no terminal: keep apt from prompting.
    std::env::set_var("DEBIAN_FRONTEND", "noninteractive");

    let args: Vec<String> = std::env::args().skip(1).collect();
    if cli::run(&args).await? {
        return Ok(());
    }

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let (pool, data_dir) = open_database().await?;

    let jwt_secret = db::resolve_jwt_secret(&pool).await?;

    let access = services::panel_access::AccessPolicy::load(&pool).await;
    let (listener, port) = bind_listener(&pool).await?;
    let tls_config = load_panel_tls(&pool, &data_dir).await;

    let supervisor = Arc::new(services::appproc::Supervisor::new());
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);

    let metrics = services::metrics::MetricsEngine::new();
    services::metrics::spawn(metrics.clone(), shutdown_tx.subscribe());

    let tracker = services::server::Tracker::new(services::server::Limits::from_env());
    let state = Arc::new(AppState {
        db: pool,
        jwt_secret,
        openresty_lock: tokio::sync::Mutex::new(()),
        metrics,
        supervisor: supervisor.clone(),
        data_dir: data_dir.clone(),
        shutdown: shutdown_tx,
        login_guard: api::auth::LoginGuard::default(),
        access: std::sync::RwLock::new(Arc::new(access)),
        tls_active: tls_config.is_some(),
        listen_port: port,
        restart: tokio::sync::Notify::new(),
        defense: services::defense::Engine::new(&data_dir),
        connections: tracker.clone(),
    });

    // Keep ACME certificates fresh without operator intervention.
    tokio::spawn(api::cert_api::renewal_task(
        state.clone(),
        state.shutdown.subscribe(),
    ));

    // Active defense: log watching and automatic kernel bans. nginx gets its
    // shared defense settings (per-site logs, rate limit zones) once at start.
    {
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(err) = services::openresty::apply_defense(&state).await {
                tracing::debug!("nginx defense settings not applied: {}", err);
            }
        });
    }
    tokio::spawn(services::defense::run(
        state.defense.clone(),
        state.db.clone(),
        state.shutdown.subscribe(),
    ));

    // Bring managed applications back up after a panel restart.
    tokio::spawn(services::appproc::start_autostart_apps(
        supervisor,
        state.db.clone(),
        data_dir.clone(),
    ));

    let api_routes = api::routes(state.clone());
    tracing::info!("Serving embedded web resources from executable");

    // Embedded UI; gzip variants are cached per file (see `gzip_cached`).
    let web_routes = axum::Router::new()
        .route("/", get(serve_root))
        .route("/{*path}", get(serve_embedded_path));

    // No CORS layer: the UI is served from this same origin (and the Vite dev
    // server proxies /api), so cross-origin access is never needed.
    let app = axum::Router::new()
        .nest("/api", api_routes)
        .merge(web_routes)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            middleware::access_guard,
        ))
        .layer(axum::middleware::from_fn(middleware::security_headers))
        .layer(TraceLayer::new_for_http());

    let scheme = if tls_config.is_some() {
        "https"
    } else {
        "http"
    };
    tracing::info!(
        "LinxPanel {} listening on {}://{}",
        env!("CARGO_PKG_VERSION"),
        scheme,
        listener.local_addr()?
    );
    if state.entrance().is_some() {
        tracing::info!("Security entrance is enabled (show it with `Lp entrance`)");
    }
    // Idle keep-alive connections and open WebSockets can keep graceful shutdown
    // waiting indefinitely, so a top-level watchdog bounds how long that may take.
    // It must be spawned here, in the normal flow, to be reliably polled.
    let mut watchdog_rx = state.shutdown.subscribe();
    tokio::spawn(async move {
        while !*watchdog_rx.borrow() {
            if watchdog_rx.changed().await.is_err() {
                return;
            }
        }
        tokio::time::sleep(SHUTDOWN_GRACE).await;
        tracing::warn!(
            "Forcing exit: connections did not drain within {:?}",
            SHUTDOWN_GRACE
        );
        std::process::exit(0);
    });

    let stopping = tokio::spawn(shutdown_future(state.clone()));
    services::server::serve(
        listener,
        tls_config,
        app,
        state.connections.clone(),
        state.shutdown.subscribe(),
    )
    .await;
    // Managed apps are stopped by the shutdown task; wait for it.
    let _ = stopping.await;

    tracing::info!("LinxPanel stopped");
    Ok(())
}

/// Binds the configured port. A port saved from the panel that cannot be bound
/// (taken by another program since) falls back to the PORT environment variable,
/// so a bad setting never keeps the panel down.
async fn bind_listener(pool: &sqlx::SqlitePool) -> anyhow::Result<(tokio::net::TcpListener, u16)> {
    let bind_ip: std::net::IpAddr = std::env::var("BIND")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
    let env_port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(7200);
    let port = services::panel_access::effective_port(pool).await;

    match tokio::net::TcpListener::bind(SocketAddr::new(bind_ip, port)).await {
        Ok(listener) => Ok((listener, port)),
        Err(err) if port != env_port => {
            tracing::error!(
                "Cannot listen on configured port {} ({}); falling back to {}. \
                 Change it in the panel or with `Lp port`",
                port,
                err,
                env_port
            );
            let addr = SocketAddr::new(bind_ip, env_port);
            let listener = tokio::net::TcpListener::bind(addr)
                .await
                .map_err(|err| anyhow::anyhow!("Cannot listen on {}: {}", addr, err))?;
            Ok((listener, env_port))
        }
        Err(err) => Err(anyhow::anyhow!("Cannot listen on port {}: {}", port, err)),
    }
}

/// Loads the panel certificate when HTTPS is on. A missing or broken certificate
/// falls back to plain HTTP with an error in the log, so the panel never becomes
/// unreachable because of it.
async fn load_panel_tls(
    pool: &sqlx::SqlitePool,
    data_dir: &std::path::Path,
) -> Option<Arc<tokio_rustls::rustls::ServerConfig>> {
    if !services::panel_access::ssl_enabled(pool).await {
        return None;
    }
    let (cert_path, key_path, _) = services::panel_access::ssl_paths(pool, data_dir).await;
    match services::panel_access::load_tls_config(&cert_path, &key_path) {
        Ok(config) => Some(config),
        Err(err) => {
            tracing::error!(
                "Panel HTTPS is on but the certificate cannot be loaded ({:#}); serving plain HTTP. \
                 Fix it in the panel, or run `Lp ssl on --regenerate`",
                err
            );
            None
        }
    }
}

/// Completes when the panel should stop, after telling long-lived handlers and
/// managed applications to shut down.
async fn shutdown_future(state: Arc<AppState>) {
    tokio::select! {
        _ = shutdown_signal() => tracing::info!("Shutdown signal received"),
        _ = state.restart.notified() => tracing::info!("Restarting"),
    }

    // Tell WebSocket handlers to close; otherwise graceful shutdown waits
    // forever on the metrics and terminal streams.
    let _ = state.shutdown.send(true);
    services::appproc::stop_all(state.supervisor.clone()).await;
}

/// Resolves on Ctrl-C, or on SIGTERM when the panel runs under systemd.
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(err) => tracing::warn!("Cannot listen for SIGTERM: {}", err),
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
