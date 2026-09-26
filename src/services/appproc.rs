//! Managed application processes.
//!
//! A site backed by Node / Python / Java / Go / a plain binary needs something to
//! actually run it. This module supervises those processes: it starts them, keeps
//! their output in a log file, restarts them when they die, and stops them cleanly.
//!
//! Supervision is in-process rather than via systemd so it works without root and
//! on any platform the panel runs on.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, Mutex};

use super::runtime;

/// How long to wait after a crash before restarting, and the ceiling for backoff.
const RESTART_DELAY: Duration = Duration::from_secs(2);
const RESTART_DELAY_MAX: Duration = Duration::from_secs(60);
/// A process that survives this long is considered healthy, so backoff resets.
const HEALTHY_AFTER: Duration = Duration::from_secs(30);
/// Cap the on-disk log so a chatty app cannot fill the disk.
const LOG_MAX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct AppRecord {
    pub id: i64,
    pub name: String,
    /// node | python | java | go | binary | php
    pub runtime: String,
    pub work_dir: String,
    /// Script, jar or executable, relative to work_dir unless absolute.
    pub entry: String,
    pub args: String,
    /// One KEY=VALUE per line.
    pub env: String,
    pub port: i64,
    pub autostart: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct SaveAppRequest {
    pub name: String,
    pub runtime: String,
    pub work_dir: String,
    pub entry: String,
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    pub env: String,
    #[serde(default)]
    pub port: i64,
    #[serde(default)]
    pub autostart: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppState_ {
    pub running: bool,
    pub pid: Option<u32>,
    /// Unix seconds; only set while running.
    pub started_at: Option<i64>,
    pub restarts: u32,
    pub last_error: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppView {
    #[serde(flatten)]
    pub record: AppRecord,
    #[serde(flatten)]
    pub state: AppState_,
}

struct Running {
    pid: u32,
    started_at: i64,
    restarts: u32,
    last_error: String,
    /// Dropping or sending on this tells the supervisor to stop without restarting.
    stop_tx: Option<oneshot::Sender<()>>,
}

#[derive(Default)]
pub struct Supervisor {
    running: Mutex<HashMap<i64, Running>>,
}

impl Supervisor {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn state_of(&self, id: i64) -> AppState_ {
        let guard = self.running.lock().await;
        match guard.get(&id) {
            Some(entry) => AppState_ {
                running: true,
                pid: Some(entry.pid),
                started_at: Some(entry.started_at),
                restarts: entry.restarts,
                last_error: entry.last_error.clone(),
            },
            None => AppState_ {
                running: false,
                pid: None,
                started_at: None,
                restarts: 0,
                last_error: String::new(),
            },
        }
    }

    pub async fn is_running(&self, id: i64) -> bool {
        self.running.lock().await.contains_key(&id)
    }

    /// Ids of every supervised application, newest first.
    pub async fn running_ids(&self) -> Vec<i64> {
        self.running.lock().await.keys().copied().collect()
    }
}

/// Stops every supervised application.
///
/// Without this, killing the panel orphans its children: they keep holding their
/// ports, and the next boot autostarts a duplicate that cannot bind.
pub async fn stop_all(supervisor: Arc<Supervisor>) {
    let ids = supervisor.running_ids().await;
    if ids.is_empty() {
        return;
    }
    tracing::info!("Stopping {} managed application(s)", ids.len());
    for id in ids {
        if let Err(err) = stop(supervisor.clone(), id).await {
            tracing::warn!("Failed to stop application {}: {}", id, err);
        }
    }
    // Give the supervisor tasks a moment to reap their children before the process exits.
    tokio::time::sleep(Duration::from_millis(400)).await;
}

/// Resolves the log file for an app. Callers create the parent directory.
pub fn log_path(data_dir: &std::path::Path, id: i64) -> std::path::PathBuf {
    data_dir.join("app-logs").join(format!("app-{}.log", id))
}

fn validate(request: &SaveAppRequest) -> Result<(), String> {
    if request.name.trim().is_empty() {
        return Err("Application name is required".into());
    }
    if request.entry.trim().is_empty() {
        return Err("Entry point is required".into());
    }
    let work_dir = request.work_dir.trim();
    if work_dir.is_empty() {
        return Err("Working directory is required".into());
    }
    if !std::path::Path::new(work_dir).is_dir() {
        return Err(format!("Working directory does not exist: {}", work_dir));
    }
    const KINDS: &[&str] = &["node", "python", "java", "go", "binary", "php"];
    if !KINDS.contains(&request.runtime.as_str()) {
        return Err(format!("Unsupported runtime: {}", request.runtime));
    }
    if request.port != 0 && !(1..=65535).contains(&request.port) {
        return Err("Port must be between 1 and 65535".into());
    }
    for line in request.env.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if !line.contains('=') {
            return Err(format!("Invalid environment line: {}", line));
        }
    }
    Ok(())
}

pub async fn list(pool: &sqlx::SqlitePool) -> Result<Vec<AppRecord>, String> {
    sqlx::query_as::<_, AppRecord>("SELECT * FROM apps ORDER BY id DESC")
        .fetch_all(pool)
        .await
        .map_err(|err| err.to_string())
}

pub async fn get(pool: &sqlx::SqlitePool, id: i64) -> Result<AppRecord, String> {
    sqlx::query_as::<_, AppRecord>("SELECT * FROM apps WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "Application not found".to_string())
}

pub async fn create(pool: &sqlx::SqlitePool, request: &SaveAppRequest) -> Result<i64, String> {
    validate(request)?;
    let result = sqlx::query(
        "INSERT INTO apps (name, runtime, work_dir, entry, args, env, port, autostart, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, datetime('now'), datetime('now'))",
    )
    .bind(request.name.trim())
    .bind(&request.runtime)
    .bind(request.work_dir.trim())
    .bind(request.entry.trim())
    .bind(request.args.trim())
    .bind(request.env.trim())
    .bind(request.port)
    .bind(request.autostart)
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;
    Ok(result.last_insert_rowid())
}

pub async fn update(
    pool: &sqlx::SqlitePool,
    id: i64,
    request: &SaveAppRequest,
) -> Result<(), String> {
    validate(request)?;
    sqlx::query(
        "UPDATE apps SET name = ?, runtime = ?, work_dir = ?, entry = ?, args = ?, env = ?,
         port = ?, autostart = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(request.name.trim())
    .bind(&request.runtime)
    .bind(request.work_dir.trim())
    .bind(request.entry.trim())
    .bind(request.args.trim())
    .bind(request.env.trim())
    .bind(request.port)
    .bind(request.autostart)
    .bind(id)
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;
    Ok(())
}

pub async fn delete(pool: &sqlx::SqlitePool, id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM apps WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|err| err.to_string())?;
    Ok(())
}

/// Builds the command line for an app. Returns (program, args).
fn build_command(record: &AppRecord) -> Result<(String, Vec<String>), String> {
    let entry = record.entry.trim().to_string();
    let extra: Vec<String> = shell_split(&record.args);

    let (program, mut args) = match record.runtime.as_str() {
        "node" => (
            runtime::interpreter_path("node").unwrap_or_else(|| "node".into()),
            vec![entry],
        ),
        "python" => (
            runtime::interpreter_path("python").unwrap_or_else(|| "python3".into()),
            vec![entry],
        ),
        "php" => (
            runtime::interpreter_path("php").unwrap_or_else(|| "php".into()),
            vec![entry],
        ),
        "java" => (
            runtime::interpreter_path("java").unwrap_or_else(|| "java".into()),
            vec!["-jar".to_string(), entry],
        ),
        // Go programs and plain binaries are executed directly.
        "go" | "binary" => (entry, Vec::new()),
        other => return Err(format!("Unsupported runtime: {}", other)),
    };
    args.extend(extra);
    Ok((program, args))
}

/// Minimal argument splitter honouring single and double quotes.
fn shell_split(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;

    for ch in input.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => current.push(ch),
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            None => current.push(ch),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn parse_env(raw: &str) -> Vec<(String, String)> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            if key.is_empty() {
                return None;
            }
            Some((key.to_string(), value.trim().to_string()))
        })
        .collect()
}

/// Truncates the log when it grows past the cap, so tailing stays cheap.
fn rotate_if_needed(path: &std::path::Path) {
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() > LOG_MAX_BYTES {
            let _ = std::fs::write(path, b"[LinxPanel] log truncated\n");
        }
    }
}

