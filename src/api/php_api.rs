//! PHP versions: install / remove side by side, default version, extensions,
//! php.ini and FPM settings, and the PHP version of each website.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::models::ApiResponse;
use crate::services::{openresty, php, phpver, tasks};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/php", get(overview))
        .route("/php/install", post(install))
        .route("/php/uninstall", post(uninstall))
        .route("/php/default", post(set_default))
        .route("/php/{version}/restart", post(restart))
        .route(
            "/php/{version}/extensions",
            get(extensions).post(set_extension),
        )
        .route("/php/{version}/settings", get(settings).post(save_settings))
        .route("/sites/{id}/php", post(set_site_php))
}

#[derive(Serialize)]
struct Overview {
    /// Side-by-side versions can be installed (Debian / Ubuntu).
    multi: bool,
    versions: Vec<&'static str>,
    installed: Vec<phpver::Installed>,
    /// The version sites without their own choice use.
    default: String,
    /// Number of PHP sites per version ("" = default).
    sites: BTreeMap<String, i64>,
}

async fn default_version(state: &AppState) -> String {
    let saved = crate::db::get_setting(&state.db, phpver::SETTING_DEFAULT)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    if php::pool_for(&saved).is_some() {
        saved
    } else {
        php::detect().map(|p| p.version).unwrap_or_default()
    }
}

async fn site_counts(state: &AppState) -> BTreeMap<String, i64> {
    sqlx::query_as::<_, (String, i64)>(
        "SELECT php_version, COUNT(*) FROM sites WHERE site_type = 'php' GROUP BY php_version",
    )
    .fetch_all(&state.db)
    .await
    .unwrap_or_default()
    .into_iter()
    .collect()
}

