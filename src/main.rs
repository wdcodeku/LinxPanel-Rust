use axum::{
    body::Body,
    extract::Path,
    http::{header, Response, StatusCode},
    routing::get,
};
use include_dir::{include_dir, Dir, File};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod api;
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

fn web_file_response(path: &str, file: &File<'_>) -> Response<Body> {
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let cache_control = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    let content = file.contents().to_vec();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CACHE_CONTROL, cache_control)
        .body(Body::from(content))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn index_response() -> Response<Body> {
    if let Some(file) = EMBEDDED_WEB.get_file("index.html") {
        return web_file_response("index.html", &file);
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

async fn serve_root() -> Response<Body> {
    index_response()
}

async fn serve_embedded_path(Path(path): Path<String>) -> Response<Body> {
    let normalized = path.trim_start_matches('/').trim();
    if normalized.is_empty() {
        return index_response();
    }

    if let Some(file) = EMBEDDED_WEB.get_file(normalized) {
        return web_file_response(normalized, &file);
    }

    if !normalized.contains('.') {
        return index_response();
    }

    not_found_response()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "linxpanel=debug,info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

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

    let jwt_secret = db::resolve_jwt_secret(&pool).await?;

    let supervisor = Arc::new(services::appproc::Supervisor::new());
    let (shutdown_tx, _shutdown_rx) = tokio::sync::watch::channel(false);

    let metrics = services::metrics::MetricsEngine::new();
    services::metrics::spawn(metrics.clone(), shutdown_tx.subscribe());

    let state = Arc::new(AppState {
        db: pool,
        jwt_secret,
        openresty_lock: tokio::sync::Mutex::new(()),
        metrics,
        supervisor: supervisor.clone(),
        data_dir: data_dir.clone(),
        shutdown: shutdown_tx,
    });

    // Keep ACME certificates fresh without operator intervention.
    tokio::spawn(api::cert_api::renewal_task(
        state.clone(),
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

    let app = axum::Router::new()
        .nest("/api", api_routes)
        .route("/", get(serve_root))
        .route("/{*path}", get(serve_embedded_path))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "7200".into())
        .parse()
        .unwrap_or(7200);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("LinxPanel listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
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
        tracing::warn!("Forcing exit: connections did not drain within {:?}", SHUTDOWN_GRACE);
        std::process::exit(0);
    });

    let shutdown_supervisor = state.supervisor.clone();
    let shutdown_notifier = state.shutdown.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            tracing::info!("Shutdown signal received");

            // Tell WebSocket handlers to close; otherwise graceful shutdown waits
            // forever on the metrics and terminal streams.
            let _ = shutdown_notifier.send(true);
            services::appproc::stop_all(shutdown_supervisor).await;
        })
        .await?;

    tracing::info!("LinxPanel stopped");
    Ok(())
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
