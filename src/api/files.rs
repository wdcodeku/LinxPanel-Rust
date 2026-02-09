use axum::{
    body::{Body, Bytes},
    extract::{
        DefaultBodyLimit, Multipart,
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query,
    },
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures::StreamExt;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use crate::models::ApiResponse;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/files/roots", get(file_roots))
        .route("/files/list", get(list_files))
        .route("/files/read", get(read_file))
        .route("/files/download", get(download_file))
        .route("/files/dir-size", get(directory_size))
        .route("/files/write", post(write_file))
        .route("/files/mkdir", post(mkdir))
        .route("/files/delete", post(delete_path))
        .route("/files/rename", post(rename_path))
        .route("/files/info", get(file_info))
        .route(
            "/files/upload",
            post(upload_files).layer(DefaultBodyLimit::max(1024 * 1024 * 1024)),
        )
        .route("/files/exec", post(exec_cmd))
        .route("/files/exec/stream", post(exec_cmd_stream))
        .route("/files/terminal/ws", get(terminal_ws))
        .route("/files/chmod", post(chmod_path))
}

#[derive(Debug, Deserialize)]
struct PathQuery { path: String }

#[derive(Debug, Serialize)]
struct FileEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
    modified: String,
    permissions: String,
}

#[derive(Debug, Serialize)]
struct FileContent {
    path: String,
    content: String,
    size: u64,
}

#[derive(Debug, Serialize)]
struct DirSizePayload {
    path: String,
    size: u64,
}

#[derive(Debug, Serialize)]
struct UploadResult {
    uploaded: usize,
    files: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct WriteRequest { path: String, content: String }

#[derive(Debug, Deserialize)]
struct RenameRequest { from: String, to: String }

#[derive(Debug, Serialize)]
struct FileRootEntry {
    path: String,
    label: String,
}

#[derive(Debug, Serialize)]
struct FileRootsPayload {
    is_windows: bool,
    roots: Vec<FileRootEntry>,
}

#[cfg(windows)]
fn detect_file_roots() -> Vec<FileRootEntry> {
    let mut roots = Vec::new();
    for letter in b'A'..=b'Z' {
        let drive = format!("{}:/", letter as char);
        if std::path::Path::new(&drive).exists() {
            roots.push(FileRootEntry {
                path: drive.clone(),
                label: format!("{}:", letter as char),
            });
        }
    }

    if roots.is_empty() {
        if let Ok(current_dir) = std::env::current_dir() {
            let current = current_dir.to_string_lossy().replace('\\', "/");
            if current.len() >= 2 && current.as_bytes().get(1) == Some(&b':') {
                let drive_root = format!("{}:/", current.chars().next().unwrap_or('C'));
                roots.push(FileRootEntry {
                    path: drive_root.clone(),
                    label: drive_root.trim_end_matches('/').to_string(),
                });
            }
        }
    }

    if roots.is_empty() {
        roots.push(FileRootEntry {
            path: "C:/".to_string(),
            label: "C:".to_string(),
        });
    }

    roots
}

#[cfg(not(windows))]
fn detect_file_roots() -> Vec<FileRootEntry> {
    vec![FileRootEntry {
        path: "/".to_string(),
        label: "/".to_string(),
    }]
}

async fn file_roots() -> Json<ApiResponse<FileRootsPayload>> {
    Json(ApiResponse::ok(FileRootsPayload {
        is_windows: cfg!(windows),
        roots: detect_file_roots(),
    }))
}

fn normalize_fs_path(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    #[cfg(windows)]
    {
        let mut normalized = normalized;
        if normalized.len() >= 3
            && normalized.starts_with('/')
            && normalized.as_bytes().get(2) == Some(&b':')
        {
            normalized = normalized[1..].to_string();
        }
        if normalized.len() == 2 && normalized.as_bytes().get(1) == Some(&b':') {
            normalized.push('/');
        }
        normalized
    }
    #[cfg(not(windows))]
    {
        normalized
    }
}

fn sanitize_upload_filename(raw: &str) -> Option<String> {
    let normalized = raw.replace('\\', "/");
    let base = normalized
        .rsplit('/')
        .next()
        .map(|name| name.trim())
        .unwrap_or_default();
    if base.is_empty() || base == "." || base == ".." || base.contains('\0') {
        return None;
    }
    Some(base.to_string())
}

fn format_permissions(metadata: &std::fs::Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode() & 0o777;
        return format!("{:03o}", mode);
    }
    #[cfg(not(unix))]
    {
        if metadata.permissions().readonly() {
            return "444".to_string();
        }
        if metadata.is_dir() {
            return "755".to_string();
        }
        "644".to_string()
    }
}

