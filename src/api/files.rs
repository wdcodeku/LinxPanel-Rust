use axum::{extract::{Query, State}, routing::{get, post}, Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::models::ApiResponse;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/files/list", get(list_files))
        .route("/files/read", get(read_file))
        .route("/files/write", post(write_file))
        .route("/files/mkdir", post(mkdir))
        .route("/files/delete", post(delete_path))
        .route("/files/rename", post(rename_path))
        .route("/files/info", get(file_info))
        .route("/files/exec", post(exec_cmd))
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

#[derive(Debug, Deserialize)]
struct WriteRequest { path: String, content: String }

#[derive(Debug, Deserialize)]
struct RenameRequest { from: String, to: String }

async fn list_files(Query(q): Query<PathQuery>) -> Json<ApiResponse<Vec<FileEntry>>> {
    let dir = std::path::Path::new(&q.path);
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
                let permissions = if is_dir { "drwxr-xr-x".into() } else { "-rw-r--r--".into() };
                entries.push(FileEntry { name, path, is_dir, size, modified, permissions });
            }
            entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
            Json(ApiResponse::ok(entries))
        }
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}
async fn read_file(Query(q): Query<PathQuery>) -> Json<ApiResponse<FileContent>> {
    let p = std::path::Path::new(&q.path);
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
            path: q.path,
            content,
            size,
        })),
        Err(e) => Json(ApiResponse::err(&format!("Cannot read: {}", e))),
    }
}

async fn write_file(Json(req): Json<WriteRequest>) -> Json<ApiResponse<()>> {
    if let Some(parent) = std::path::Path::new(&req.path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(&req.path, &req.content) {
        Ok(_) => Json(ApiResponse::ok_msg("File saved")),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn mkdir(Json(req): Json<PathQuery>) -> Json<ApiResponse<()>> {
    match std::fs::create_dir_all(&req.path) {
        Ok(_) => Json(ApiResponse::ok_msg("Directory created")),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn delete_path(Json(req): Json<PathQuery>) -> Json<ApiResponse<()>> {
    let p = std::path::Path::new(&req.path);
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
    match std::fs::rename(&req.from, &req.to) {
        Ok(_) => Json(ApiResponse::ok_msg("Renamed")),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn file_info(Query(q): Query<PathQuery>) -> Json<ApiResponse<FileEntry>> {
    let p = std::path::Path::new(&q.path);
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
        path: q.path,
        is_dir,
        size,
        modified,
        permissions: if is_dir { "drwxr-xr-x".into() } else { "-rw-r--r--".into() },
    }))
}


#[derive(Debug, Deserialize)]
struct ChmodRequest { path: String, mode: String }

async fn chmod_path(Json(req): Json<ChmodRequest>) -> Json<ApiResponse<()>> {
    let p = std::path::Path::new(&req.path);
    if !p.exists() {
        return Json(ApiResponse::err("Path not found"));
    }
    #[cfg(unix)]
    {
        let cmd_str = format!("chmod {} '{}'", req.mode, req.path);
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
struct ExecRequest { cmd: String, cwd: Option<String> }

#[derive(Debug, Serialize)]
struct ExecResult { stdout: String, stderr: String, exit_code: i32 }

async fn exec_cmd(Json(req): Json<ExecRequest>) -> Json<ApiResponse<ExecResult>> {
    let cwd = req.cwd.unwrap_or_else(|| "/".into());
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