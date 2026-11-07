//! Downloads the browser handles natively: an authenticated request mints a
//! short-lived, unguessable link for one path, and the browser fetches that
//! link itself - so its download manager shows the file at once, with
//! progress, instead of the page buffering the whole file first. Files support
//! Range requests (resume); folders stream as a .tar.gz made on the fly.

use axum::{
    body::Body,
    extract::Path as UrlPath,
    http::{header, HeaderMap, StatusCode},
    response::Response,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use super::files::normalize_fs_path;
use crate::models::ApiResponse;
use crate::AppState;

/// Long enough for a browser to start the transfer and resume it soon after.
const LINK_TTL: Duration = Duration::from_secs(15 * 60);
const MAX_LINKS: usize = 2000;

struct Link {
    path: PathBuf,
    expires: Instant,
}

fn links() -> &'static Mutex<HashMap<String, Link>> {
    static LINKS: OnceLock<Mutex<HashMap<String, Link>>> = OnceLock::new();
    LINKS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Behind the panel's login.
pub fn routes() -> Router<Arc<AppState>> {
    Router::new().route("/files/download-link", post(create_link))
}

/// Reachable without a session: the token in the URL is the credential.
/// (The panel's access policy - allowed IPs, bound domain - still applies.)
pub fn public_routes() -> Router<Arc<AppState>> {
    Router::new().route("/files/dl/{token}/{name}", get(serve))
}

#[derive(Deserialize)]
struct LinkRequest {
    path: String,
}

#[derive(Serialize)]
struct LinkResponse {
    url: String,
    name: String,
    is_dir: bool,
}

fn random_token() -> String {
    // Two v4 UUIDs: 244 bits from the OS CSPRNG.
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// The file name a download is saved under (a folder gets .tar.gz).
fn download_name(path: &Path, is_dir: bool) -> String {
    let base = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "root".into());
    if is_dir {
        format!("{}.tar.gz", base)
    } else {
        base
    }
}

/// RFC 3986 encoding of one path segment / RFC 5987 value.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{:02X}", byte));
        }
    }
    out
}

async fn create_link(Json(req): Json<LinkRequest>) -> Json<ApiResponse<LinkResponse>> {
    let path = PathBuf::from(normalize_fs_path(&req.path));
    let meta = match tokio::fs::metadata(&path).await {
        Ok(meta) => meta,
        Err(_) => return Json(ApiResponse::err("Path not found")),
    };
    if !meta.is_file() && !meta.is_dir() {
        return Json(ApiResponse::err("Only files and folders can be downloaded"));
    }
    let is_dir = meta.is_dir();
    let name = download_name(&path, is_dir);
    let token = random_token();
    {
        let mut map = links().lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        map.retain(|_, link| link.expires > now);
        if map.len() >= MAX_LINKS {
            return Json(ApiResponse::err(
                "Too many pending downloads; try again shortly",
            ));
        }
        map.insert(
            token.clone(),
            Link {
                path,
                expires: now + LINK_TTL,
            },
        );
    }
    // The name in the URL is what browsers fall back to for the file name.
    Json(ApiResponse::ok(LinkResponse {
        url: format!("/api/files/dl/{}/{}", token, percent_encode(&name)),
        name,
        is_dir,
    }))
}

fn plain(status: StatusCode, text: &str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(text.to_string()))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

fn disposition(name: &str) -> String {
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!(
        "attachment; filename=\"{}\"; filename*=UTF-8''{}",
        ascii,
        percent_encode(name)
    )
}

/// `bytes=a-b`, `bytes=a-`, `bytes=-n` against a file of `len` bytes.
/// None: not a single satisfiable range (serve the whole file / 416).
fn parse_range(value: &str, len: u64) -> Option<Result<(u64, u64), ()>> {
    let spec = value.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None; // multipart ranges: send the whole file
    }
    let (start, end) = spec.split_once('-')?;
    let (start, end) = (start.trim(), end.trim());
    if len == 0 {
        return Some(Err(()));
    }
    let range = if start.is_empty() {
        let suffix: u64 = end.parse().ok()?;
        if suffix == 0 {
            return Some(Err(()));
        }
        (len.saturating_sub(suffix), len - 1)
    } else {
        let start: u64 = start.parse().ok()?;
        let end: u64 = if end.is_empty() {
            len - 1
        } else {
            end.parse::<u64>().ok()?.min(len - 1)
        };
        if start > end || start >= len {
            return Some(Err(()));
        }
        (start, end)
    };
    Some(Ok(range))
}