fn calculate_directory_size(path: &Path) -> u64 {
    let mut total = 0_u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if let Ok(metadata) = entry.metadata() {
                if metadata.is_file() {
                    total = total.saturating_add(metadata.len());
                } else if metadata.is_dir() {
                    total = total.saturating_add(calculate_directory_size(&entry_path));
                }
            }
        }
    }
    total
}

async fn list_files(Query(q): Query<PathQuery>) -> Json<ApiResponse<Vec<FileEntry>>> {
    let normalized_path = normalize_fs_path(&q.path);
    let dir = std::path::Path::new(&normalized_path);
    if !dir.exists() {
        return Json(ApiResponse::err("Path does not exist"));
    }
    let mut entries = Vec::new();
    match std::fs::read_dir(dir) {
        Ok(rd) => {
            for entry in rd.flatten() {
                let meta = entry.metadata().ok();
                let name = entry.file_name().to_string_lossy().to_string();
                let path = entry.path().to_string_lossy().to_string().replace('\\', "/");
                let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
                let modified = meta.as_ref()
                    .and_then(|m| m.modified().ok())
                    .map(|t| {
                        let dt: chrono::DateTime<chrono::Local> = t.into();
                        dt.format("%Y-%m-%d %H:%M:%S").to_string()
                    })
                    .unwrap_or_default();
                let permissions = meta
                    .as_ref()
                    .map(format_permissions)
                    .unwrap_or_else(|| if is_dir { "755".into() } else { "644".into() });
                entries.push(FileEntry { name, path, is_dir, size, modified, permissions });
            }
            entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
            Json(ApiResponse::ok(entries))
        }
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}
async fn read_file(Query(q): Query<PathQuery>) -> Json<ApiResponse<FileContent>> {
    let normalized_path = normalize_fs_path(&q.path);
    let p = std::path::Path::new(&normalized_path);
    if !p.exists() || !p.is_file() {
        return Json(ApiResponse::err("File not found"));
    }
    let meta = std::fs::metadata(p).ok();
    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    if size > 10 * 1024 * 1024 {
        return Json(ApiResponse::err("File too large (>10MB)"));
    }
    match std::fs::read_to_string(p) {
        Ok(content) => Json(ApiResponse::ok(FileContent {
            path: normalized_path,
            content,
            size,
        })),
        Err(e) => Json(ApiResponse::err(&format!("Cannot read: {}", e))),
    }
}

async fn download_file(Query(q): Query<PathQuery>) -> Response<Body> {
    let normalized_path = normalize_fs_path(&q.path);
    let file_path = std::path::Path::new(&normalized_path);
    if !file_path.exists() {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Body::from("File not found"))
            .unwrap_or_else(|_| Response::new(Body::empty()));
    }
    if !file_path.is_file() {
        return Response::builder()
            .status(StatusCode::BAD_REQUEST)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Body::from("Download supports file only"))
            .unwrap_or_else(|_| Response::new(Body::empty()));
    }

    let bytes = match std::fs::read(file_path) {
        Ok(data) => data,
        Err(error) => {
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
                .body(Body::from(format!("Failed to read file: {}", error)))
                .unwrap_or_else(|_| Response::new(Body::empty()));
        }
    };

    let mime = mime_guess::from_path(file_path).first_or_octet_stream();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CONTENT_DISPOSITION, "attachment")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(bytes))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