async fn overview(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Overview>> {
    let installed = tokio::task::spawn_blocking(phpver::installed)
        .await
        .unwrap_or_default();
    Json(ApiResponse::ok(Overview {
        multi: phpver::multi_supported(),
        versions: phpver::VERSIONS.to_vec(),
        installed,
        default: default_version(&state).await,
        sites: site_counts(&state).await,
    }))
}

#[derive(Deserialize)]
struct VersionRequest {
    version: String,
}

#[derive(Serialize)]
struct LogResult {
    success: bool,
    log: String,
}

async fn install(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VersionRequest>,
) -> Json<ApiResponse<tasks::Started>> {
    if !phpver::valid_version(&req.version) {
        return Json(ApiResponse::err("Unsupported PHP version"));
    }
    if !phpver::multi_supported() {
        return Json(ApiResponse::err(
            "Multiple PHP versions are supported on Debian and Ubuntu",
        ));
    }
    let title = format!("Install PHP {}", req.version);
    let task_state = state.clone();
    Json(ApiResponse::ok(state.tasks.start(
        "php-install",
        &title,
        async move {
            let version = req.version.clone();
            let result = tasks::blocking(move || phpver::run_script("install", &version)).await;
            match result {
                Ok(Ok(log)) => {
                    // The first PHP becomes the default.
                    if default_version(&task_state).await.is_empty() || php::pools().len() == 1 {
                        let _ = crate::db::set_setting(
                            &task_state.db,
                            phpver::SETTING_DEFAULT,
                            &req.version,
                        )
                        .await;
                    }
                    ApiResponse::ok(LogResult { success: true, log })
                }
                Ok(Err(err)) => ApiResponse::ok(LogResult {
                    success: false,
                    log: err,
                }),
                Err(err) => ApiResponse::err(&err.to_string()),
            }
        },
    )))
}

async fn uninstall(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VersionRequest>,
) -> Json<ApiResponse<tasks::Started>> {
    let version = req.version.trim().to_string();
    if !phpver::valid_version(&version) {
        return Json(ApiResponse::err("Unsupported PHP version"));
    }
    let used: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sites WHERE php_version = ?")
        .bind(&version)
        .fetch_one(&state.db)
        .await
        .unwrap_or((0,));
    if used.0 > 0 {
        return Json(ApiResponse::err(&format!(
            "{} website(s) use PHP {}; switch them to another version first",
            used.0, version
        )));
    }
    if default_version(&state).await == version && php::pools().len() > 1 {
        return Json(ApiResponse::err(
            "This is the default PHP version; make another version the default first",
        ));
    }
    let title = format!("Remove PHP {}", version);
    Json(ApiResponse::ok(state.tasks.start(
        "php-remove",
        &title,
        async move {
            match tasks::blocking(move || phpver::run_script("remove", &version)).await {
                Ok(Ok(log)) => ApiResponse::ok(LogResult { success: true, log }),
                Ok(Err(err)) => ApiResponse::ok(LogResult {
                    success: false,
                    log: err,
                }),
                Err(err) => ApiResponse::err(&err.to_string()),
            }
        },
    )))
}

/// Re-renders every PHP site (those following the default change backend).
async fn resync_php_sites(state: &Arc<AppState>, only_default: bool) -> Vec<String> {
    let query = if only_default {
        "SELECT id FROM sites WHERE site_type = 'php' AND php_version = ''"
    } else {
        "SELECT id FROM sites WHERE site_type = 'php'"
    };
    let ids: Vec<(i64,)> = sqlx::query_as(query)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
    let mut errors = Vec::new();
    for (id,) in ids {
        if let Err(err) = openresty::sync_site(state, id).await {
            errors.push(format!("site {}: {}", id, err));
        }
    }
    errors
}

async fn set_default(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VersionRequest>,
) -> Json<ApiResponse<()>> {
    if php::pool_for(&req.version).is_none() {
        return Json(ApiResponse::err("This PHP version is not installed"));
    }
    if let Err(err) = crate::db::set_setting(&state.db, phpver::SETTING_DEFAULT, &req.version).await
    {
        return Json(ApiResponse::err(&err.to_string()));
    }
    let version = req.version.clone();
    let _ = tokio::task::spawn_blocking(move || phpver::set_cli_default(&version)).await;
    let errors = resync_php_sites(&state, true).await;
    if errors.is_empty() {
        Json(ApiResponse::ok_msg("Default PHP version changed"))
    } else {
        Json(ApiResponse::err(&errors.join("; ")))
    }
}

async fn restart(Path(version): Path<String>) -> Json<ApiResponse<()>> {
    match tokio::task::spawn_blocking(move || phpver::reload(&version)).await {
        Ok(Ok(())) => Json(ApiResponse::ok_msg("Restarted")),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

async fn extensions(Path(version): Path<String>) -> Json<ApiResponse<Vec<phpver::Extension>>> {
    match tokio::task::spawn_blocking(move || phpver::extensions(&version)).await {
        Ok(Ok(list)) => Json(ApiResponse::ok(list)),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Deserialize)]
struct ExtensionRequest {
    name: String,
    install: bool,
}

async fn set_extension(
    State(state): State<Arc<AppState>>,
    Path(version): Path<String>,
    Json(req): Json<ExtensionRequest>,
) -> Json<ApiResponse<tasks::Started>> {
    if php::pool_for(&version).is_none() {
        return Json(ApiResponse::err("This PHP version is not installed"));
    }
    let title = format!(
        "{} php{}-{}",
        if req.install { "Install" } else { "Remove" },
        version,
        req.name
    );
    Json(ApiResponse::ok(state.tasks.start(
        "php-ext",
        &title,
        async move {
            match tasks::blocking(move || phpver::set_extension(&version, &req.name, req.install))
                .await
            {
                Ok(Ok(log)) => ApiResponse::ok(LogResult { success: true, log }),
                Ok(Err(err)) => ApiResponse::ok(LogResult {
                    success: false,
                    log: err,
                }),
                Err(err) => ApiResponse::err(&err.to_string()),
            }
        },
    )))
}

async fn settings(Path(version): Path<String>) -> Json<ApiResponse<phpver::Settings>> {
    match tokio::task::spawn_blocking(move || phpver::settings(&version)).await {
        Ok(Ok(s)) => Json(ApiResponse::ok(s)),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

async fn save_settings(
    Path(version): Path<String>,
    Json(req): Json<phpver::Settings>,
) -> Json<ApiResponse<()>> {
    match tokio::task::spawn_blocking(move || phpver::save_settings(&version, &req)).await {
        Ok(Ok(())) => Json(ApiResponse::ok_msg("Saved")),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

async fn set_site_php(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<VersionRequest>,
) -> Json<ApiResponse<()>> {
    let version = req.version.trim().to_string();
    if !version.is_empty() && php::pool_for(&version).is_none() {
        return Json(ApiResponse::err("This PHP version is not installed"));
    }
    let site_type: Option<(String,)> = sqlx::query_as("SELECT site_type FROM sites WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .unwrap_or(None);
    match site_type {
        Some((t,)) if t == "php" => {}
        Some(_) => return Json(ApiResponse::err("Only PHP sites have a PHP version")),
        None => return Json(ApiResponse::err("Site not found")),
    }
    if let Err(err) =
        sqlx::query("UPDATE sites SET php_version = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(&version)
            .bind(id)
            .execute(&state.db)
            .await
    {
        return Json(ApiResponse::err(&err.to_string()));
    }
    match openresty::sync_site(&state, id).await {
        Ok(()) => Json(ApiResponse::ok_msg("PHP version changed")),
        Err(err) => Json(ApiResponse::err(&format!(
            "OpenResty apply failed: {}",
            err
        ))),
    }
}
