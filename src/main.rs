use axum::{
    body::Body,
    extract::Path,
    http::{header, Response, StatusCode},
    routing::get,
};
use include_dir::{include_dir, Dir, File};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::compression::CompressionLayer;
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
    pub login_guard: api::auth::LoginGuard,
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

const USAGE: &str = "\
Usage: linxpanel [COMMAND]

Commands:
  (none)                       Run the panel server
  reset-password [--user NAME] [--password-stdin]
                               Set a user's password. Reads it from stdin with
                               --password-stdin, otherwise generates and prints one.
  version, --version, -V       Print the version
  help, --help, -h             Print this help

Environment: PORT (default 7200), BIND (default 0.0.0.0), DATABASE_URL, JWT_SECRET, RUST_LOG";

/// Opens (and migrates) the panel database, returning it with the data directory.
async fn open_database() -> anyhow::Result<(sqlx::SqlitePool, std::path::PathBuf)> {
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

/// `linxpanel reset-password`: recovery path for a lost password, also used by the
/// installer to replace the well-known default with a random one.
async fn reset_password_command(args: &[String]) -> anyhow::Result<()> {
    let mut username = db::DEFAULT_ADMIN_USER.to_string();
    let mut from_stdin = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--user" | "-u" => {
                username = iter
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("--user requires a value"))?
                    .clone();
            }
            "--password-stdin" => from_stdin = true,
            other => anyhow::bail!("Unknown argument: {}\n\n{}", other, USAGE),
        }
    }
    if username.trim().is_empty() {
        anyhow::bail!("Username must not be empty");
    }

    let password = if from_stdin {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        line.trim_end_matches(['\r', '\n']).to_string()
    } else {
        db::generate_password(16)
    };
    db::validate_password(&password).map_err(|e| anyhow::anyhow!(e))?;

    let (pool, _) = open_database().await?;
    db::set_user_password(&pool, &username, &password).await?;
    pool.close().await;

    println!("Username: {}", username);
    if !from_stdin {
        println!("Password: {}", password);
    }
    println!("Password updated. Existing sessions stay valid until they expire.");
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {}
        Some("version" | "--version" | "-V") => {
            println!("linxpanel {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("help" | "--help" | "-h") => {
            println!("{}", USAGE);
            return Ok(());
        }
        Some("reset-password") => return reset_password_command(&args[1..]).await,
        Some(other) => anyhow::bail!("Unknown command: {}\n\n{}", other, USAGE),
    }

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let (pool, data_dir) = open_database().await?;

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
        login_guard: api::auth::LoginGuard::default(),
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

    // Only the embedded UI is compressed: compressing /api would buffer the
    // streamed command output and break WebSocket upgrades.
    let web_routes = axum::Router::new()
        .route("/", get(serve_root))
        .route("/{*path}", get(serve_embedded_path))
        .layer(CompressionLayer::new());

    // No CORS layer: the UI is served from this same origin (and the Vite dev
    // server proxies /api), so cross-origin access is never needed.
    let app = axum::Router::new()
        .nest("/api", api_routes)
        .merge(web_routes)
        .layer(TraceLayer::new_for_http());

    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "7200".into())
        .parse()
        .unwrap_or(7200);
    let bind_ip: std::net::IpAddr = std::env::var("BIND")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
    let addr = SocketAddr::new(bind_ip, port);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|err| anyhow::anyhow!("Cannot listen on {}: {}", addr, err))?;
    tracing::info!("LinxPanel {} listening on http://{}", env!("CARGO_PKG_VERSION"), addr);
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
    let make_service = app.into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, make_service)
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