async fn serve(
    UrlPath((token, _name)): UrlPath<(String, String)>,
    headers: HeaderMap,
) -> Response<Body> {
    let path = {
        let map = links().lock().unwrap_or_else(|e| e.into_inner());
        match map.get(&token) {
            Some(link) if link.expires > Instant::now() => link.path.clone(),
            _ => {
                return plain(
                    StatusCode::NOT_FOUND,
                    "This download link has expired. Start the download again from the panel.",
                )
            }
        }
    };
    let meta = match tokio::fs::metadata(&path).await {
        Ok(meta) => meta,
        Err(_) => return plain(StatusCode::NOT_FOUND, "File not found"),
    };
    if meta.is_dir() {
        return serve_dir(path);
    }
    let name = download_name(&path, false);
    let len = meta.len();
    let mut file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(err) => {
            return plain(
                StatusCode::FORBIDDEN,
                &format!("Cannot read the file: {}", err),
            )
        }
    };
    let modified = meta.modified().ok().map(|t| httpdate(t));
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| parse_range(v, len));
    let mime = mime_guess::from_path(&path).first_or_octet_stream();
    let mut builder = Response::builder()
        .header(header::CONTENT_TYPE, mime.as_ref())
        .header(header::CONTENT_DISPOSITION, disposition(&name))
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "no-store");
    if let Some(modified) = modified {
        builder = builder.header(header::LAST_MODIFIED, modified);
    }
    match range {
        Some(Err(())) => Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{}", len))
            .body(Body::empty())
            .unwrap_or_else(|_| Response::new(Body::empty())),
        Some(Ok((start, end))) => {
            if file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
                return plain(StatusCode::INTERNAL_SERVER_ERROR, "Seek failed");
            }
            let count = end - start + 1;
            builder
                .status(StatusCode::PARTIAL_CONTENT)
                .header(
                    header::CONTENT_RANGE,
                    format!("bytes {}-{}/{}", start, end, len),
                )
                .header(header::CONTENT_LENGTH, count)
                .body(Body::from_stream(tokio_util::io::ReaderStream::new(
                    file.take(count),
                )))
                .unwrap_or_else(|_| Response::new(Body::empty()))
        }
        None => builder
            .status(StatusCode::OK)
            .header(header::CONTENT_LENGTH, len)
            .body(Body::from_stream(tokio_util::io::ReaderStream::new(file)))
            .unwrap_or_else(|_| Response::new(Body::empty())),
    }
}

fn httpdate(time: std::time::SystemTime) -> String {
    let utc: chrono::DateTime<chrono::Utc> = time.into();
    utc.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// A folder as a gzip-compressed tar, produced while it is sent: nothing is
/// written to disk and memory stays flat whatever the folder's size.
fn serve_dir(path: PathBuf) -> Response<Body> {
    let name = download_name(&path, true);
    let (reader, writer) = tokio::io::duplex(256 * 1024);
    let root_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "root".into());
    tokio::task::spawn_blocking(move || {
        let sink = tokio_util::io::SyncIoBridge::new(writer);
        let gz = flate2::write::GzEncoder::new(sink, flate2::Compression::fast());
        let mut tar = tar::Builder::new(gz);
        // Links are archived as links, never followed out of the folder.
        tar.follow_symlinks(false);
        let result = tar
            .append_dir_all(&root_name, &path)
            .and_then(|_| tar.into_inner())
            .and_then(|gz| gz.finish());
        if let Err(err) = result {
            // The client sees a truncated archive; the reason goes to the log.
            tracing::warn!("Folder download {} stopped: {}", path.display(), err);
        }
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/gzip")
        .header(header::CONTENT_DISPOSITION, disposition(&name))
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from_stream(tokio_util::io::ReaderStream::new(reader)))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-9", 100), Some(Ok((0, 9))));
        assert_eq!(parse_range("bytes=90-", 100), Some(Ok((90, 99))));
        assert_eq!(parse_range("bytes=-10", 100), Some(Ok((90, 99))));
        assert_eq!(parse_range("bytes=50-500", 100), Some(Ok((50, 99))));
        assert_eq!(parse_range("bytes=100-", 100), Some(Err(())));
        assert_eq!(parse_range("bytes=9-3", 100), Some(Err(())));
        assert_eq!(parse_range("bytes=0-1,5-6", 100), None);
        assert_eq!(parse_range("items=0-1", 100), None);
    }

    #[test]
    fn names_and_headers() {
        assert_eq!(download_name(Path::new("/www/site"), true), "site.tar.gz");
        assert_eq!(download_name(Path::new("/"), true), "root.tar.gz");
        assert_eq!(percent_encode("报告 v1.txt"), "%E6%8A%A5%E5%91%8A%20v1.txt");
        let d = disposition("报告\"x.txt");
        assert!(d.starts_with("attachment; filename=\"___"));
        assert!(d.contains("filename*=UTF-8''%E6%8A%A5%E5%91%8A%22x.txt"));
        assert_eq!(random_token().len(), 64);
    }
}
