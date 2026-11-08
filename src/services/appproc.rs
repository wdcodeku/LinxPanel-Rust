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

use super::{runtime, shell};

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
    /// node | python | java | go | binary | php | dotnet | rust | ruby | lua
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
    /// System user the app runs as; empty = the panel's own user.
    #[sqlx(default)]
    pub run_user: String,
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
    #[serde(default)]
    pub run_user: String,
    /// Give the working directory to `run_user` (recursively) on save.
    #[serde(default)]
    pub chown_work_dir: bool,
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

/// uid, gid and home of a system user.
pub fn run_user_ids(user: &str) -> Option<(u32, u32, String)> {
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd.lines().find_map(|line| {
        let cols: Vec<&str> = line.split(':').collect();
        (cols.len() >= 6 && cols[0] == user).then(|| {
            Some((
                cols[2].parse().ok()?,
                cols[3].parse().ok()?,
                cols[5].to_string(),
            ))
        })?
    })
}

fn normalized_user(user: &str) -> String {
    match user.trim() {
        "root" => String::new(),
        other => other.to_string(),
    }
}

fn hand_over_work_dir(request: &SaveAppRequest) {
    let user = normalized_user(&request.run_user);
    if request.chown_work_dir && !user.is_empty() {
        chown_to(&user, std::path::Path::new(request.work_dir.trim()));
    }
}

/// Gives `dir` (recursively) to `user` and its primary group.
pub fn chown_to(user: &str, dir: &std::path::Path) {
    if let Some((uid, gid, _)) = run_user_ids(user) {
        let mut stack = vec![dir.to_path_buf()];
        while let Some(path) = stack.pop() {
            let _ = std::os::unix::fs::lchown(&path, Some(uid), Some(gid));
            if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
                if let Ok(entries) = std::fs::read_dir(&path) {
                    stack.extend(entries.flatten().map(|e| e.path()));
                }
            }
        }
    }
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
    const KINDS: &[&str] = &[
        "node", "python", "java", "go", "binary", "php", "dotnet", "rust", "ruby", "lua",
    ];
    if !KINDS.contains(&request.runtime.as_str()) {
        return Err(format!("Unsupported runtime: {}", request.runtime));
    }
    if request.port != 0 && !(1..=65535).contains(&request.port) {
        return Err("Port must be between 1 and 65535".into());
    }
    let user = request.run_user.trim();
    if !user.is_empty() && user != "root" && run_user_ids(user).is_none() {
        return Err(format!("No such system user: {}", user));
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
        "INSERT INTO apps (name, runtime, work_dir, entry, args, env, port, autostart, run_user, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'), datetime('now'))",
    )
    .bind(request.name.trim())
    .bind(&request.runtime)
    .bind(request.work_dir.trim())
    .bind(request.entry.trim())
    .bind(request.args.trim())
    .bind(request.env.trim())
    .bind(request.port)
    .bind(request.autostart)
    .bind(normalized_user(&request.run_user))
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;
    hand_over_work_dir(request);
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
         port = ?, autostart = ?, run_user = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(request.name.trim())
    .bind(&request.runtime)
    .bind(request.work_dir.trim())
    .bind(request.entry.trim())
    .bind(request.args.trim())
    .bind(request.env.trim())
    .bind(request.port)
    .bind(request.autostart)
    .bind(normalized_user(&request.run_user))
    .bind(id)
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;
    hand_over_work_dir(request);
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

/// Project-local tool directories, searched before the system PATH: a Python
/// virtualenv and Node's node_modules/.bin.
fn project_bin_dirs(work_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    [".venv/bin", "venv/bin", "node_modules/.bin"]
        .iter()
        .map(|dir| work_dir.join(dir))
        .filter(|dir| dir.is_dir())
        .collect()
}

fn find_project_bin(work_dir: &std::path::Path, name: &str) -> Option<String> {
    if name.contains('/') {
        return None;
    }
    project_bin_dirs(work_dir)
        .into_iter()
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().to_string())
}

