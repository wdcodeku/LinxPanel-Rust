//! File manager endpoints beyond single files: resumable chunked uploads
//! (with folders), extracting and creating archives, copy / move.
//!
//! Uploads go in chunks of a few MB. Each chunk says where it starts; the
//! server appends it to a hidden partial file and answers with the size it
//! now has. After a dropped connection the page asks for that size and
//! carries on from there, so a flaky network costs a retry, not the upload.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Query, State},
    routing::{get, post},
    Json, Router,
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};

use super::files::{normalize_fs_path, sanitize_upload_filename};
use crate::models::ApiResponse;
use crate::services::fileops::{self, CompressFormat, Conflict};
use crate::services::tasks;
use crate::AppState;

/// A chunk is a few MB; this only bounds a misbehaving client.
const CHUNK_LIMIT: usize = 64 * 1024 * 1024;
/// Partial files nobody touched for this long are leftovers of abandoned uploads.
const STALE_PARTIAL: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/files/upload/chunk",
            post(upload_chunk).layer(DefaultBodyLimit::max(CHUNK_LIMIT)),
        )
        .route("/files/upload/status", get(upload_status))
        .route("/files/upload/abort", post(upload_abort))
        .route("/files/extract", post(extract))
        .route("/files/compress", post(compress))
        .route("/files/transfer", post(transfer))
}

// ===== Chunked uploads =====

#[derive(Debug, Deserialize)]
struct UploadTarget {
    /// The directory the upload goes into.
    dir: String,
    /// File name, or a relative path for folder uploads ("site/css/a.css").
    name: String,
    /// The page's id for this upload (one per file).
    id: String,
}

#[derive(Debug, Deserialize)]
struct ChunkQuery {
    #[serde(flatten)]
    target: UploadTarget,
    offset: u64,
    total: u64,
    /// Octal permissions for the finished file ("755"); default 644.
    #[serde(default)]
    mode: String,
}

#[derive(Debug, Serialize)]
struct ChunkState {
    /// Bytes the server has; the next chunk starts here.
    size: u64,
    complete: bool,
}

struct Resolved {
    final_path: PathBuf,
    partial: PathBuf,
    base_dir: PathBuf,
}

fn valid_id(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The final and partial paths of an upload, validated: the name may hold
/// folders but never leaves `dir`.
fn resolve(target: &UploadTarget) -> Result<Resolved, String> {
    if !valid_id(&target.id) {
        return Err("Invalid upload id".into());
    }
    let base_dir = PathBuf::from(normalize_fs_path(&target.dir));
    if !base_dir.is_dir() {
        return Err("Upload target path is not a directory".into());
    }
    let parts: Vec<&str> = target
        .name
        .split(['/', '\\'])
        .filter(|p| !p.is_empty())
        .collect();
    if parts.is_empty() || parts.len() > 64 {
        return Err("Invalid upload file name".into());
    }
    let mut final_path = base_dir.clone();
    for part in &parts {
        // Each part must be a plain name (sanitize keeps only the last
        // segment, so a part with separators or dots fails the comparison).
        match sanitize_upload_filename(part) {
            Some(clean) if clean == *part && part.len() <= 255 => final_path.push(part),
            _ => return Err("Invalid upload file name".into()),
        }
    }
    let parent = final_path.parent().ok_or("Invalid upload file name")?;
    let file_name = final_path
        .file_name()
        .ok_or("Invalid upload file name")?
        .to_string_lossy()
        .to_string();
    let partial = parent.join(format!(".{}.uploading-{}", file_name, target.id));
    Ok(Resolved {
        final_path,
        partial,
        base_dir,
    })
}

fn parse_mode(raw: &str) -> Result<u32, String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(0o644);
    }
    if !(3..=4).contains(&raw.len()) || !raw.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
        return Err("Mode must be octal, e.g. 644 or 755".into());
    }
    u32::from_str_radix(raw, 8)
        .map(|m| m & 0o777)
        .map_err(|e| e.to_string())
}

