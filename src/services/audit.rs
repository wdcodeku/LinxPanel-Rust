//! Operation audit log: who changed what, from where, and whether it worked.
//! Every state-changing API call (anything but GET) is recorded with a short
//! description of its target taken from whitelisted request fields; secrets
//! are never stored.

use axum::{
    body::Body,
    extract::{ConnectInfo, Request},
    http::{header, Method},
    middleware::Next,
    response::Response,
};
use serde::Serialize;
use sqlx::SqlitePool;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Requests bodies / responses larger than this are not inspected.
const INSPECT_LIMIT: usize = 64 * 1024;
const KEEP_DAYS: u32 = 180;
const KEEP_ROWS: u32 = 100_000;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Entry {
    pub id: i64,
    pub created_at: String,
    pub username: String,
    pub ip: String,
    pub method: String,
    pub path: String,
    pub target: String,
    pub ok: bool,
    pub message: String,
}

static INSERTS: AtomicU64 = AtomicU64::new(0);

#[allow(clippy::too_many_arguments)]
pub async fn record(
    pool: &SqlitePool,
    username: &str,
    ip: &str,
    method: &str,
    path: &str,
    target: &str,
    ok: bool,
    message: &str,
) {
    let _ = sqlx::query(
        "INSERT INTO audit_log (username, ip, method, path, target, ok, message) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(username)
    .bind(ip)
    .bind(method)
    .bind(path)
    .bind(target)
    .bind(ok)
    .bind(message.chars().take(300).collect::<String>())
    .execute(pool)
    .await;
    // Trim now and then, not on every write.
    if INSERTS.fetch_add(1, Ordering::Relaxed) % 500 == 0 {
        let _ = sqlx::query("DELETE FROM audit_log WHERE created_at < datetime('now', ?)")
            .bind(format!("-{} days", KEEP_DAYS))
            .execute(pool)
            .await;
        let _ = sqlx::query(
            "DELETE FROM audit_log WHERE id <= (SELECT id FROM audit_log ORDER BY id DESC LIMIT 1 OFFSET ?)",
        )
        .bind(KEEP_ROWS)
        .execute(pool)
        .await;
    }
}

/// Paths whose calls are routine plumbing, not operations.
fn ignored(path: &str) -> bool {
    path.starts_with("/files/upload/chunk")
        || path.starts_with("/files/upload/status")
        || path == "/files/exists"
        || path.starts_with("/tasks")
        || path == "/auth/refresh"
}

fn is_secret_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    [
        "pass", "secret", "token", "key", "content", "private", "cert",
    ]
    .iter()
    .any(|w| k.contains(w))
}

fn short(text: &str, max: usize) -> String {
    let text = text.replace(['\n', '\r'], " ");
    if text.chars().count() > max {
        format!("{}…", text.chars().take(max).collect::<String>())
    } else {
        text
    }
}

/// A one-line description of what a request acts on.
pub fn describe(json: &serde_json::Value) -> String {
    const KEYS: &[&str] = &[
        "path",
        "paths",
        "from",
        "to",
        "dest",
        "name",
        "names",
        "domain",
        "domains",
        "app",
        "runtime",
        "kind",
        "engine",
        "database",
        "db_name",
        "username",
        "user",
        "table",
        "ids",
        "id",
        "port",
        "ip",
        "rule",
        "version",
        "cmd",
        "sql",
        "work_dir",
        "entry",
        "permanent",
        "mode",
    ];
    let Some(object) = json.as_object() else {
        return String::new();
    };
    let mut parts = Vec::new();
    for key in KEYS {
        let Some(value) = object.get(*key) else {
            continue;
        };
        if is_secret_key(key) {
            continue;
        }
        let text = match value {
            serde_json::Value::String(s) if s.is_empty() => continue,
            serde_json::Value::String(s) => short(
                s,
                if *key == "sql" || *key == "cmd" {
                    160
                } else {
                    120
                },
            ),
            serde_json::Value::Array(items) if items.is_empty() => continue,
            serde_json::Value::Array(items) => {
                let shown: Vec<String> = items
                    .iter()
                    .take(3)
                    .map(|v| {
                        v.as_str()
                            .map(|s| short(s, 80))
                            .unwrap_or_else(|| v.to_string())
                    })
                    .collect();
                if items.len() > 3 {
                    format!("{} (+{})", shown.join(", "), items.len() - 3)
                } else {
                    shown.join(", ")
                }
            }
            serde_json::Value::Null => continue,
            other => other.to_string(),
        };
        parts.push(format!("{}={}", key, text));
        if parts.len() >= 5 {
            break;
        }
    }
    parts.join(" · ")
}