/// The project's virtualenv interpreter, else the system one.
fn python_for(work_dir: &std::path::Path) -> String {
    for venv in [".venv", "venv"] {
        let python = work_dir.join(venv).join("bin/python");
        if python.is_file() {
            return python.to_string_lossy().to_string();
        }
    }
    runtime::interpreter_path("python").unwrap_or_else(|| "python3".into())
}

/// Where a Go app given as source is compiled to (inside its directory),
/// one file per app so apps sharing a directory never overwrite each other.
fn go_build_output(record: &AppRecord) -> String {
    format!(".linxpanel/app-{}", record.id)
}

/// A Go entry naming source rather than a built program: ".", a .go file,
/// or a directory of the project (./cmd/server).
fn go_source_entry(work_dir: &std::path::Path, entry: &str) -> bool {
    let entry = entry.trim();
    if entry == "." || entry.ends_with(".go") {
        return true;
    }
    let path = work_dir.join(entry);
    path.is_dir()
        && std::fs::read_dir(&path)
            .map(|mut items| {
                items.any(|e| {
                    e.map(|e| e.file_name().to_string_lossy().ends_with(".go"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false)
}

/// Compiles a Go app given as source (`go build -o .linxpanel/app <entry>`),
/// writing the compiler output to the app's log. Blocking.
fn build_go_source(record: &AppRecord, log_file: &std::path::Path) -> Result<(), String> {
    let work_dir = std::path::Path::new(&record.work_dir);
    let go = runtime::interpreter_path("go")
        .or_else(|| shell::which("go").map(|p| p.to_string_lossy().to_string()))
        .ok_or("Go is not installed (Runtimes > Go)")?;
    let output_name = go_build_output(record);
    let out = work_dir.join(&output_name);
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let entry = record.entry.trim();
    append_log(
        log_file,
        &format!("[LinxPanel] go build -o {} {}\n", output_name, entry),
    );
    let mut command = std::process::Command::new(&go);
    command
        .args(["build", "-o", output_name.as_str(), entry])
        .current_dir(work_dir);
    // A service started without HOME has no Go build cache otherwise.
    if std::env::var_os("HOME").is_none() {
        command.env("HOME", "/root");
    }
    for (key, value) in parse_env(&record.env) {
        command.env(key, value);
    }
    let output = shell::run_command(command, None, std::time::Duration::from_secs(900))?;
    let text = format!("{}{}", output.stdout, output.stderr);
    if !text.trim().is_empty() {
        append_log(log_file, &text);
    }
    if !output.ok() {
        let detail: Vec<&str> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .take(8)
            .collect();
        return Err(format!("go build failed:\n{}", detail.join("\n")));
    }
    let run_user = normalized_user(&record.run_user);
    if !run_user.is_empty() {
        chown_to(&run_user, &work_dir.join(".linxpanel"));
    }
    append_log(log_file, "[LinxPanel] Build finished\n");
    Ok(())
}

/// Builds the command line for an app. Returns (program, args).
///
/// Besides a script/jar/binary path, the entry may be a command line:
/// `-m uvicorn main:app`, `gunicorn app:app` (from the virtualenv),
/// `npm start`, `pnpm start` or `next start` (from node_modules/.bin).
fn build_command(record: &AppRecord) -> Result<(String, Vec<String>), String> {
    let entry = record.entry.trim().to_string();
    let extra: Vec<String> = shell_split(&record.args);
    let work_dir = std::path::Path::new(&record.work_dir);
    // An entry naming an existing file is used whole, even with spaces in it.
    let tokens = if work_dir.join(&entry).is_file() || std::path::Path::new(&entry).is_file() {
        vec![entry.clone()]
    } else {
        shell_split(&entry)
    };
    let (first, rest) = tokens
        .split_first()
        .map(|(first, rest)| (first.clone(), rest.to_vec()))
        .ok_or("Entry point is required")?;

    let (program, mut args) = match record.runtime.as_str() {
        "python" => {
            if first.starts_with('-') || first.ends_with(".py") {
                (python_for(work_dir), tokens.clone())
            } else if let Some(bin) = find_project_bin(work_dir, &first) {
                (bin, rest)
            } else {
                (python_for(work_dir), tokens.clone())
            }
        }
        "node" => {
            if matches!(first.as_str(), "npm" | "pnpm" | "yarn" | "npx") {
                (first, rest)
            } else if let Some(bin) = find_project_bin(work_dir, &first) {
                (bin, rest)
            } else {
                (
                    runtime::interpreter_path("node").unwrap_or_else(|| "node".into()),
                    tokens.clone(),
                )
            }
        }
        "php" => (
            runtime::interpreter_path("php").unwrap_or_else(|| "php".into()),
            tokens.clone(),
        ),
        // A .java file runs straight from source (Java 11+); anything else is a jar.
        "java" if first.ends_with(".java") => (
            runtime::interpreter_path("java").unwrap_or_else(|| "java".into()),
            tokens.clone(),
        ),
        "java" => (
            runtime::interpreter_path("java").unwrap_or_else(|| "java".into()),
            vec!["-jar".to_string(), entry],
        ),
        // Go and Rust programs and plain binaries are executed directly
        // (`cargo run --release` works too, for development).
        "rust" if first == "cargo" => (
            runtime::interpreter_path("rust")
                .map(|rustc| rustc.replace("rustc", "cargo"))
                .filter(|cargo| std::path::Path::new(cargo).exists())
                .unwrap_or_else(|| "cargo".into()),
            rest,
        ),
        // Go source (".", main.go, ./cmd/server) was compiled by `start`.
        "go" if go_source_entry(work_dir, &entry) => (
            work_dir
                .join(go_build_output(record))
                .to_string_lossy()
                .to_string(),
            Vec::new(),
        ),
        // A bare name like "app" means the file in the project, not one on PATH.
        "go" | "binary" | "rust" => {
            let local = work_dir.join(&entry);
            if !entry.contains('/') && local.is_file() {
                (local.to_string_lossy().to_string(), Vec::new())
            } else {
                (entry, Vec::new())
            }
        }
        // App.dll runs on the dotnet host; a published apphost runs directly;
        // a project file runs with `dotnet run` (development).
        "dotnet" => {
            let dotnet = runtime::interpreter_path("dotnet").unwrap_or_else(|| "dotnet".into());
            if first.ends_with(".dll") {
                (dotnet, tokens.clone())
            } else if first.ends_with(".csproj") || first.ends_with(".fsproj") {
                (
                    dotnet,
                    vec![
                        "run".into(),
                        "--project".into(),
                        first,
                        "-c".into(),
                        "Release".into(),
                    ],
                )
            } else if first == "dotnet" {
                (dotnet, rest)
            } else {
                (first, rest)
            }
        }
        // app.rb, or a command (bundle exec puma, rails server, rackup ...)
        // from the project's bin/ stubs or the system.
        "ruby" => {
            let ruby = runtime::interpreter_path("ruby").unwrap_or_else(|| "ruby".into());
            if first.ends_with(".rb") || first.starts_with('-') {
                (ruby, tokens.clone())
            } else if work_dir.join("bin").join(&first).is_file() {
                (
                    work_dir
                        .join("bin")
                        .join(&first)
                        .to_string_lossy()
                        .to_string(),
                    rest,
                )
            } else if first == "ruby" {
                (ruby, rest)
            } else {
                (
                    shell::which(&first)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or(first),
                    rest,
                )
            }
        }
        "lua" => {
            let lua = runtime::interpreter_path("lua").unwrap_or_else(|| "lua".into());
            if matches!(
                first.as_str(),
                "lua" | "luajit" | "lua5.1" | "lua5.3" | "lua5.4"
            ) {
                (
                    shell::which(&first)
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or(first),
                    rest,
                )
            } else {
                (lua, tokens.clone())
            }
        }
        other => return Err(format!("Unsupported runtime: {}", other)),
    };
    args.extend(extra);
    Ok((program, args))
}

/// PATH with the project's tool directories first.
fn project_path(work_dir: &std::path::Path) -> Option<String> {
    let dirs = project_bin_dirs(work_dir);
    if dirs.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = dirs
        .iter()
        .map(|dir| dir.to_string_lossy().to_string())
        .collect();
    parts.push(std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into()));
    Some(parts.join(":"))
}

/// Installs the project's dependencies in its working directory, with the app's
/// own environment (so PIP_INDEX_URL / npm_config_registry mirrors apply).
/// Returns the command log.
pub fn install_dependencies(record: &AppRecord, build: bool) -> Result<String, String> {
    let work_dir = std::path::Path::new(&record.work_dir);
    if !work_dir.is_dir() {
        return Err(format!(
            "Working directory does not exist: {}",
            record.work_dir
        ));
    }
    let has = |file: &str| work_dir.join(file).exists();
    let mut log = String::new();
    let mut step = |program: &str, args: &[&str]| -> Result<(), String> {
        log.push_str(&format!("$ {} {}\n", program, args.join(" ")));
        let mut command = std::process::Command::new(program);
        command.args(args).current_dir(work_dir);
        for (key, value) in parse_env(&record.env) {
            command.env(key, value);
        }
        if let Some(path) = project_path(work_dir) {
            command.env("PATH", path);
        }
        // Composer refuses to run as root without this.
        command.env("COMPOSER_ALLOW_SUPERUSER", "1");
        command
            .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
            .env("DOTNET_NOLOGO", "1");
        let output = shell::run_command(command, None, std::time::Duration::from_secs(1800))?;
        for text in [&output.stdout, &output.stderr] {
            if !text.trim().is_empty() {
                log.push_str(text.trim_end());
                log.push('\n');
            }
        }
        if output.ok() {
            Ok(())
        } else {
            Err(format!("{} exited with status {}", program, output.status))
        }
    };

    let result = match record.runtime.as_str() {
        "python" => {
            let venv = if work_dir.join("venv/bin/python").is_file() {
                "venv"
            } else {
                ".venv"
            };
            if !work_dir.join(venv).join("bin/python").is_file() {
                let python =
                    runtime::interpreter_path("python").unwrap_or_else(|| "python3".into());
                step(&python, &["-m", "venv", venv])?;
            }
            let pip = format!("{}/bin/pip", venv);
            step(&pip, &["install", "--upgrade", "pip"]).ok();
            if has("requirements.txt") {
                step(&pip, &["install", "-r", "requirements.txt"])
            } else if has("pyproject.toml") || has("setup.py") {
                step(&pip, &["install", "."])
            } else {
                Err("No requirements.txt or pyproject.toml found".into())
            }
        }
        "node" => {
            let manager = if has("pnpm-lock.yaml") && shell::has_binary("pnpm") {
                "pnpm"
            } else if has("yarn.lock") && shell::has_binary("yarn") {
                "yarn"
            } else {
                "npm"
            };
            if !has("package.json") {
                return Err("No package.json found".into());
            }
            let install: &[&str] = match manager {
                "pnpm" => &["install", "--frozen-lockfile"],
                "yarn" => &["install", "--frozen-lockfile"],
                _ if has("package-lock.json") => &["ci"],
                _ => &["install"],
            };
            step(manager, install)?;
            let has_build_script = std::fs::read_to_string(work_dir.join("package.json"))
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .is_some_and(|json| json["scripts"]["build"].is_string());
            if build && has_build_script {
                step(manager, &["run", "build"])
            } else {
                Ok(())
            }
        }
        "php" => {
            if !has("composer.json") {
                return Err("No composer.json found".into());
            }
            if !shell::has_binary("composer") {
                return Err("composer is not installed".into());
            }
            step(
                "composer",
                &[
                    "install",
                    "--no-dev",
                    "--optimize-autoloader",
                    "--no-interaction",
                ],
            )
        }
        // Restore and publish into ./publish (entry: publish/App.dll).
        "dotnet" => {
            let project = find_with_extension(work_dir, &["sln", "csproj", "fsproj"])
                .ok_or("No .sln / .csproj found")?;
            let dotnet =
                runtime::interpreter_path("dotnet").ok_or("The .NET SDK is not installed")?;
            step(&dotnet, &["restore", &project])?;
            step(
                &dotnet,
                &[
                    "publish", &project, "-c", "Release", "-o", "publish", "--nologo",
                ],
            )
        }
        // A release build (entry: target/release/<name>).
        "rust" => {
            if !has("Cargo.toml") {
                return Err("No Cargo.toml found".into());
            }
            let cargo = shell::which("cargo")
                .map(|p| p.to_string_lossy().to_string())
                .ok_or("cargo is not installed")?;
            step(&cargo, &["build", "--release"])
        }
        // Gems into ./vendor/bundle, so the app does not depend on system gems.
        "ruby" => {
            if !has("Gemfile") {
                return Err("No Gemfile found".into());
            }
            let bundle = shell::which("bundle")
                .map(|p| p.to_string_lossy().to_string())
                .ok_or("bundler is not installed")?;
            step(
                &bundle,
                &["config", "set", "--local", "path", "vendor/bundle"],
            )?;
            step(&bundle, &["install", "--jobs", "4"])
        }
        // Rocks into ./lua_modules for the Lua version that runs the app.
        "lua" => {
            let rockspec =
                find_with_extension(work_dir, &["rockspec"]).ok_or("No .rockspec found")?;
            let luarocks = shell::which("luarocks")
                .map(|p| p.to_string_lossy().to_string())
                .ok_or("luarocks is not installed")?;
            let lua_version = shell::cached_version_output("lua", &["-v"]).and_then(|text| {
                let v = text.split_whitespace().nth(1)?.to_string();
                let mut parts = v.split('.');
                Some(format!("{}.{}", parts.next()?, parts.next()?))
            });
            let mut args = vec![];
            if let Some(version) = &lua_version {
                args.push("--lua-version");
                args.push(version.as_str());
            }
            args.extend([
                "--tree",
                "lua_modules",
                "install",
                "--only-deps",
                rockspec.as_str(),
            ]);
            step(&luarocks, &args)
        }
        // Builds ./app from the module in the working directory.
        "go" => {
            if !has("go.mod") {
                return Err("No go.mod in the working directory".into());
            }
            let go = shell::which("go")
                .map(|p| p.to_string_lossy().to_string())
                .ok_or("Go is not installed")?;
            step(&go, &["build", "-o", "app", "."])
        }
        _ => Err("This runtime has no dependency step".into()),
    };
    // Built files belong to the user the app runs as.
    let run_user = normalized_user(&record.run_user);
    if result.is_ok() && !run_user.is_empty() {
        chown_to(&run_user, work_dir);
    }
    match result {
        Ok(()) => Ok(log),
        Err(err) => Err(format!("{}\n{}", log, err)),
    }
}

/// The first file in `dir` with one of `extensions` (sorted, for stability).
fn find_with_extension(dir: &std::path::Path, extensions: &[&str]) -> Option<String> {
    for ext in extensions {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(&format!(".{}", ext)))
            .collect();
        names.sort();
        if let Some(first) = names.into_iter().next() {
            return Some(first);
        }
    }
    None
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
    let log_file = log_path(&data_dir, id);
    if let Some(parent) = log_file.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    rotate_if_needed(&log_file);

    // Go given as source is compiled on every (re)start from the panel; a
    // crash restart by the supervisor reuses the last build.
    if record.runtime == "go"
        && go_source_entry(std::path::Path::new(&record.work_dir), &record.entry)
    {
        let build_record = record.clone();
        let build_log = log_file.clone();
        tokio::task::spawn_blocking(move || build_go_source(&build_record, &build_log))
            .await
            .map_err(|e| e.to_string())??;
    }
    let (program, args) = build_command(&record)?;

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

/// LUA_PATH / LUA_CPATH for a project-local rocks tree.
fn lua_module_paths(work_dir: &std::path::Path) -> Option<(String, String)> {
    let tree = work_dir.join("lua_modules");
    let version = std::fs::read_dir(tree.join("share/lua"))
        .ok()?
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .next()?;
    let t = tree.to_string_lossy();
    Some((
        format!("{t}/share/lua/{version}/?.lua;{t}/share/lua/{version}/?/init.lua;;"),
        format!("{t}/lib/lua/{version}/?.so;;"),
    ))
}

#[cfg(unix)]
extern "C" {
    fn setgid(gid: u32) -> i32;
    fn setuid(uid: u32) -> i32;
    fn setgroups(size: usize, list: *const u32) -> i32;
}

/// Runs the child as `user` (dropping root's groups first); HOME and USER
/// follow. Nothing changes for an empty user or root.
fn apply_run_user(command: &mut tokio::process::Command, user: &str) -> Result<(), String> {
    let user = normalized_user(user);
    if user.is_empty() {
        return Ok(());
    }
    let (uid, gid, home) =
        run_user_ids(&user).ok_or_else(|| format!("No such system user: {}", user))?;
    command
        .env("HOME", if home.is_empty() { "/".into() } else { home })
        .env("USER", &user)
        .env("LOGNAME", &user);
    #[cfg(unix)]
    // SAFETY: only async-signal-safe system calls between fork and exec.
    unsafe {
        command.pre_exec(move || {
            if setgroups(1, &gid) != 0 || setgid(gid) != 0 || setuid(uid) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
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
    // SAFETY: the hook only makes a system call (see shell::normal_priority).
    unsafe {
        command.pre_exec(shell::normal_priority);
    }

    let work_dir = std::path::Path::new(&record.work_dir);
    if let Some(path) = project_path(work_dir) {
        command.env("PATH", path);
    }
    for venv in [".venv", "venv"] {
        if work_dir.join(venv).join("bin/python").is_file() {
            command.env("VIRTUAL_ENV", work_dir.join(venv));
            break;
        }
    }
    // The app's own variables win, including PATH.
    for (key, value) in parse_env(&record.env) {
        command.env(key, value);
    }
    if record.port > 0 {
        // Convention every supported runtime understands.
        command.env("PORT", record.port.to_string());
    }
    let user_env: Vec<String> = parse_env(&record.env).into_iter().map(|(k, _)| k).collect();
    let unset = |key: &str| !user_env.iter().any(|k| k == key);
    match record.runtime.as_str() {
        "dotnet" => {
            command
                .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
                .env("DOTNET_NOLOGO", "1");
            // ASP.NET Core listens on its own setting, not PORT.
            if record.port > 0 && unset("ASPNETCORE_URLS") && unset("ASPNETCORE_HTTP_PORTS") {
                command.env("ASPNETCORE_URLS", format!("http://0.0.0.0:{}", record.port));
            }
            if std::path::Path::new("/usr/share/dotnet/dotnet").exists() && unset("DOTNET_ROOT") {
                command.env("DOTNET_ROOT", "/usr/share/dotnet");
            }
        }
        "lua" => {
            // Rocks installed into the project (luarocks --tree lua_modules).
            if let Some((path, cpath)) = lua_module_paths(work_dir) {
                if unset("LUA_PATH") {
                    command.env("LUA_PATH", path);
                }
                if unset("LUA_CPATH") {
                    command.env("LUA_CPATH", cpath);
                }
            }
        }
        "ruby" => {
            if unset("RACK_ENV") && unset("RAILS_ENV") {
                command.env("RACK_ENV", "production");
            }
        }
        _ => {}
    }
    apply_run_user(&mut command, &record.run_user)?;

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
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
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
        match start(
            supervisor.clone(),
            pool.clone(),
            data_dir.clone(),
            record.id,
        )
        .await
        {
            Ok(()) => tracing::info!("Autostarted application '{}'", record.name),
            Err(err) => tracing::warn!("Autostart failed for '{}': {}", record.name, err),
        }
    }
}
