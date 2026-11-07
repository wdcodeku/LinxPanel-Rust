//! Website backups and what scheduled backups need to know.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::models::{ApiResponse, Site};
use crate::services::{backup, database::BackupFile};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/sites/{id}/backups",
            get(list_site_backups).post(create_site_backup),
        )
        .route("/sites/{id}/backups/restore", post(restore_site_backup))
        .route("/sites/{id}/backups/delete", post(delete_site_backup))
        .route("/backups/cron-prefix", get(cron_prefix))
        .route(
            "/backups/remote",
            get(remote_config).post(save_remote_config),
        )
        .route("/backups/remote/test", post(test_remote))
        .route("/backups/remote/history", get(remote_history))
}

async fn site(state: &AppState, id: i64) -> Result<Site, String> {
    sqlx::query_as::<_, Site>("SELECT * FROM sites WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Site not found".to_string())
}

async fn list_site_backups(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<Vec<BackupFile>>> {
    Json(ApiResponse::ok(backup::list_site_backups(
        &state.data_dir,
        id,
    )))
}

#[derive(Deserialize, Default)]
struct BackupRequest {
    #[serde(default)]
    keep: Option<usize>,
}

async fn create_site_backup(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    body: Option<Json<BackupRequest>>,
) -> Json<ApiResponse<serde_json::Value>> {
    let site = match site(&state, id).await {
        Ok(site) => site,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    let keep = body.and_then(|Json(b)| b.keep);
    let data_dir = state.data_dir.clone();
    let result = tokio::task::spawn_blocking(move || {
        backup::backup_site(
            &data_dir,
            site.id,
            &site.name,
            std::path::Path::new(&site.doc_root),
            keep,
        )
    })
    .await;
    match result {
        Ok(Ok(path)) => Json(ApiResponse::ok(
            serde_json::json!({ "path": path.to_string_lossy() }),
        )),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Deserialize)]
struct FileRequest {
    file: String,
}

async fn restore_site_backup(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<FileRequest>,
) -> Json<ApiResponse<()>> {
    let site = match site(&state, id).await {
        Ok(site) => site,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    let data_dir = state.data_dir.clone();
    let result = tokio::task::spawn_blocking(move || {
        backup::restore_site(
            &data_dir,
            site.id,
            std::path::Path::new(&site.doc_root),
            &req.file,
        )
    })
    .await;
    match result {
        Ok(Ok(())) => Json(ApiResponse::ok_msg("Site restored")),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

async fn delete_site_backup(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<FileRequest>,
) -> Json<ApiResponse<()>> {
    match backup::delete_site_backup(&state.data_dir, id, &req.file) {
        Ok(()) => Json(ApiResponse::ok_msg("Backup deleted")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn cron_prefix() -> Json<ApiResponse<serde_json::Value>> {
    Json(ApiResponse::ok(
        serde_json::json!({ "prefix": backup::cron_command_prefix() }),
    ))
}

// ===== Remote storage =====

use crate::services::remote_backup;

async fn remote_config(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<remote_backup::Config>> {
    Json(ApiResponse::ok(remote_backup::masked(
        remote_backup::load(&state.db).await,
    )))
}

async fn save_remote_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<remote_backup::Config>,
) -> Json<ApiResponse<()>> {
    match remote_backup::save(&state.db, req).await {
        Ok(()) => Json(ApiResponse::ok_msg("Saved")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

/// Uploads a small file with the given settings and removes it again.
async fn test_remote(
    State(state): State<Arc<AppState>>,
    Json(req): Json<remote_backup::Config>,
) -> Json<ApiResponse<String>> {
    let config = remote_backup::merge_secrets(&state.db, req).await;
    match tokio::task::spawn_blocking(move || remote_backup::test(&config)).await {
        Ok(Ok(key)) => Json(ApiResponse::ok(key)),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(serde::Serialize, sqlx::FromRow)]
struct RemoteRow {
    id: i64,
    kind: String,
    group_key: String,
    object_key: String,
    size: i64,
    uploaded_at: String,
}

async fn remote_history(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<RemoteRow>>> {
    match sqlx::query_as::<_, RemoteRow>("SELECT * FROM remote_backups ORDER BY id DESC LIMIT 100")
        .fetch_all(&state.db)
        .await
    {
        Ok(rows) => Json(ApiResponse::ok(rows)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}
