//! Website access logs: analysis report, newest lines, clearing, and the
//! rotation settings shared by all sites.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::ApiResponse;
use crate::services::accesslog;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/access-logs/settings", get(settings).post(save_settings))
        .route("/sites/{id}/access-logs/report", get(report))
        .route("/sites/{id}/access-logs/tail", get(tail))
        .route("/sites/{id}/access-logs/clear", post(clear))
}

/// The site's host names (its own pages are not "referrers").
async fn site_hosts(state: &AppState, id: i64) -> Option<Vec<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT domains FROM sites WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();
    row.map(|(domains,)| {
        domains
            .split([',', ' ', '\n', '\t', ';'])
            .map(|d| {
                d.trim()
                    .split(':')
                    .next()
                    .unwrap_or("")
                    .to_ascii_lowercase()
            })
            .filter(|d| !d.is_empty())
            .collect()
    })
}

#[derive(Deserialize)]
struct ReportQuery {
    /// today | yesterday | 24h | 7d | 30d | month; ignored with from/to.
    #[serde(default)]
    range: String,
    from: Option<i64>,
    to: Option<i64>,
}

#[derive(Serialize)]
struct ReportView {
    #[serde(flatten)]
    report: accesslog::Report,
    usage: accesslog::Usage,
    /// The panel writes this site's log (OpenResty with the panel's format).
    logging: bool,
}

async fn report(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Query(q): Query<ReportQuery>,
) -> Json<ApiResponse<ReportView>> {
    let Some(hosts) = site_hosts(&state, id).await else {
        return Json(ApiResponse::err("Site not found"));
    };
    let (from, to) = match (q.from, q.to) {
        (Some(from), Some(to)) if from < to && to - from <= 366 * 86_400 => (from, to),
        (Some(_), Some(_)) => return Json(ApiResponse::err("Choose a range of at most a year")),
        _ => match accesslog::range(if q.range.is_empty() {
            "today"
        } else {
            &q.range
        }) {
            Some(range) => range,
            None => return Json(ApiResponse::err("Unknown range")),
        },
    };
    let dir = accesslog::log_dir(&state);
    let result = tokio::task::spawn_blocking(move || {
        let logging = dir.join(format!("site-{}.log", id)).exists()
            || accesslog::usage(&dir, id).archives > 0;
        ReportView {
            report: accesslog::analyze(&dir, id, from, to, &hosts),
            usage: accesslog::usage(&dir, id),
            logging,
        }
    })
    .await;
    match result {
        Ok(view) => Json(ApiResponse::ok(view)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Deserialize)]
struct TailQuery {
    #[serde(default)]
    lines: Option<usize>,
    #[serde(default)]
    q: String,
}

async fn tail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Query(q): Query<TailQuery>,
) -> Json<ApiResponse<Vec<String>>> {
    if site_hosts(&state, id).await.is_none() {
        return Json(ApiResponse::err("Site not found"));
    }
    let path = accesslog::current_log(&accesslog::log_dir(&state), id);
    let limit = q.lines.unwrap_or(200).clamp(1, 2000);
    match tokio::task::spawn_blocking(move || accesslog::tail(&path, limit, &q.q)).await {
        Ok(Ok(lines)) => Json(ApiResponse::ok(lines)),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Deserialize)]
struct ClearRequest {
    /// Also delete rotated logs.
    #[serde(default)]
    archives: bool,
}

async fn clear(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<ClearRequest>,
) -> Json<ApiResponse<()>> {
    if site_hosts(&state, id).await.is_none() {
        return Json(ApiResponse::err("Site not found"));
    }
    let dir = accesslog::log_dir(&state);
    let result = tokio::task::spawn_blocking(move || {
        accesslog::clear(&accesslog::current_log(&dir, id))?;
        if req.archives {
            accesslog::remove_archives(&dir, id);
        }
        Ok::<(), String>(())
    })
    .await;
    match result {
        Ok(Ok(())) => Json(ApiResponse::ok_msg("Cleared")),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

async fn settings(State(state): State<Arc<AppState>>) -> Json<ApiResponse<accesslog::Settings>> {
    Json(ApiResponse::ok(accesslog::load_settings(&state.db).await))
}

async fn save_settings(
    State(state): State<Arc<AppState>>,
    Json(req): Json<accesslog::Settings>,
) -> Json<ApiResponse<()>> {
    if let Err(err) = accesslog::validate(&req) {
        return Json(ApiResponse::err(&err));
    }
    for (key, value) in [
        (accesslog::SETTING_KEEP_DAYS, req.keep_days),
        (accesslog::SETTING_MAX_MB, req.max_mb),
    ] {
        if let Err(err) = crate::db::set_setting(&state.db, key, &value.to_string()).await {
            return Json(ApiResponse::err(&err.to_string()));
        }
    }
    // Apply a shorter retention right away.
    let _ = accesslog::maintain(&state).await;
    Json(ApiResponse::ok_msg("Saved"))
}