async fn directory_size(Query(q): Query<PathQuery>) -> Json<ApiResponse<DirSizePayload>> {
    let normalized_path = normalize_fs_path(&q.path);
    let path = Path::new(&normalized_path);
    if !path.exists() {
        return Json(ApiResponse::err("Path not found"));
    }
    if !path.is_dir() {
        return Json(ApiResponse::err("Path is not a directory"));
    }

    let size = calculate_directory_size(path);
    Json(ApiResponse::ok(DirSizePayload {
        path: normalized_path,
        size,
    }))
}

async fn write_file(Json(req): Json<WriteRequest>) -> Json<ApiResponse<()>> {
    let normalized_path = normalize_fs_path(&req.path);
    if let Some(parent) = std::path::Path::new(&normalized_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(&normalized_path, &req.content) {
        Ok(_) => Json(ApiResponse::ok_msg("File saved")),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn mkdir(Json(req): Json<PathQuery>) -> Json<ApiResponse<()>> {
    let normalized_path = normalize_fs_path(&req.path);
    match std::fs::create_dir_all(&normalized_path) {
        Ok(_) => Json(ApiResponse::ok_msg("Directory created")),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn delete_path(Json(req): Json<PathQuery>) -> Json<ApiResponse<()>> {
    let normalized_path = normalize_fs_path(&req.path);
    let p = std::path::Path::new(&normalized_path);
    if !p.exists() {
        return Json(ApiResponse::err("Path not found"));
    }
    let result = if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) };
    match result {
        Ok(_) => Json(ApiResponse::ok_msg("Deleted")),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn rename_path(Json(req): Json<RenameRequest>) -> Json<ApiResponse<()>> {
    let from = normalize_fs_path(&req.from);
    let to = normalize_fs_path(&req.to);
    match std::fs::rename(&from, &to) {
        Ok(_) => Json(ApiResponse::ok_msg("Renamed")),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn file_info(Query(q): Query<PathQuery>) -> Json<ApiResponse<FileEntry>> {
    let normalized_path = normalize_fs_path(&q.path);
    let p = std::path::Path::new(&normalized_path);
    if !p.exists() {
        return Json(ApiResponse::err("Path not found"));
    }
    let meta = std::fs::metadata(p).ok();
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
    let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
    let modified = meta.as_ref()
        .and_then(|m| m.modified().ok())
        .map(|t| { let dt: chrono::DateTime<chrono::Local> = t.into(); dt.format("%Y-%m-%d %H:%M:%S").to_string() })
        .unwrap_or_default();
    Json(ApiResponse::ok(FileEntry {
        name,
        path: normalized_path,
        is_dir,
        size,
        modified,
        permissions: meta
            .as_ref()
            .map(format_permissions)
            .unwrap_or_else(|| if is_dir { "755".into() } else { "644".into() }),
    }))
}


#[derive(Debug, Deserialize)]
struct ChmodRequest { path: String, mode: String }

async fn chmod_path(Json(req): Json<ChmodRequest>) -> Json<ApiResponse<()>> {
    let normalized_path = normalize_fs_path(&req.path);
    let p = std::path::Path::new(&normalized_path);
    if !p.exists() {
        return Json(ApiResponse::err("Path not found"));
    }
    #[cfg(unix)]
    {
        let cmd_str = format!("chmod {} '{}'", req.mode, normalized_path);
        match std::process::Command::new("sh").arg("-c").arg(&cmd_str).output() {
            Ok(output) => {
                if output.status.success() {
                    Json(ApiResponse::ok_msg("Permission changed"))
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    Json(ApiResponse::err(&format!("Failed: {}", stderr)))
                }
            }
            Err(e) => Json(ApiResponse::err(&e.to_string())),
        }
    }
    #[cfg(windows)]
    {
        // Windows doesn't use chmod, just return ok
        let _ = &req.mode;
        Json(ApiResponse::ok_msg("Permission change not supported on Windows"))
    }
}

#[derive(Debug, Deserialize)]
struct TerminalWsQuery {
    cwd: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TerminalWsControl {
    #[serde(rename = "type")]
    message_type: String,
    cols: Option<u16>,
    rows: Option<u16>,
    path: Option<String>,
}

enum TerminalInputEvent {
    Input(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    Chdir(String),
    Close,
}

enum TerminalServerEvent {
    Output(Vec<u8>),
    Cwd(String),
}

#[derive(Debug, Serialize)]
struct TerminalServerControl {
    #[serde(rename = "type")]
    control_type: String,
    path: String,
}

fn default_terminal_cwd() -> String {
    if cfg!(windows) {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("C:/"))
            .to_string_lossy()
            .to_string()
    } else {
        "/".to_string()
    }
}

fn normalize_terminal_cwd(input: Option<String>) -> String {
    let requested = input.unwrap_or_else(default_terminal_cwd);
    let path = std::path::Path::new(&requested);
    if path.is_dir() {
        requested
    } else {
        default_terminal_cwd()
    }
}

fn build_terminal_shell(cwd: &str) -> CommandBuilder {
    #[cfg(windows)]
    {
        let mut command = CommandBuilder::new("powershell.exe");
        command.arg("-NoLogo");
        command.arg("-NoProfile");
        command.cwd(cwd);
        command
    }
    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "/bin/bash".to_string());
        let mut command = CommandBuilder::new(shell.as_str());
        command.cwd(cwd);
        command
    }
}

fn parse_terminal_control(text: &str) -> Option<TerminalInputEvent> {
    let control = serde_json::from_str::<TerminalWsControl>(text).ok()?;
    match control.message_type.as_str() {
        "resize" => {
            let cols = control.cols.unwrap_or(120).max(1);
            let rows = control.rows.unwrap_or(30).max(1);
            Some(TerminalInputEvent::Resize { cols, rows })
        }
        "chdir" => {
            let path = control.path.unwrap_or_default();
            if path.trim().is_empty() {
                return None;
            }
            Some(TerminalInputEvent::Chdir(path))
        }
        _ => None,
    }
}

fn detect_home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Ok(value) = std::env::var("USERPROFILE") {
            if !value.trim().is_empty() {
                return Some(PathBuf::from(value));
            }
        }
        let drive = std::env::var("HOMEDRIVE").unwrap_or_default();
        let home_path = std::env::var("HOMEPATH").unwrap_or_default();
        if !drive.trim().is_empty() && !home_path.trim().is_empty() {
            return Some(PathBuf::from(format!("{}{}", drive, home_path)));
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(value) = std::env::var("HOME") {
            if !value.trim().is_empty() {
                return Some(PathBuf::from(value));
            }
        }
    }
    None
}

fn normalize_path_components(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::Normal(segment) => normalized.push(segment),
        }
    }
    if normalized.as_os_str().is_empty() {
        if cfg!(windows) {
            PathBuf::from(default_terminal_cwd())
        } else {
            PathBuf::from("/")
        }
    } else {
        normalized
    }
}

fn resolve_terminal_cd_path(current_dir: &Path, target: &str) -> Option<PathBuf> {
    let raw = target.trim();
    let mut candidate = if raw.is_empty() || raw == "~" {
        detect_home_dir().unwrap_or_else(|| PathBuf::from(default_terminal_cwd()))
    } else if raw.starts_with("~/") || raw.starts_with("~\\") {
        let home = detect_home_dir().unwrap_or_else(|| PathBuf::from(default_terminal_cwd()));
        home.join(&raw[2..])
    } else {
        PathBuf::from(raw)
    };

    if !candidate.is_absolute() {
        candidate = current_dir.join(candidate);
    }

    let normalized = normalize_path_components(candidate);
    if normalized.is_dir() {
        Some(normalized)
    } else {
        None
    }
}

fn parse_terminal_cd_command(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.eq_ignore_ascii_case("cd") {
        return Some(String::new());
    }
    if !(trimmed.starts_with("cd ") || trimmed.starts_with("cd\t")) {
        return None;
    }
    let mut target = trimmed[2..].trim().to_string();
    if target.is_empty() {
        return Some(String::new());
    }
    if (target.starts_with('"') && target.ends_with('"'))
        || (target.starts_with('\'') && target.ends_with('\''))
    {
        if target.len() >= 2 {
            target = target[1..target.len() - 1].to_string();
        }
    }
    Some(target)
}

fn sync_cwd_from_command_line(
    line: &str,
    current_dir: &mut PathBuf,
    output_tx: &mpsc::UnboundedSender<TerminalServerEvent>,
) {
    let Some(target) = parse_terminal_cd_command(line) else {
        return;
    };
    let Some(next_dir) = resolve_terminal_cd_path(current_dir, &target) else {
        return;
    };
    *current_dir = next_dir.clone();
    let _ = output_tx.send(TerminalServerEvent::Cwd(
        next_dir.to_string_lossy().replace('\\', "/"),
    ));
}

fn process_terminal_input_for_cwd(
    data: &[u8],
    pending_line: &mut String,
    current_dir: &mut PathBuf,
    output_tx: &mpsc::UnboundedSender<TerminalServerEvent>,
) {
    let text = String::from_utf8_lossy(data);
    for ch in text.chars() {
        match ch {
            '\r' | '\n' => {
                if !pending_line.trim().is_empty() {
                    sync_cwd_from_command_line(pending_line, current_dir, output_tx);
                }
                pending_line.clear();
            }
            '\u{8}' | '\u{7f}' => {
                pending_line.pop();
            }
            '\u{1b}' => {}
            _ if ch.is_control() => {}
            _ => pending_line.push(ch),
        }
    }
}

fn build_terminal_chdir_command(path: &str) -> String {
    #[cfg(windows)]
    {
        let escaped = path.replace('\'', "''");
        format!("Set-Location -LiteralPath '{}'\r", escaped)
    }
    #[cfg(not(windows))]
    {
        let escaped = path.replace('\'', "'\\''");
        format!("cd '{}'\n", escaped)
    }
}

fn spawn_terminal_output_reader(
    mut reader: Box<dyn Read + Send>,
    output_tx: mpsc::UnboundedSender<TerminalServerEvent>,
) {
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(size) => {
                    if output_tx
                        .send(TerminalServerEvent::Output(buffer[..size].to_vec()))
                        .is_err()
                    {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

fn spawn_terminal_input_writer(
    master: Box<dyn portable_pty::MasterPty + Send>,
    mut writer: Box<dyn Write + Send>,
    input_rx: std_mpsc::Receiver<TerminalInputEvent>,
    output_tx: mpsc::UnboundedSender<TerminalServerEvent>,
    initial_cwd: String,
) {
    std::thread::spawn(move || {
        let mut current_dir = PathBuf::from(initial_cwd);
        if !current_dir.is_dir() {
            current_dir = PathBuf::from(default_terminal_cwd());
        }
        let mut pending_line = String::new();

        for event in input_rx {
            match event {
                TerminalInputEvent::Input(data) => {
                    if writer.write_all(&data).is_err() {
                        break;
                    }
                    let _ = writer.flush();
                    process_terminal_input_for_cwd(
                        &data,
                        &mut pending_line,
                        &mut current_dir,
                        &output_tx,
                    );
                }
                TerminalInputEvent::Resize { cols, rows } => {
                    let _ = master.resize(PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                }
                TerminalInputEvent::Chdir(path) => {
                    let command = build_terminal_chdir_command(&path);
                    if writer.write_all(command.as_bytes()).is_err() {
                        break;
                    }
                    let _ = writer.flush();
                    if let Some(next_dir) = resolve_terminal_cd_path(&current_dir, &path) {
                        current_dir = next_dir.clone();
                        let _ = output_tx.send(TerminalServerEvent::Cwd(
                            next_dir.to_string_lossy().replace('\\', "/"),
                        ));
                    }
                }
                TerminalInputEvent::Close => break,
            }
        }
    });
}

async fn terminal_ws(ws: WebSocketUpgrade, Query(q): Query<TerminalWsQuery>) -> impl IntoResponse {
    let cwd = normalize_terminal_cwd(q.cwd);
    ws.on_upgrade(move |socket| handle_terminal_socket(socket, cwd))
}

async fn send_terminal_error(socket: &mut WebSocket, message: &str) {
    let _ = socket
        .send(Message::Text(format!("[LinxPanel] {}\r\n", message).into()))
        .await;
}

async fn handle_terminal_socket(mut socket: WebSocket, cwd: String) {
    let pty_system = native_pty_system();
    let pair = match pty_system.openpty(PtySize {
        rows: 30,
        cols: 120,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(pair) => pair,
        Err(err) => {
            send_terminal_error(&mut socket, &format!("open terminal failed: {}", err)).await;
            return;
        }
    };

    let shell_cmd = build_terminal_shell(&cwd);
    let mut child = match pair.slave.spawn_command(shell_cmd) {
        Ok(child) => child,
        Err(err) => {
            send_terminal_error(&mut socket, &format!("start shell failed: {}", err)).await;
            return;
        }
    };

    let output_reader = match pair.master.try_clone_reader() {
        Ok(reader) => reader,
        Err(err) => {
            send_terminal_error(&mut socket, &format!("open terminal reader failed: {}", err)).await;
            let _ = child.kill();
            return;
        }
    };

    let input_writer = match pair.master.take_writer() {
        Ok(writer) => writer,
        Err(err) => {
            send_terminal_error(&mut socket, &format!("open terminal writer failed: {}", err)).await;
            let _ = child.kill();
            return;
        }
    };

    let (output_tx, mut output_rx) = mpsc::unbounded_channel::<TerminalServerEvent>();
    let (input_tx, input_rx) = std_mpsc::channel::<TerminalInputEvent>();

    spawn_terminal_output_reader(output_reader, output_tx.clone());
    spawn_terminal_input_writer(pair.master, input_writer, input_rx, output_tx.clone(), cwd.clone());

    let mut output_closed = false;
    loop {
        tokio::select! {
            maybe_output = output_rx.recv(), if !output_closed => {
                match maybe_output {
                    Some(event) => {
                        match event {
                            TerminalServerEvent::Output(data) => {
                                if socket.send(Message::Binary(data.into())).await.is_err() {
                                    break;
                                }
                            }
                            TerminalServerEvent::Cwd(path) => {
                                let payload = TerminalServerControl {
                                    control_type: "cwd".to_string(),
                                    path,
                                };
                                let text = serde_json::to_string(&payload).unwrap_or_else(|_| {
                                    "{\"type\":\"cwd\",\"path\":\"/\"}".to_string()
                                });
                                if socket.send(Message::Text(text.into())).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                    None => {
                        output_closed = true;
                    }
                }
            }
            maybe_message = socket.next() => {
                match maybe_message {
                    Some(Ok(Message::Text(text))) => {
                        if let Some(control_event) = parse_terminal_control(&text) {
                            let _ = input_tx.send(control_event);
                        } else {
                            let _ = input_tx.send(TerminalInputEvent::Input(text.as_bytes().to_vec()));
                        }
                    }
                    Some(Ok(Message::Binary(data))) => {
                        let _ = input_tx.send(TerminalInputEvent::Input(data.to_vec()));
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Close(_))) => break,
                    Some(Err(_)) | None => break,
                }
            }
        }

        if output_closed {
            break;
        }
    }

    let _ = input_tx.send(TerminalInputEvent::Close);
    let _ = child.kill();
}

#[derive(Debug, Deserialize)]
struct ExecRequest { cmd: String, cwd: Option<String> }

#[derive(Debug, Serialize)]
struct ExecResult { stdout: String, stderr: String, exit_code: i32 }

#[derive(Debug, Serialize)]
struct ExecStreamChunk {
    #[serde(rename = "type")]
    chunk_type: String,
    data: String,
    exit_code: Option<i32>,
    done: bool,
}

fn stream_chunk_bytes(chunk_type: &str, data: &str, exit_code: Option<i32>, done: bool) -> Bytes {
    let payload = ExecStreamChunk {
        chunk_type: chunk_type.to_string(),
        data: data.to_string(),
        exit_code,
        done,
    };
    let mut line = serde_json::to_string(&payload).unwrap_or_else(|_| {
        "{\"type\":\"err\",\"data\":\"serialize error\",\"exit_code\":-1,\"done\":false}".to_string()
    });
    line.push('\n');
    Bytes::from(line)
}

fn take_utf8_chunks(buffer: &mut Vec<u8>, final_flush: bool) -> Vec<String> {
    let mut chunks = Vec::new();
    loop {
        if buffer.is_empty() {
            break;
        }
        match std::str::from_utf8(buffer) {
            Ok(text) => {
                if !text.is_empty() {
                    chunks.push(text.to_string());
                }
                buffer.clear();
                break;
            }
            Err(err) => {
                let valid = err.valid_up_to();
                if valid > 0 {
                    let text = String::from_utf8_lossy(&buffer[..valid]).to_string();
                    if !text.is_empty() {
                        chunks.push(text);
                    }
                    buffer.drain(..valid);
                    continue;
                }
                if err.error_len().is_some() {
                    let text = String::from_utf8_lossy(buffer).to_string();
                    if !text.is_empty() {
                        chunks.push(text);
                    }
                    buffer.clear();
                }
                break;
            }
        }
    }

    if final_flush && !buffer.is_empty() {
        let text = String::from_utf8_lossy(buffer).to_string();
        if !text.is_empty() {
            chunks.push(text);
        }
        buffer.clear();
    }

    chunks
}

async fn stream_pipe_output<R>(
    mut reader: R,
    chunk_type: &'static str,
    tx: mpsc::Sender<Result<Bytes, Infallible>>,
)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let mut buf = [0_u8; 4096];
    let mut pending = Vec::<u8>::new();
    loop {
        match reader.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                pending.extend_from_slice(&buf[..n]);
                for part in take_utf8_chunks(&mut pending, false) {
                    if tx
                        .send(Ok(stream_chunk_bytes(chunk_type, &part, None, false)))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
            Err(err) => {
                let _ = tx
                    .send(Ok(stream_chunk_bytes(
                        "err",
                        &format!("stream read error: {}", err),
                        None,
                        false,
                    )))
                    .await;
                return;
            }
        }
    }

    for part in take_utf8_chunks(&mut pending, true) {
        if tx
            .send(Ok(stream_chunk_bytes(chunk_type, &part, None, false)))
            .await
            .is_err()
        {
            return;
        }
    }
}

async fn exec_cmd(Json(req): Json<ExecRequest>) -> Json<ApiResponse<ExecResult>> {
    let cwd = normalize_fs_path(&req.cwd.unwrap_or_else(|| "/".into()));
    let shell = if cfg!(windows) { "cmd" } else { "sh" };
    let flag = if cfg!(windows) { "/C" } else { "-c" };

    match std::process::Command::new(shell)
        .arg(flag)
        .arg(&req.cmd)
        .current_dir(&cwd)
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);
            Json(ApiResponse::ok(ExecResult { stdout, stderr, exit_code }))
        }
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn upload_files(mut multipart: Multipart) -> Json<ApiResponse<UploadResult>> {
    let mut target_dir_path = String::new();
    let mut uploaded_files: Vec<String> = Vec::new();

    loop {
        let Some(field) = (match multipart.next_field().await {
            Ok(item) => item,
            Err(error) => {
                return Json(ApiResponse::err(&format!(
                    "Failed to parse upload request: {}",
                    error
                )))
            }
        }) else {
            break;
        };

        let field_name = field.name().unwrap_or_default().to_string();
        if field_name == "path" {
            let raw_path = match field.text().await {
                Ok(text) => text,
                Err(error) => {
                    return Json(ApiResponse::err(&format!(
                        "Failed to read upload field: {}",
                        error
                    )))
                }
            };
            let normalized_path = normalize_fs_path(&raw_path);
            if normalized_path.trim().is_empty() {
                return Json(ApiResponse::err("Upload target path is required"));
            }
            let target_dir = std::path::Path::new(&normalized_path);
            if !target_dir.exists() {
                return Json(ApiResponse::err("Path does not exist"));
            }
            if !target_dir.is_dir() {
                return Json(ApiResponse::err("Upload target path is not a directory"));
            }
            target_dir_path = normalized_path;
            continue;
        }

        if field_name != "files" {
            continue;
        }

        if target_dir_path.is_empty() {
            return Json(ApiResponse::err("Upload target path is required"));
        }

        let raw_name = field.file_name().unwrap_or_default();
        let Some(file_name) = sanitize_upload_filename(raw_name) else {
            return Json(ApiResponse::err("Invalid upload file name"));
        };

        let file_bytes = match field.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                return Json(ApiResponse::err(&format!(
                    "Failed to read upload field: {}",
                    error
                )))
            }
        };

        let target_file = std::path::Path::new(&target_dir_path).join(&file_name);
        if let Err(error) = std::fs::write(&target_file, file_bytes) {
            return Json(ApiResponse::err(&format!(
                "Failed to save upload: {}",
                error
            )));
        }

        uploaded_files.push(target_file.to_string_lossy().replace('\\', "/"));
    }

    if uploaded_files.is_empty() {
        return Json(ApiResponse::err("No file uploaded"));
    }

    Json(ApiResponse::ok(UploadResult {
        uploaded: uploaded_files.len(),
        files: uploaded_files,
    }))
}

async fn exec_cmd_stream(Json(req): Json<ExecRequest>) -> Response {
    let cwd = normalize_fs_path(&req.cwd.unwrap_or_else(|| "/".into()));
    let shell = if cfg!(windows) { "cmd" } else { "sh" };
    let flag = if cfg!(windows) { "/C" } else { "-c" };

    let mut command = tokio::process::Command::new(shell);
    command
        .arg(flag)
        .arg(&req.cmd)
        .current_dir(&cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::<()>::err(&err.to_string())),
            )
                .into_response();
        }
    };

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (tx, rx) = mpsc::channel::<Result<Bytes, Infallible>>(128);

    tokio::spawn(async move {
        let stdout_task = stdout.map(|stream| tokio::spawn(stream_pipe_output(stream, "out", tx.clone())));
        let stderr_task = stderr.map(|stream| tokio::spawn(stream_pipe_output(stream, "err", tx.clone())));

        let exit_code = match child.wait().await {
            Ok(status) => status.code().unwrap_or(-1),
            Err(err) => {
                let _ = tx
                    .send(Ok(stream_chunk_bytes(
                        "err",
                        &format!("command execution failed: {}", err),
                        None,
                        false,
                    )))
                    .await;
                -1
            }
        };

        if let Some(task) = stdout_task {
            let _ = task.await;
        }
        if let Some(task) = stderr_task {
            let _ = task.await;
        }

        let _ = tx
            .send(Ok(stream_chunk_bytes("exit", "", Some(exit_code), true)))
            .await;
    });

    let stream = ReceiverStream::new(rx);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(stream))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}
