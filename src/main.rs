use std::net::SocketAddr;
use std::sync::Arc;
use axum::response::Html;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
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
}

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
    let db_path = data_dir.join("linxpanel.db");
    let db_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| format!("sqlite:{}?mode=rwc", db_path.display()));

    let pool = sqlx::SqlitePool::connect(&db_url).await?;
    db::migrate(&pool).await?;
    db::seed_admin(&pool).await?;

    let state = Arc::new(AppState {
        db: pool,
        jwt_secret: std::env::var("JWT_SECRET")
            .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string()),
    });

    let api_routes = api::routes(state.clone());

    let web_dir = resolve_path("web/dist");
    tracing::info!("Serving static files from: {:?}", web_dir);

    let index_html = std::fs::read_to_string(web_dir.join("index.html"))
        .unwrap_or_else(|_| "<h1>index.html not found</h1>".into());
    let index_html: &'static str = Box::leak(index_html.into_boxed_str());

    let spa_fallback = axum::Router::new()
        .fallback(move || async move { Html(index_html) });

    let app = axum::Router::new()
        .nest("/api", api_routes)
        .nest_service("/assets", ServeDir::new(web_dir.join("assets")))
        .nest_service("/favicon.svg", tower_http::services::ServeFile::new(web_dir.join("favicon.svg")))
        .fallback_service(spa_fallback)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "7200".into())
        .parse()
        .unwrap_or(7200);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("LinxPanel listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}