pub async fn start(
    supervisor: Arc<Supervisor>,
    pool: sqlx::SqlitePool,
    data_dir: std::path::PathBuf,
    id: i64,
) -> Result<(), String> {
    if supervisor.is_running(id).await {
        return Err("Application is already running".into());
    }
    let record = get(&pool, id).await?;
    let (program, args) = build_command(&record)?;

    let log_file = log_path(&data_dir, id);
    if let Some(parent) = log_file.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    rotate_if_needed(&log_file);

    let (stop_tx, stop_rx) = oneshot::channel();
    // Spawn once synchronously so the caller learns about bad commands immediately.
    let first = spawn_child(&record, &program, &args, &log_file)?;
    let pid = first.id().unwrap_or(0);

    {
        let mut guard = supervisor.running.lock().await;
        guard.insert(
            id,
            Running {
                pid,
                started_at: chrono::Utc::now().timestamp(),
                restarts: 0,
                last_error: String::new(),
                stop_tx: Some(stop_tx),
            },
        );
    }

    tokio::spawn(supervise(
        supervisor.clone(),
        record,
        program,
        args,
        log_file,
        first,
        stop_rx,
        id,
    ));
    Ok(())
}

fn spawn_child(
    record: &AppRecord,
    program: &str,
    args: &[String],
    log_file: &std::path::Path,
) -> Result<tokio::process::Child, String> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_file)
        .map_err(|err| format!("Cannot open log file: {}", err))?;
    let err_file = file
        .try_clone()
        .map_err(|err| format!("Cannot open log file: {}", err))?;

    let mut command = tokio::process::Command::new(program);
    command
        .args(args)
        .current_dir(&record.work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(file))
        .stderr(Stdio::from(err_file))
        .kill_on_drop(false);

    for (key, value) in parse_env(&record.env) {
        command.env(key, value);
    }
    if record.port > 0 {
        // Convention every supported runtime understands.
        command.env("PORT", record.port.to_string());
    }

    command
        .spawn()
        .map_err(|err| format!("Failed to start {}: {}", program, err))
}

