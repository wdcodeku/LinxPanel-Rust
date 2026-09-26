//! TLS certificate endpoints.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::models::ApiResponse;
use crate::services::cert::{
    self, AcmeRequest, AcmeStatus, CertIssueResult, CertView, SelfSignedRequest, UploadCertRequest,
};
use crate::services::openresty;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/certs", get(list))
        .route("/certs/upload", post(upload))
        .route("/certs/self-signed", post(self_signed))
        .route("/certs/acme", post(issue_acme))
        .route("/certs/acme/status", get(acme_status))
        .route("/certs/{id}", axum::routing::delete(remove))
        .route("/certs/{id}/renew", post(renew))
        .route("/sites/{id}/ssl", post(bind_ssl))
}

async fn list(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<CertView>>> {
    match cert::list(&state.db).await {
        Ok(items) => Json(ApiResponse::ok(items)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn acme_status() -> Json<ApiResponse<AcmeStatus>> {
    match tokio::task::spawn_blocking(cert::acme_status).await {
        Ok(status) => Json(ApiResponse::ok(status)),
        Err(_) => Json(ApiResponse::err("Failed to detect ACME client")),
    }
}

async fn upload(
    State(state): State<Arc<AppState>>,
    Json(body): Json<UploadCertRequest>,
) -> Json<ApiResponse<i64>> {
    match cert::upload(&state.db, &state.data_dir, &body).await {
        Ok(id) => Json(ApiResponse::ok(id)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn self_signed(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SelfSignedRequest>,
) -> Json<ApiResponse<i64>> {
    match cert::self_signed(&state.db, &state.data_dir, &body).await {
        Ok(id) => Json(ApiResponse::ok(id)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn issue_acme(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AcmeRequest>,
) -> Json<ApiResponse<CertIssueResult>> {
    match cert::issue_acme(&state.db, &state.data_dir, &body).await {
        Ok(result) => Json(ApiResponse::ok(result)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn renew(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<CertIssueResult>> {
    match cert::renew(&state.db, id).await {
        Ok(result) => {
            // Paths are stable across a renewal, but nginx still needs to re-read them.
            if result.success {
                reload_sites_using(&state, id).await;
            }
            Json(ApiResponse::ok(result))
        }
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn remove(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<()>> {
    match cert::delete(&state.db, &state.data_dir, id).await {
        Ok(()) => Json(ApiResponse::ok_msg("Certificate deleted")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

#[derive(Debug, Deserialize)]
struct BindSslRequest {
    /// Certificate to attach; null or omitted disables TLS for the site.
    #[serde(default)]
    cert_id: Option<i64>,
    #[serde(default)]
    force_https: bool,
    #[serde(default)]
    hsts: bool,
}

/// Attaches (or detaches) a certificate on a site and re-applies its nginx config.
async fn bind_ssl(
    State(state): State<Arc<AppState>>,
    Path(site_id): Path<i64>,
    Json(body): Json<BindSslRequest>,
) -> Json<ApiResponse<()>> {
    let result = match body.cert_id {
        Some(cert_id) => {
            let record = match cert::get(&state.db, cert_id).await {
                Ok(record) => record,
                Err(err) => return Json(ApiResponse::err(&err)),
            };
            sqlx::query(
                "UPDATE sites SET ssl_enabled = 1, ssl_cert_path = ?, ssl_key_path = ?,
                 force_https = ?, hsts = ?, updated_at = datetime('now') WHERE id = ?",
            )
            .bind(&record.cert_path)
            .bind(&record.key_path)
            .bind(body.force_https)
            .bind(body.hsts)
            .bind(site_id)
            .execute(&state.db)
            .await
        }
        None => {
            sqlx::query(
                "UPDATE sites SET ssl_enabled = 0, ssl_cert_path = NULL, ssl_key_path = NULL,
                 force_https = 0, hsts = 0, updated_at = datetime('now') WHERE id = ?",
            )
            .bind(site_id)
            .execute(&state.db)
            .await
        }
    };

    if let Err(err) = result {
        return Json(ApiResponse::err(&err.to_string()));
    }

    if let Err(err) = openresty::sync_site(state.as_ref(), site_id).await {
        return Json(ApiResponse::err(&format!(
            "OpenResty apply failed: {}",
            err
        )));
    }
    Json(ApiResponse::ok_msg("SSL settings updated"))
}

/// Re-syncs every site bound to a certificate, so a renewal takes effect immediately.
async fn reload_sites_using(state: &Arc<AppState>, cert_id: i64) {
    let Ok(record) = cert::get(&state.db, cert_id).await else {
        return;
    };
    let sites: Vec<(i64,)> = sqlx::query_as("SELECT id FROM sites WHERE ssl_cert_path = ?")
        .bind(&record.cert_path)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    for (site_id,) in sites {
        if let Err(err) = openresty::sync_site(state.as_ref(), site_id).await {
            tracing::warn!(
                "Failed to reload site {} after certificate renewal: {}",
                site_id,
                err
            );
        }
    }
}

/// Daily auto-renewal sweep.
///
/// ACME certificates live ~90 days, so a panel that never renews them silently
/// breaks every site it issued for. This runs on a timer, renews anything inside
/// the renewal window, and reloads the sites bound to it.
pub async fn renewal_task(
    state: Arc<AppState>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    /// Certificates change on the order of months, so a daily sweep is plenty.
    const INTERVAL: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);
    /// Let the panel finish starting before the first sweep.
    const STARTUP_DELAY: std::time::Duration = std::time::Duration::from_secs(60);

    tokio::select! {
        _ = tokio::time::sleep(STARTUP_DELAY) => {}
        _ = shutdown.changed() => return,
    }

    loop {
        for record in cert::due_for_renewal(&state.db).await {
            let days = cert::days_until(&record.not_after).unwrap_or(0);
            tracing::info!(
                "Auto-renewing certificate '{}' ({} days remaining)",
                record.name,
                days
            );
            match cert::renew(&state.db, record.id).await {
                Ok(result) if result.success => {
                    tracing::info!("Renewed certificate '{}'", record.name);
                    reload_sites_using(&state, record.id).await;
                }
                Ok(result) => tracing::warn!(
                    "Renewal of '{}' did not succeed:\n{}",
                    record.name,
                    result.log
                ),
                Err(err) => tracing::warn!("Renewal of '{}' failed: {}", record.name, err),
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(INTERVAL) => {}
            _ = shutdown.changed() => return,
        }
    }
}
