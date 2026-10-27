//! Database servers and databases: install engines, create/delete databases and
//! users, rotate passwords, back up and restore.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::ApiResponse;
use crate::services::database::{self, Engine, MysqlAdmin};
use crate::services::tasks;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/databases/engines", get(engines))
        .route("/databases/engines/{engine}/install", post(install))
        .route("/databases/engines/{engine}/restart", post(restart))
        .route("/databases/engines/mysql/admin", post(set_mysql_admin))
        .route(
            "/databases/engines/redis/password",
            post(set_redis_password),
        )
        .route("/databases", get(list).post(create))
        .route("/databases/{engine}/{name}/delete", post(delete))
        .route("/databases/{engine}/{name}/password", post(change_password))
        .route(
            "/databases/{engine}/{name}/backups",
            get(list_backups).post(create_backup),
        )
        .route("/databases/{engine}/{name}/restore", post(restore))
        .route(
            "/databases/{engine}/{name}/backups/delete",
            post(delete_backup),
        )
}

async fn admin(state: &AppState) -> Option<MysqlAdmin> {
    database::load_mysql_admin(&state.db).await
}

fn parse_engine(raw: &str) -> Result<Engine, Json<ApiResponse<()>>> {
    Engine::parse(raw).map_err(|e| Json(ApiResponse::err(&e)))
}

#[derive(Serialize)]
struct EngineInfo {
    #[serde(flatten)]
    status: database::EngineStatus,
    /// Redis only: the configured `requirepass`.
    #[serde(skip_serializing_if = "Option::is_none")]
    redis_password: Option<String>,
}