/// Removes partial files of the same name left by abandoned uploads.
fn remove_stale_partials(parent: &Path, file_name: &str) {
    let prefix = format!(".{}.uploading-", file_name);
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(&prefix) {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > STALE_PARTIAL);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn friendly_io(err: &std::io::Error) -> String {
    match err.raw_os_error() {
        Some(28) => "No space left on the disk".into(),
        Some(13) => "Permission denied".into(),
        _ => err.to_string(),
    }
}

async fn upload_chunk(Query(q): Query<ChunkQuery>, body: Body) -> Json<ApiResponse<ChunkState>> {
    match receive_chunk(q, body).await {
        Ok(state) => Json(ApiResponse::ok(state)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn receive_chunk(q: ChunkQuery, body: Body) -> Result<ChunkState, String> {
    let mode = parse_mode(&q.mode)?;
    let resolved = tokio::task::spawn_blocking({
        let target = UploadTarget {
            dir: q.target.dir.clone(),
            name: q.target.name.clone(),
            id: q.target.id.clone(),
        };
        move || -> Result<Resolved, String> {
            let resolved = resolve(&target)?;
            let parent = resolved
                .partial
                .parent()
                .ok_or("Invalid upload file name")?;
            if !parent.exists() {
                // Folder uploads: new folders get the owner of the folder above.
                fileops::create_dirs_like_parent(parent).map_err(|e| friendly_io(&e))?;
            }
            if resolved.final_path.is_dir() {
                return Err(format!(
                    "A folder named {} already exists here",
                    resolved
                        .final_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                ));
            }
            Ok(resolved)
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    let mut file = if q.offset == 0 {
        if let (Some(parent), Some(name)) =
            (resolved.partial.parent(), resolved.final_path.file_name())
        {
            let (parent, name) = (parent.to_path_buf(), name.to_string_lossy().to_string());
            let _ =
                tokio::task::spawn_blocking(move || remove_stale_partials(&parent, &name)).await;
        }
        tokio::fs::File::create(&resolved.partial)
            .await
            .map_err(|e| friendly_io(&e))?
    } else {
        let current = tokio::fs::metadata(&resolved.partial)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        if current != q.offset {
            // A retried or lost chunk: tell the page where to continue.
            return Ok(ChunkState {
                size: current,
                complete: false,
            });
        }
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&resolved.partial)
            .await
            .map_err(|e| friendly_io(&e))?;
        file.seek(std::io::SeekFrom::Start(q.offset))
            .await
            .map_err(|e| friendly_io(&e))?;
        file
    };

    let mut size = q.offset;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        // A dropped connection ends here; what arrived stays for the resume.
        let chunk = chunk.map_err(|e| format!("Upload interrupted: {}", e))?;
        size += chunk.len() as u64;
        if size > q.total {
            drop(file);
            let _ = tokio::fs::remove_file(&resolved.partial).await;
            return Err("The upload is larger than announced".into());
        }
        file.write_all(&chunk).await.map_err(|e| friendly_io(&e))?;
    }
    file.flush().await.map_err(|e| friendly_io(&e))?;

    if size < q.total {
        return Ok(ChunkState {
            size,
            complete: false,
        });
    }
    file.sync_all().await.map_err(|e| friendly_io(&e))?;
    drop(file);
    let partial = resolved.partial.clone();
    let final_path = resolved.final_path.clone();
    let base_dir = resolved.base_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(mode))
                .map_err(|e| friendly_io(&e))?;
        }
        std::fs::rename(&partial, &final_path).map_err(|e| {
            let _ = std::fs::remove_file(&partial);
            friendly_io(&e)
        })?;
        let parent = final_path.parent().unwrap_or(&base_dir);
        fileops::inherit_owner(&final_path, parent);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(ChunkState {
        size,
        complete: true,
    })
}

async fn upload_status(Query(target): Query<UploadTarget>) -> Json<ApiResponse<ChunkState>> {
    let result = tokio::task::spawn_blocking(move || -> Result<ChunkState, String> {
        let resolved = resolve(&target)?;
        let size = std::fs::metadata(&resolved.partial)
            .map(|m| m.len())
            .unwrap_or(0);
        Ok(ChunkState {
            size,
            complete: false,
        })
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    match result {
        Ok(state) => Json(ApiResponse::ok(state)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn upload_abort(Json(target): Json<UploadTarget>) -> Json<ApiResponse<()>> {
    let result = tokio::task::spawn_blocking(move || -> Result<(), String> {
        let resolved = resolve(&target)?;
        let _ = std::fs::remove_file(&resolved.partial);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    match result {
        Ok(()) => Json(ApiResponse::ok_msg("Upload canceled")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

// ===== Archives =====

#[derive(Debug, Deserialize)]
struct ExtractRequest {
    /// The archive.
    path: String,
    /// Where to put its contents (created when missing).
    dest: String,
    #[serde(default = "yes")]
    overwrite: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Serialize)]
struct CountResult {
    files: usize,
    skipped: usize,
}

async fn extract(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ExtractRequest>,
) -> Json<ApiResponse<tasks::Started>> {
    let archive = PathBuf::from(normalize_fs_path(&req.path));
    let dest = PathBuf::from(normalize_fs_path(&req.dest));
    if !archive.is_file() {
        return Json(ApiResponse::err("File not found"));
    }
    if !dest.is_absolute() {
        return Json(ApiResponse::err("Choose an absolute destination path"));
    }
    let title = format!(
        "Extract {}",
        archive
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    );
    Json(ApiResponse::ok(state.tasks.start(
        "files-extract",
        &title,
        async move {
            match tasks::blocking(move || fileops::extract(&archive, &dest, req.overwrite)).await {
                Ok(Ok(summary)) => ApiResponse::ok(CountResult {
                    files: summary.files,
                    skipped: summary.skipped,
                }),
                Ok(Err(err)) => ApiResponse::err(&err),
                Err(err) => ApiResponse::err(&err.to_string()),
            }
        },
    )))
}

#[derive(Debug, Deserialize)]
struct CompressRequest {
    dir: String,
    names: Vec<String>,
    /// The archive's file name (in `dir`).
    name: String,
    #[serde(default = "default_format")]
    format: String,
    #[serde(default)]
    overwrite: bool,
}

fn default_format() -> String {
    "zip".into()
}

async fn compress(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CompressRequest>,
) -> Json<ApiResponse<tasks::Started>> {
    let format = match CompressFormat::parse(&req.format) {
        Ok(format) => format,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    let dir = PathBuf::from(normalize_fs_path(&req.dir));
    if !dir.is_dir() {
        return Json(ApiResponse::err("Path is not a directory"));
    }
    let Some(name) = sanitize_upload_filename(&req.name).filter(|n| *n == req.name.trim()) else {
        return Json(ApiResponse::err("Invalid archive name"));
    };
    let output = dir.join(&name);
    if output.exists() && !req.overwrite {
        return Json(ApiResponse::err(&format!("{} already exists", name)));
    }
    let title = format!("Compress {}", name);
    let names = req.names;
    Json(ApiResponse::ok(state.tasks.start(
        "files-compress",
        &title,
        async move {
            match tasks::blocking(move || fileops::compress(&dir, &names, &output, format)).await {
                Ok(Ok(files)) => ApiResponse::ok(CountResult { files, skipped: 0 }),
                Ok(Err(err)) => ApiResponse::err(&err),
                Err(err) => ApiResponse::err(&err.to_string()),
            }
        },
    )))
}

// ===== Copy / move =====

#[derive(Debug, Deserialize)]
struct TransferRequest {
    sources: Vec<String>,
    dest: String,
    /// "copy" or "move".
    #[serde(default)]
    mode: String,
    /// "overwrite" | "skip" | "rename" (keep both).
    #[serde(default)]
    conflict: String,
}

async fn transfer(
    State(state): State<Arc<AppState>>,
    Json(req): Json<TransferRequest>,
) -> Json<ApiResponse<tasks::Started>> {
    if req.sources.is_empty() {
        return Json(ApiResponse::err("Nothing selected"));
    }
    let moving = req.mode == "move";
    let sources: Vec<PathBuf> = req
        .sources
        .iter()
        .map(|s| PathBuf::from(normalize_fs_path(s)))
        .collect();
    if sources
        .iter()
        .any(|s| !s.is_absolute() || s.parent().is_none())
    {
        return Json(ApiResponse::err("Invalid source path"));
    }
    let dest = PathBuf::from(normalize_fs_path(&req.dest));
    let conflict = Conflict::parse(&req.conflict);
    let title = format!(
        "{} {} item(s)",
        if moving { "Move" } else { "Copy" },
        sources.len()
    );
    Json(ApiResponse::ok(state.tasks.start(
        "files-transfer",
        &title,
        async move {
            match tasks::blocking(move || fileops::transfer(&sources, &dest, moving, conflict))
                .await
            {
                Ok(Ok(summary)) => ApiResponse::ok(CountResult {
                    files: summary.done,
                    skipped: summary.skipped,
                }),
                Ok(Err(err)) => ApiResponse::err(&err),
                Err(err) => ApiResponse::err(&err.to_string()),
            }
        },
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(name: &str) -> UploadTarget {
        UploadTarget {
            dir: std::env::temp_dir().to_string_lossy().to_string(),
            name: name.into(),
            id: "abcdef123456".into(),
        }
    }

    #[test]
    fn upload_names_stay_inside() {
        assert!(resolve(&target("a.txt")).is_ok());
        assert!(resolve(&target("site/css/a.css")).is_ok());
        assert!(resolve(&target("../x")).is_err());
        assert!(resolve(&target("a/../../x")).is_err());
        assert!(resolve(&target("./a")).is_err());
        assert!(resolve(&target("")).is_err());
        let mut bad_id = target("a.txt");
        bad_id.id = "../../x".into();
        assert!(resolve(&bad_id).is_err());
        let resolved = resolve(&target("site/a.css")).unwrap();
        assert!(resolved
            .partial
            .ends_with("site/.a.css.uploading-abcdef123456"));
    }

    #[test]
    fn modes_parse() {
        assert_eq!(parse_mode("").unwrap(), 0o644);
        assert_eq!(parse_mode("755").unwrap(), 0o755);
        assert_eq!(parse_mode("4755").unwrap(), 0o755);
        assert!(parse_mode("9").is_err());
    }
}
