//! Database servers and databases: install engines, create/delete databases and
//! users, rotate passwords, back up and restore.

use axum::{
    extract::{ConnectInfo, Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;

use crate::models::ApiResponse;
use crate::services::database::{self, Engine, MysqlAdmin};
use crate::services::{dbconf, dbnet, sqlitedb, tasks};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/databases/engines", get(engines))
        .route("/databases/engines/{engine}/install", post(install))
        .route("/databases/engines/{engine}/versions", get(versions))
        .route("/databases/engines/{engine}/network", get(network))
        .route("/databases/engines/{engine}/remote", post(set_remote))
        .route(
            "/databases/engines/{engine}/settings",
            get(engine_settings).post(apply_settings),
        )
        .route(
            "/databases/engines/{engine}/superuser-password",
            post(set_superuser_password),
        )
        .route("/databases/engines/{engine}/start", post(start_engine))
        .route("/databases/engines/{engine}/stop", post(stop_engine))
        .route(
            "/databases/engines/{engine}/uninstall",
            post(uninstall_engine),
        )
        .route(
            "/databases/engines/{engine}/config",
            get(read_config).post(write_config),
        )
        .route("/databases/engines/{engine}/logs/{kind}", get(read_log))
        .route(
            "/databases/engines/{engine}/logs/{kind}/clear",
            post(clear_log),
        )
        .route("/databases/engines/mysql/binlog/purge", post(purge_binlogs))
        .route("/databases/engines/{engine}/restart", post(restart))
        .route("/databases/engines/{engine}/admin", post(set_admin))
        .route(
            "/databases/engines/redis/password",
            post(set_redis_password),
        )
        .route("/databases", get(list).post(create))
        .route("/databases/{engine}/{name}/delete", post(delete))
        .route("/databases/{engine}/{name}/tables", get(tables))
        .route("/databases/{engine}/{name}/adopt", post(adopt))
        .route("/databases/{engine}/{name}/password", post(change_password))
        .route("/databases/mysql/{name}/host", post(change_host))
        .route(
            "/databases/{engine}/{name}/backups",
            get(list_backups).post(create_backup),
        )
        .route("/databases/{engine}/{name}/restore", post(restore))
        .route(
            "/databases/{engine}/{name}/backups/delete",
            post(delete_backup),
        )
        .route("/sqlite", get(sqlite_list))
        .route("/sqlite/discover", get(sqlite_discover))
        .route("/sqlite/register", post(sqlite_register))
        .route("/sqlite/create", post(sqlite_create))
        .route("/sqlite/remove", post(sqlite_remove))
        .route("/sqlite/tables", get(sqlite_tables))
        .route(
            "/sqlite/backups",
            get(sqlite_backups).post(sqlite_backup),
        )
        .route("/sqlite/restore", post(sqlite_restore))
        .route("/sqlite/backups/delete", post(sqlite_delete_backup))
}

/// The panel's administrator login for the engine (MySQL / MongoDB).
async fn admin(state: &AppState, engine: Engine) -> Option<MysqlAdmin> {
    database::load_admin(&state.db, engine).await
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
    let mysql_admin = admin(&state, Engine::Mysql).await;
    let mongo_admin = admin(&state, Engine::Mongodb).await;
    let result = tokio::task::spawn_blocking(move || {
        database::ENGINES
            .iter()
            .map(|engine| EngineInfo {
                status: database::status(
                    *engine,
                    match engine {
                        Engine::Mongodb => mongo_admin.as_ref(),
                        _ => mysql_admin.as_ref(),
                    },
                ),
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

#[derive(Deserialize, Default)]
struct InstallRequest {
    /// An id from the versions endpoint; empty for the distro default.
    #[serde(default)]
    version: String,
}

/// Starts the install as a background task (its log streams to the browser).
async fn install(
    State(state): State<Arc<AppState>>,
    Path(engine): Path<String>,
    body: Option<Json<InstallRequest>>,
) -> Json<ApiResponse<tasks::Started>> {
    let engine = match Engine::parse(&engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    let version = body.map(|Json(b)| b.version).unwrap_or_default();
    let title = format!("Install {}", engine.as_str());
    let task_state = state.clone();
    Json(ApiResponse::ok(state.tasks.start(
        "db-install",
        &title,
        install_task(task_state, engine, version),
    )))
}

async fn versions(Path(engine): Path<String>) -> Json<ApiResponse<Vec<dbnet::VersionOption>>> {
    match Engine::parse(&engine) {
        Ok(engine) => Json(ApiResponse::ok(
            tokio::task::spawn_blocking(move || dbnet::versions(engine))
                .await
                .unwrap_or_default(),
        )),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

#[derive(Serialize)]
struct NetworkView {
    #[serde(flatten)]
    info: dbnet::NetworkInfo,
    /// The visitor's address, offered as the allowed source.
    your_ip: String,
}

async fn network(
    Path(engine): Path<String>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Json<ApiResponse<NetworkView>> {
    let engine = match Engine::parse(&engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    match tokio::task::spawn_blocking(move || dbnet::network_info(engine)).await {
        Ok(info) => Json(ApiResponse::ok(NetworkView {
            info,
            your_ip: peer.ip().to_canonical().to_string(),
        })),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Deserialize)]
struct RemoteRequest {
    enabled: bool,
    /// Limit the firewall rule to this IP / CIDR; empty for any.
    #[serde(default)]
    allow_from: String,
}

async fn set_remote(
    Path(engine): Path<String>,
    Json(req): Json<RemoteRequest>,
) -> Json<ApiResponse<String>> {
    let engine = match Engine::parse(&engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    let allow_from = req.allow_from.trim().to_string();
    if !allow_from.is_empty() {
        if let Err(err) = crate::services::panel_access::parse_ip_list(&allow_from) {
            return Json(ApiResponse::err(&err));
        }
    }
    let result = tokio::task::spawn_blocking(move || {
        let allow_from = Some(allow_from.as_str()).filter(|s| !s.is_empty());
        match engine {
            Engine::Mysql => dbnet::set_mysql_remote(req.enabled, allow_from),
            _ => dbconf::set_remote(engine, req.enabled, allow_from),
        }
    })
    .await;
    match result {
        Ok(Ok(log)) => Json(ApiResponse::ok(log)),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Deserialize)]
struct HostRequest {
    host: String,
}

async fn change_host(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(req): Json<HostRequest>,
) -> Json<ApiResponse<()>> {
    let admin = database::load_mysql_admin(&state.db).await;
    match database::change_host(&state.db, &name, &req.host, admin).await {
        Ok(()) => Json(ApiResponse::ok_msg("Access host changed")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn install_task(
    state: Arc<AppState>,
    engine: Engine,
    version: String,
) -> ApiResponse<LogResult> {
    let (log, created_admin) = match tasks::blocking(move || {
        database::install(engine, Some(version.as_str()).filter(|v| !v.is_empty()))
    })
    .await
    {
        Ok(result) => result,
        Err(err) => return ApiResponse::err(&err.to_string()),
    };
    if let Some(admin) = created_admin {
        if let Err(err) = database::save_admin(&state.db, engine, &admin).await {
            return ApiResponse::err(&format!(
                "Cannot save the {} login: {}",
                engine.display_name(),
                err
            ));
        }
    }
    let admin = admin(&state, engine).await;
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

/// Connects the panel to a MySQL / MongoDB it did not install (or repairs
/// the stored login).
async fn set_admin(
    State(state): State<Arc<AppState>>,
    Path(engine): Path<String>,
    Json(req): Json<MysqlAdminRequest>,
) -> Json<ApiResponse<()>> {
    let engine = match parse_engine(&engine) {
        Ok(engine @ (Engine::Mysql | Engine::Mongodb)) => engine,
        Ok(_) => return Json(ApiResponse::err("This engine has no administrator login")),
        Err(err) => return err,
    };
    let result = tokio::task::spawn_blocking(move || {
        let given = MysqlAdmin {
            user: req.user.trim().to_string(),
            password: req.password,
        };
        match (engine, given.user.is_empty()) {
            (Engine::Mongodb, true) => crate::services::mongo::bootstrap_admin(),
            (Engine::Mongodb, false) => {
                crate::services::mongo::ping(Some(&given)).map(|_| given)
            }
            (_, true) => database::bootstrap_mysql_admin(),
            (_, false) => database::verify_mysql_admin(&given).map(|_| given),
        }
    })
    .await;
    match result {
        Ok(Ok(admin)) => match database::save_admin(&state.db, engine, &admin).await {
            Ok(()) => Json(ApiResponse::ok_msg("Connected")),
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
    match database::list(&state.db, engine, admin(&state, engine).await).await {
        Ok(rows) => Json(ApiResponse::ok(rows)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<database::CreateDatabaseRequest>,
) -> Json<ApiResponse<database::CreatedDatabase>> {
    let engine = Engine::parse(&req.engine).unwrap_or(Engine::Mysql);
    let admin = admin(&state, engine).await;
    match database::create(&state.db, req, admin).await {
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
    match database::delete(&state.db, engine, &name, admin(&state, engine).await).await {
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
    match database::change_password(&state.db, engine, &name, req.password, admin(&state, engine).await)
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
    let admin = admin(&state, engine).await;
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
    let admin = admin(&state, engine).await;
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

/// The tables of a database, including ones created outside the panel.
async fn tables(
    State(state): State<Arc<AppState>>,
    Path((engine, name)): Path<(String, String)>,
) -> Json<ApiResponse<Vec<database::TableInfo>>> {
    let engine = match Engine::parse(&engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    match database::tables(engine, &name, admin(&state, engine).await).await {
        Ok(list) => Json(ApiResponse::ok(list)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

/// Gives a database created outside the panel a user the panel manages.
async fn adopt(
    State(state): State<Arc<AppState>>,
    Path((engine, name)): Path<(String, String)>,
    Json(req): Json<database::AdoptRequest>,
) -> Json<ApiResponse<database::CreatedDatabase>> {
    let engine = match Engine::parse(&engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    let admin = admin(&state, engine).await;
    match database::adopt(&state.db, engine, &name, req, admin).await {
        Ok(created) => Json(ApiResponse::ok(created)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

// ===== Engine settings =====

async fn engine_settings(
    State(state): State<Arc<AppState>>,
    Path(engine): Path<String>,
) -> Json<ApiResponse<dbconf::EngineSettings>> {
    match Engine::parse(&engine) {
        Ok(engine) => Json(ApiResponse::ok(dbconf::settings(&state.db, engine).await)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

/// Parameters and log switches; the server restarts (and rolls back if it fails).
async fn apply_settings(
    State(state): State<Arc<AppState>>,
    Path(engine): Path<String>,
    Json(change): Json<dbconf::SettingsChange>,
) -> Json<ApiResponse<String>> {
    let engine = match Engine::parse(&engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    match dbconf::apply(&state.db, engine, change).await {
        Ok(log) => Json(ApiResponse::ok(log)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

#[derive(Deserialize)]
struct SuperuserPasswordRequest {
    #[serde(default)]
    password: String,
}

async fn set_superuser_password(
    State(state): State<Arc<AppState>>,
    Path(engine): Path<String>,
    Json(req): Json<SuperuserPasswordRequest>,
) -> Json<ApiResponse<()>> {
    let engine = match Engine::parse(&engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    match dbconf::set_superuser_password(&state.db, engine, &req.password).await {
        Ok(()) => Json(ApiResponse::ok_msg("Password updated")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn start_engine(Path(engine): Path<String>) -> Json<ApiResponse<()>> {
    let engine = match parse_engine(&engine) {
        Ok(engine) => engine,
        Err(err) => return err,
    };
    match tokio::task::spawn_blocking(move || dbconf::start(engine)).await {
        Ok(Ok(_)) => Json(ApiResponse::ok_msg("Started")),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

async fn stop_engine(Path(engine): Path<String>) -> Json<ApiResponse<()>> {
    let engine = match parse_engine(&engine) {
        Ok(engine) => engine,
        Err(err) => return err,
    };
    match tokio::task::spawn_blocking(move || dbconf::stop(engine)).await {
        Ok(Ok(_)) => Json(ApiResponse::ok_msg("Stopped")),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Deserialize, Default)]
struct UninstallRequest {
    /// Also delete the data directory (every database on this server).
    #[serde(default)]
    purge_data: bool,
}

/// Removes the engine as a background task (its log streams to the browser).
async fn uninstall_engine(
    State(state): State<Arc<AppState>>,
    Path(engine): Path<String>,
    Json(req): Json<UninstallRequest>,
) -> Json<ApiResponse<tasks::Started>> {
    let engine = match Engine::parse(&engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    let title = format!("Uninstall {}", engine.as_str());
    let task_state = state.clone();
    Json(ApiResponse::ok(state.tasks.start(
        "db-uninstall",
        &title,
        uninstall_task(task_state, engine, req.purge_data),
    )))
}

async fn uninstall_task(
    state: Arc<AppState>,
    engine: Engine,
    purge_data: bool,
) -> ApiResponse<LogResult> {
    match tasks::blocking(move || dbconf::uninstall(engine, purge_data)).await {
        Ok(Ok(log)) => {
            dbconf::forget(&state.db, engine, purge_data).await;
            ApiResponse::ok(LogResult { success: true, log })
        }
        Ok(Err(err)) => ApiResponse::err(&err),
        Err(err) => ApiResponse::err(&err.to_string()),
    }
}

#[derive(Deserialize)]
struct ConfigQuery {
    #[serde(default)]
    file: String,
}

#[derive(Serialize)]
struct ConfigView {
    files: Vec<dbconf::ConfigFile>,
    file: Option<dbconf::ConfigFile>,
    content: String,
}

async fn read_config(
    Path(engine): Path<String>,
    Query(query): Query<ConfigQuery>,
) -> Json<ApiResponse<ConfigView>> {
    let engine = match Engine::parse(&engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    let result = tokio::task::spawn_blocking(move || {
        let files = dbconf::config_files(engine);
        let id = if query.file.is_empty() {
            files.first().map(|f| f.id).unwrap_or("main").to_string()
        } else {
            query.file
        };
        match dbconf::read_config(engine, &id) {
            Ok((file, content)) => Ok(ConfigView {
                files,
                file: Some(file),
                content,
            }),
            Err(_) if files.is_empty() => Ok(ConfigView {
                files,
                file: None,
                content: String::new(),
            }),
            Err(err) => Err(err),
        }
    })
    .await;
    match result {
        Ok(Ok(view)) => Json(ApiResponse::ok(view)),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Deserialize)]
struct ConfigWrite {
    file: String,
    content: String,
}

async fn write_config(
    Path(engine): Path<String>,
    Json(req): Json<ConfigWrite>,
) -> Json<ApiResponse<String>> {
    let engine = match Engine::parse(&engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    match tokio::task::spawn_blocking(move || {
        dbconf::write_config_file(engine, &req.file, &req.content)
    })
    .await
    {
        Ok(Ok(log)) => Json(ApiResponse::ok(log)),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}

#[derive(Deserialize)]
struct LogQuery {
    #[serde(default = "default_lines")]
    lines: usize,
}

fn default_lines() -> usize {
    300
}

async fn read_log(
    State(state): State<Arc<AppState>>,
    Path((engine, kind)): Path<(String, String)>,
    Query(query): Query<LogQuery>,
) -> Json<ApiResponse<String>> {
    let engine = match Engine::parse(&engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    match dbconf::tail_log(&state.db, engine, &kind, query.lines).await {
        Ok(text) => Json(ApiResponse::ok(text)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn clear_log(
    State(state): State<Arc<AppState>>,
    Path((engine, kind)): Path<(String, String)>,
) -> Json<ApiResponse<()>> {
    let engine = match Engine::parse(&engine) {
        Ok(engine) => engine,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    match dbconf::clear_log(&state.db, engine, &kind).await {
        Ok(()) => Json(ApiResponse::ok_msg("Log cleared")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

// ===== SQLite (files, managed by path) =====

#[derive(Deserialize)]
struct SqlitePathQuery {
    path: String,
}

#[derive(Deserialize)]
struct SqliteRequest {
    path: String,
    /// remove: also delete the file.
    #[serde(default)]
    delete_file: bool,
    /// backup: keep only this many newest backups.
    #[serde(default)]
    keep: Option<usize>,
    /// restore / delete backup: the backup's file name.
    #[serde(default)]
    file: String,
}

fn result<T: Serialize>(value: Result<T, String>) -> Json<ApiResponse<T>> {
    match value {
        Ok(value) => Json(ApiResponse::ok(value)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn sqlite_list(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<Vec<sqlitedb::SqliteFile>>> {
    Json(ApiResponse::ok(sqlitedb::list(&state.db).await))
}

async fn sqlite_discover(
    State(state): State<Arc<AppState>>,
) -> Json<ApiResponse<Vec<sqlitedb::Found>>> {
    let known = sqlitedb::registered(&state.db).await;
    let data_dir = state.data_dir.clone();
    result(
        tokio::task::spawn_blocking(move || sqlitedb::discover(&data_dir, &known))
            .await
            .map_err(|e| e.to_string()),
    )
}

async fn sqlite_register(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SqliteRequest>,
) -> Json<ApiResponse<sqlitedb::SqliteFile>> {
    result(sqlitedb::register(&state.db, &state.data_dir, &req.path).await)
}

async fn sqlite_create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SqliteRequest>,
) -> Json<ApiResponse<sqlitedb::SqliteFile>> {
    result(sqlitedb::create(&state.db, &state.data_dir, &req.path).await)
}

async fn sqlite_remove(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SqliteRequest>,
) -> Json<ApiResponse<()>> {
    match sqlitedb::remove(&state.db, &state.data_dir, &req.path, req.delete_file).await {
        Ok(()) => Json(ApiResponse::ok_msg("Removed")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn sqlite_tables(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SqlitePathQuery>,
) -> Json<ApiResponse<Vec<database::TableInfo>>> {
    result(sqlitedb::tables(&state.db, &state.data_dir, &query.path).await)
}

async fn sqlite_backups(
    State(state): State<Arc<AppState>>,
    Query(query): Query<SqlitePathQuery>,
) -> Json<ApiResponse<Vec<database::BackupFile>>> {
    result(sqlitedb::list_backups(&state.db, &state.data_dir, &query.path).await)
}

async fn sqlite_backup(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SqliteRequest>,
) -> Json<ApiResponse<serde_json::Value>> {
    result(
        sqlitedb::backup(&state.db, &state.data_dir, &req.path, req.keep)
            .await
            .map(|path| serde_json::json!({ "path": path.to_string_lossy() })),
    )
}

async fn sqlite_restore(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SqliteRequest>,
) -> Json<ApiResponse<()>> {
    match sqlitedb::restore(&state.db, &state.data_dir, &req.path, &req.file).await {
        Ok(()) => Json(ApiResponse::ok_msg("Database restored")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn sqlite_delete_backup(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SqliteRequest>,
) -> Json<ApiResponse<()>> {
    match sqlitedb::delete_backup(&state.db, &state.data_dir, &req.path, &req.file).await {
        Ok(()) => Json(ApiResponse::ok_msg("Backup deleted")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

/// Deletes MySQL binary logs except the current one; returns the bytes freed.
async fn purge_binlogs(State(state): State<Arc<AppState>>) -> Json<ApiResponse<u64>> {
    let admin = database::load_mysql_admin(&state.db).await;
    match tokio::task::spawn_blocking(move || dbconf::mysql_purge_binlogs(admin.as_ref())).await {
        Ok(Ok(freed)) => Json(ApiResponse::ok(freed)),
        Ok(Err(err)) => Json(ApiResponse::err(&err)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}