#[allow(clippy::too_many_arguments)]
async fn supervise(
    supervisor: Arc<Supervisor>,
    record: AppRecord,
    program: String,
    args: Vec<String>,
    log_file: std::path::PathBuf,
    mut child: tokio::process::Child,
    mut stop_rx: oneshot::Receiver<()>,
    id: i64,
) {
    let mut delay = RESTART_DELAY;
    let mut restarts: u32 = 0;

    loop {
        let started = std::time::Instant::now();

        tokio::select! {
            status = child.wait() => {
                let detail = match status {
                    Ok(code) => format!("exited with {}", code),
                    Err(err) => format!("wait failed: {}", err),
                };
                append_log(&log_file, &format!("[LinxPanel] process {}\n", detail));

                if !record.autostart {
                    supervisor.running.lock().await.remove(&id);
                    return;
                }

                // A process that ran for a while is a transient failure, not a bad config.
                if started.elapsed() >= HEALTHY_AFTER {
                    delay = RESTART_DELAY;
                }

                append_log(
                    &log_file,
                    &format!("[LinxPanel] restarting in {}s\n", delay.as_secs()),
                );

                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = &mut stop_rx => {
                        supervisor.running.lock().await.remove(&id);
                        return;
                    }
                }
                delay = (delay * 2).min(RESTART_DELAY_MAX);
                restarts = restarts.saturating_add(1);

                match spawn_child(&record, &program, &args, &log_file) {
                    Ok(next) => {
                        let pid = next.id().unwrap_or(0);
                        child = next;
                        let mut guard = supervisor.running.lock().await;
                        if let Some(entry) = guard.get_mut(&id) {
                            entry.pid = pid;
                            entry.started_at = chrono::Utc::now().timestamp();
                            entry.restarts = restarts;
                            entry.last_error = detail;
                        } else {
                            // Stopped while we were sleeping.
                            return;
                        }
                    }
                    Err(err) => {
                        append_log(&log_file, &format!("[LinxPanel] {}\n", err));
                        supervisor.running.lock().await.remove(&id);
                        return;
                    }
                }
            }
            _ = &mut stop_rx => {
                let _ = child.kill().await;
                append_log(&log_file, "[LinxPanel] stopped by panel\n");
                supervisor.running.lock().await.remove(&id);
                return;
            }
        }
    }
}

fn append_log(path: &std::path::Path, text: &str) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(text.as_bytes());
    }
}

pub async fn stop(supervisor: Arc<Supervisor>, id: i64) -> Result<(), String> {
    let stop_tx = {
        let mut guard = supervisor.running.lock().await;
        match guard.get_mut(&id) {
            Some(entry) => entry.stop_tx.take(),
            None => return Err("Application is not running".into()),
        }
    };
    match stop_tx {
        Some(tx) => {
            let _ = tx.send(());
            // Give the supervisor a moment to reap the child before reporting state.
            tokio::time::sleep(Duration::from_millis(250)).await;
            Ok(())
        }
        None => Err("Application is already stopping".into()),
    }
}

pub async fn restart(
    supervisor: Arc<Supervisor>,
    pool: sqlx::SqlitePool,
    data_dir: std::path::PathBuf,
    id: i64,
) -> Result<(), String> {
    if supervisor.is_running(id).await {
        stop(supervisor.clone(), id).await?;
        // Wait for the port to be released before binding it again.
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    start(supervisor, pool, data_dir, id).await
}

pub fn read_log(data_dir: &std::path::Path, id: i64, lines: usize) -> String {
    let path = log_path(data_dir, id);
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let collected: Vec<&str> = content.lines().collect();
    let start = collected.len().saturating_sub(lines.clamp(10, 2000));
    collected[start..].join("\n")
}

pub fn clear_log(data_dir: &std::path::Path, id: i64) -> Result<(), String> {
    let path = log_path(data_dir, id);
    if path.exists() {
        std::fs::write(&path, b"").map_err(|err| err.to_string())?;
    }
    Ok(())
}

/// Starts every app flagged `autostart` — called once after the DB is ready.
pub async fn start_autostart_apps(
    supervisor: Arc<Supervisor>,
    pool: sqlx::SqlitePool,
    data_dir: std::path::PathBuf,
) {
    let records = match list(&pool).await {
        Ok(records) => records,
        Err(err) => {
            tracing::warn!("Failed to list apps for autostart: {}", err);
            return;
        }
    };
    for record in records.into_iter().filter(|record| record.autostart) {
        match start(supervisor.clone(), pool.clone(), data_dir.clone(), record.id).await {
            Ok(()) => tracing::info!("Autostarted application '{}'", record.name),
            Err(err) => tracing::warn!("Autostart failed for '{}': {}", record.name, err),
        }
    }
}