fn content_length(headers: &axum::http::HeaderMap) -> Option<usize> {
    headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
}

fn is_json(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"))
}

/// Middleware for the authenticated API (inside the auth layer).
pub async fn middleware(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    if matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return next.run(req).await;
    }
    let path = req.uri().path().to_string();
    if ignored(&path) {
        return next.run(req).await;
    }
    let state = req.extensions().get::<Arc<crate::AppState>>().cloned();
    let username = req
        .extensions()
        .get::<crate::models::Claims>()
        .map(|c| c.username.clone())
        .unwrap_or_default();
    let ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(peer)| peer.ip().to_canonical().to_string())
        .unwrap_or_default();

    // Read a small JSON body for the target, then hand it on unchanged.
    let (parts, body) = req.into_parts();
    let mut target = String::new();
    let req = if is_json(&parts.headers)
        && content_length(&parts.headers).is_some_and(|n| n <= INSPECT_LIMIT)
    {
        match axum::body::to_bytes(body, INSPECT_LIMIT).await {
            Ok(bytes) => {
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    target = describe(&json);
                }
                Request::from_parts(parts, Body::from(bytes))
            }
            Err(_) => Request::from_parts(parts, Body::empty()),
        }
    } else {
        Request::from_parts(parts, body)
    };

    let response = next.run(req).await;
    let status = response.status();
    let (parts, body) = response.into_parts();
    let mut ok = status.is_success();
    let mut message = String::new();
    let response = if ok
        && is_json(&parts.headers)
        && content_length(&parts.headers).is_some_and(|n| n <= INSPECT_LIMIT)
    {
        match axum::body::to_bytes(body, INSPECT_LIMIT).await {
            Ok(bytes) => {
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                    if json.get("success").and_then(|v| v.as_bool()) == Some(false) {
                        ok = false;
                    }
                    message = json
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                }
                Response::from_parts(parts, Body::from(bytes))
            }
            Err(_) => Response::from_parts(parts, Body::empty()),
        }
    } else {
        if !ok {
            message = status.to_string();
        }
        Response::from_parts(parts, body)
    };

    if let Some(state) = state {
        let method = method.to_string();
        tokio::spawn(async move {
            record(
                &state.db, &username, &ip, &method, &path, &target, ok, &message,
            )
            .await;
        });
    }
    response
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct Query {
    #[serde(default)]
    pub q: String,
    #[serde(default)]
    pub failed_only: bool,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub limit: i64,
}

#[derive(Serialize)]
pub struct Page {
    pub items: Vec<Entry>,
    pub total: i64,
}

pub async fn list(pool: &SqlitePool, q: &Query) -> Result<Page, String> {
    let like = format!("%{}%", q.q.trim().replace('%', "\\%").replace('_', "\\_"));
    let limit = q.limit.clamp(1, 200);
    let filter = "(?1 = '%%' OR username LIKE ?1 ESCAPE '\\' OR ip LIKE ?1 ESCAPE '\\' OR path LIKE ?1 ESCAPE '\\' OR target LIKE ?1 ESCAPE '\\') AND (?2 = 0 OR ok = 0)";
    let total: (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM audit_log WHERE {}", filter))
        .bind(&like)
        .bind(q.failed_only)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
    let items = sqlx::query_as::<_, Entry>(&format!(
        "SELECT * FROM audit_log WHERE {} ORDER BY id DESC LIMIT ?3 OFFSET ?4",
        filter
    ))
    .bind(&like)
    .bind(q.failed_only)
    .bind(limit)
    .bind(q.offset.max(0))
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(Page {
        items,
        total: total.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_leave_out_secrets() {
        let json = serde_json::json!({
            "path": "/www/wwwroot/a",
            "password": "hunter2",
            "content": "file body",
            "paths": ["/a", "/b", "/c", "/d"],
            "permanent": true
        });
        let text = describe(&json);
        assert!(text.contains("path=/www/wwwroot/a"));
        assert!(text.contains("paths=/a, /b, /c (+1)"));
        assert!(text.contains("permanent=true"));
        assert!(!text.contains("hunter2") && !text.contains("file body"));
        assert_eq!(describe(&serde_json::json!([1, 2])), "");
    }

    #[test]
    fn plumbing_is_not_recorded() {
        assert!(ignored("/files/upload/chunk"));
        assert!(ignored("/tasks/abc"));
        assert!(!ignored("/files/delete"));
    }
}