async fn engines(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<EngineInfo>>> {
    let admin = admin(&state).await;
    let result = tokio::task::spawn_blocking(move || {
        database::ENGINES
            .iter()
            .map(|engine| EngineInfo {
                status: database::status(*engine, admin.as_ref()),
                redis_password: (*engine == Engine::Redis)
                    .then(database::redis_password)
                    .flatten(),
            })
            .collect::<Vec<_>>()
    })
    .await;
    match result {
        Ok(list) => Json(ApiResponse::ok(list)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Serialize)]
struct LogResult {
    success: bool,
    log: String,
}

/// Starts the install as a background task (its log streams to the browser).
async fn install(
    State(state): State<Arc<AppState>>,
    Path(engine): Path<String>,
) -> Json<ApiResponse<tasks::Started>> {
    let engine = match Engine::parse(&engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    let title = format!("Install {}", engine.as_str());
    let task_state = state.clone();
    Json(ApiResponse::ok(state.tasks.start(
        "db-install",
        &title,
        install_task(task_state, engine),
    )))
}

async fn install_task(state: Arc<AppState>, engine: Engine) -> ApiResponse<LogResult> {
    let (log, created_admin) = match tasks::blocking(move || database::install(engine)).await {
        Ok(result) => result,
        Err(err) => return ApiResponse::err(&err.to_string()),
    };
    if let Some(admin) = created_admin {
        if let Err(err) = database::save_mysql_admin(&state.db, &admin).await {
            return ApiResponse::err(&format!("Cannot save MySQL login: {}", err));
        }
    }
    let admin = database::load_mysql_admin(&state.db).await;
    let success = tasks::blocking(move || database::status(engine, admin.as_ref()).running)
        .await
        .unwrap_or(false);
    ApiResponse::ok(LogResult { success, log })
}

async fn restart(Path(engine): Path<String>) -> Json<ApiResponse<()>> {
    let engine = match parse_engine(&engine) {
        Ok(engine) => engine,
        Err(err) => return err,
    };
    match tokio::task::spawn_blocking(move || database::restart(engine)).await {
        Ok(Ok(_)) => Json(ApiResponse::ok_msg("Restarted")),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Deserialize)]
struct MysqlAdminRequest {
    /// Empty: try root's passwordless socket login and create the panel's account.
    #[serde(default)]
    user: String,
    #[serde(default)]
    password: String,
}

/// Connects the panel to a MySQL it did not install (or repairs the stored login).
async fn set_mysql_admin(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MysqlAdminRequest>,
) -> Json<ApiResponse<()>> {
    let result = tokio::task::spawn_blocking(move || {
        if req.user.trim().is_empty() {
            database::bootstrap_mysql_admin()
        } else {
            let admin = MysqlAdmin {
                user: req.user.trim().to_string(),
                password: req.password,
            };
            database::verify_mysql_admin(&admin).map(|_| admin)
        }
    })
    .await;
    match result {
        Ok(Ok(admin)) => match database::save_mysql_admin(&state.db, &admin).await {
            Ok(()) => Json(ApiResponse::ok_msg("MySQL connected")),
            Err(err) => Json(ApiResponse::err(&err.to_string())),
        },
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Deserialize)]
struct RedisPasswordRequest {
    #[serde(default)]
    password: String,
}

async fn set_redis_password(Json(req): Json<RedisPasswordRequest>) -> Json<ApiResponse<()>> {
    let password = req.password.trim().to_string();
    match tokio::task::spawn_blocking(move || database::set_redis_password(&password)).await {
        Ok(Ok(())) => Json(ApiResponse::ok_msg("Redis password updated")),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Deserialize)]
struct ListQuery {
    engine: String,
}

async fn list(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> Json<ApiResponse<Vec<database::DatabaseRow>>> {
    let engine = match Engine::parse(&query.engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    match database::list(&state.db, engine, admin(&state).await).await {
        Ok(rows) => Json(ApiResponse::ok(rows)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<database::CreateDatabaseRequest>,
) -> Json<ApiResponse<database::CreatedDatabase>> {
    match database::create(&state.db, req, admin(&state).await).await {
        Ok(created) => Json(ApiResponse::ok(created)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn delete(
    State(state): State<Arc<AppState>>,
    Path((engine, name)): Path<(String, String)>,
) -> Json<ApiResponse<()>> {
    let engine = match parse_engine(&engine) {
        Ok(engine) => engine,
        Err(err) => return err,
    };
    match database::delete(&state.db, engine, &name, admin(&state).await).await {
        Ok(()) => Json(ApiResponse::ok_msg("Database deleted")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

#[derive(Deserialize)]
struct PasswordRequest {
    /// Generated when empty.
    #[serde(default)]
    password: Option<String>,
}

async fn change_password(
    State(state): State<Arc<AppState>>,
    Path((engine, name)): Path<(String, String)>,
    Json(req): Json<PasswordRequest>,
) -> Json<ApiResponse<serde_json::Value>> {
    let engine = match Engine::parse(&engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    match database::change_password(&state.db, engine, &name, req.password, admin(&state).await)
        .await
    {
        Ok(password) => Json(ApiResponse::ok(serde_json::json!({ "password": password }))),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn list_backups(
    State(state): State<Arc<AppState>>,
    Path((engine, name)): Path<(String, String)>,
) -> Json<ApiResponse<Vec<database::BackupFile>>> {
    let engine = match Engine::parse(&engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    if let Err(err) = database::validate_name(&name, "Database name") {
        return Json(ApiResponse::err(&err));
    }
    let dir = database::backup_dir(&state.data_dir, engine, &name);
    Json(ApiResponse::ok(database::list_backups(&dir)))
}

#[derive(Deserialize, Default)]
struct BackupRequest {
    /// Keep only this many newest backups.
    #[serde(default)]
    keep: Option<usize>,
}

async fn create_backup(
    State(state): State<Arc<AppState>>,
    Path((engine, name)): Path<(String, String)>,
    body: Option<Json<BackupRequest>>,
) -> Json<ApiResponse<serde_json::Value>> {
    let engine = match Engine::parse(&engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    let keep = body.and_then(|Json(b)| b.keep);
    let admin = admin(&state).await;
    let data_dir = state.data_dir.clone();
    let result = tokio::task::spawn_blocking(move || {
        database::backup(&data_dir, engine, &name, admin.as_ref(), keep)
    })
    .await;
    match result {
        Ok(Ok(path)) => Json(ApiResponse::ok(serde_json::json!({
            "path": path.to_string_lossy(),
        }))),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Deserialize)]
struct BackupFileRequest {
    file: String,
}

async fn restore(
    State(state): State<Arc<AppState>>,
    Path((engine, name)): Path<(String, String)>,
    Json(req): Json<BackupFileRequest>,
) -> Json<ApiResponse<()>> {
    let engine = match parse_engine(&engine) {
        Ok(engine) => engine,
        Err(err) => return err,
    };
    let admin = admin(&state).await;
    let data_dir = state.data_dir.clone();
    let result = tokio::task::spawn_blocking(move || {
        database::restore(&data_dir, engine, &name, &req.file, admin.as_ref())
    })
    .await;
    match result {
        Ok(Ok(())) => Json(ApiResponse::ok_msg("Database restored")),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

async fn delete_backup(
    State(state): State<Arc<AppState>>,
    Path((engine, name)): Path<(String, String)>,
    Json(req): Json<BackupFileRequest>,
) -> Json<ApiResponse<()>> {
    let engine = match parse_engine(&engine) {
        Ok(engine) => engine,
        Err(err) => return err,
    };
    if let Err(err) = database::validate_name(&name, "Database name")
        .and_then(|_| database::validate_backup_file(&req.file))
    {
        return Json(ApiResponse::err(&err));
    }
    let path = database::backup_dir(&state.data_dir, engine, &name).join(&req.file);
    match std::fs::remove_file(&path) {
        Ok(()) => Json(ApiResponse::ok_msg("Backup deleted")),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}
