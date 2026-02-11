use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use std::collections::HashSet;
use std::sync::Arc;

use crate::models::*;
use crate::services::openresty;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/sites", get(list_sites).post(create_site))
        .route(
            "/sites/{id}",
            get(get_site).put(update_site).delete(delete_site),
        )
        .route("/sites/{id}/start", post(start_site))
        .route("/sites/{id}/stop", post(stop_site))
}

async fn list_sites(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<Site>>> {
    match sqlx::query_as::<_, Site>("SELECT * FROM sites ORDER BY id DESC")
        .fetch_all(&state.db)
        .await
    {
        Ok(sites) => Json(ApiResponse::ok(sites)),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn get_site(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<Site>> {
    match sqlx::query_as::<_, Site>("SELECT * FROM sites WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
    {
        Ok(Some(site)) => Json(ApiResponse::ok(site)),
        Ok(None) => Json(ApiResponse::err("Site not found")),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn create_site(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateSiteRequest>,
) -> Json<ApiResponse<Site>> {
    if let Err(e) = openresty::ensure_management_available(state.as_ref()).await {
        return Json(ApiResponse::err(&e.to_string()));
    }

    let CreateSiteRequest {
        name,
        domains,
        doc_root,
        site_type,
        proxy_pass,
        index_files,
        stopped_page_html,
    } = req;

    let name = name.trim().to_string();
    if name.is_empty() {
        return Json(ApiResponse::err("Site name is required"));
    }

    let mut normalized_domains: Vec<String> = Vec::new();
    let mut seen_domains: HashSet<String> = HashSet::new();
    for raw_domain in domains {
        let Some(domain) = normalize_domain(raw_domain.as_str()) else {
            return Json(ApiResponse::err("Invalid domain format"));
        };
        if !seen_domains.insert(domain.clone()) {
            return Json(ApiResponse::err(&format!(
                "Domain already exists: {}",
                domain
            )));
        }
        normalized_domains.push(domain);
    }
    let domains: Vec<String> = normalized_domains;
    if domains.is_empty() {
        return Json(ApiResponse::err("At least one domain is required"));
    }

    let doc_root = doc_root
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/www/wwwroot".to_string());
    if let Err(e) = std::fs::create_dir_all(&doc_root) {
        return Json(ApiResponse::err(&format!(
            "Failed to create site directory: {}",
            e
        )));
    }

    let site_type = site_type
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "static".into());
    let index_files = normalize_index_files(index_files, &site_type);
    let stopped_page_html = normalize_stopped_page_html(stopped_page_html);

    for domain in &domains {
        match find_conflicting_domain(&state.db, domain).await {
            Ok(Some(conflict_domain)) => {
                return Json(ApiResponse::err(&format!(
                    "Domain already exists: {}",
                    conflict_domain
                )))
            }
            Ok(None) => {}
            Err(e) => return Json(ApiResponse::err(&e.to_string())),
        }
    }

    if site_type != "proxy" {
        let index_path = std::path::Path::new(&doc_root).join("index.html");
        if !index_path.exists() {
            let index_content = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Hello World</title></head><body><h1>Hello World</h1></body></html>";
            let _ = std::fs::write(&index_path, index_content);
        }
    }
    let domains_str = domains.join(",");

    let result = sqlx::query(
        "INSERT INTO sites (name, domains, doc_root, site_type, proxy_pass, index_files, stopped_page_html) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&name)
    .bind(&domains_str)
    .bind(&doc_root)
    .bind(&site_type)
    .bind(&proxy_pass)
    .bind(&index_files)
    .bind(&stopped_page_html)
    .execute(&state.db)
    .await;

    match result {
        Ok(r) => {
            let site_id = r.last_insert_rowid();
            for (i, domain) in domains.iter().enumerate() {
                let _ = sqlx::query(
                    "INSERT INTO domains (site_id, domain, is_primary, sort_order) VALUES (?, ?, ?, ?)",
                )
                .bind(site_id)
                .bind(domain)
                .bind(i == 0)
                .bind(i as i64)
                .execute(&state.db)
                .await;
            }

            if let Err(e) = openresty::sync_site(state.as_ref(), site_id).await {
                let _ = sqlx::query("DELETE FROM domains WHERE site_id = ?")
                    .bind(site_id)
                    .execute(&state.db)
                    .await;
                let _ = sqlx::query("DELETE FROM sites WHERE id = ?")
                    .bind(site_id)
                    .execute(&state.db)
                    .await;
                return Json(ApiResponse::err(&format!("OpenResty apply failed: {}", e)));
            }

            match sqlx::query_as::<_, Site>("SELECT * FROM sites WHERE id = ?")
                .bind(site_id)
                .fetch_one(&state.db)
                .await
            {
                Ok(site) => Json(ApiResponse::ok(site)),
                Err(e) => Json(ApiResponse::err(&e.to_string())),
            }
        }
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn update_site(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateSiteRequest>,
) -> Json<ApiResponse<Site>> {
    if let Err(e) = openresty::ensure_management_available(state.as_ref()).await {
        return Json(ApiResponse::err(&e.to_string()));
    }

    let existing = sqlx::query_as::<_, Site>("SELECT * FROM sites WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await;

    let existing = match existing {
        Ok(Some(s)) => s,
        Ok(None) => return Json(ApiResponse::err("Site not found")),
        Err(e) => return Json(ApiResponse::err(&e.to_string())),
    };

    let name = req.name.unwrap_or(existing.name);
    let domains = req.domains.map(|d| d.join(",")).unwrap_or(existing.domains);
    let doc_root = req.doc_root.unwrap_or(existing.doc_root);
    let site_type = req.site_type.unwrap_or(existing.site_type);
    let proxy_pass = req.proxy_pass.or(existing.proxy_pass);
    let index_files = req
        .index_files
        .map(|value| normalize_index_files(Some(value), &site_type))
        .unwrap_or_else(|| normalize_index_files(Some(existing.index_files), &site_type));
    let stopped_page_html = match req.stopped_page_html {
        Some(value) => normalize_stopped_page_html(Some(value)),
        None => existing.stopped_page_html,
    };
    let extra_config = req.extra_config.or(existing.extra_config);

    let result = sqlx::query(
        "UPDATE sites SET name=?, domains=?, doc_root=?, site_type=?, proxy_pass=?, index_files=?, stopped_page_html=?, extra_config=?, updated_at=datetime('now') WHERE id=?",
    )
    .bind(&name)
    .bind(&domains)
    .bind(&doc_root)
    .bind(&site_type)
    .bind(&proxy_pass)
    .bind(&index_files)
    .bind(&stopped_page_html)
    .bind(&extra_config)
    .bind(id)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => {
            if let Err(e) = openresty::sync_site(state.as_ref(), id).await {
                return Json(ApiResponse::err(&format!("OpenResty apply failed: {}", e)));
            }

            match sqlx::query_as::<_, Site>("SELECT * FROM sites WHERE id = ?")
                .bind(id)
                .fetch_one(&state.db)
                .await
            {
                Ok(site) => Json(ApiResponse::ok(site)),
                Err(e) => Json(ApiResponse::err(&e.to_string())),
            }
        }
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

fn default_index_files(site_type: &str) -> &'static str {
    match site_type {
        "php" => "index.php index.html index.htm",
        "proxy" => "index.html index.htm",
        _ => "index.html index.htm index.php",
    }
}

fn is_safe_index_file(token: &str) -> bool {
    if token.is_empty() || token.contains("..") || token.starts_with('/') {
        return false;
    }
    token
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' || ch == '/')
}

fn normalize_index_files(raw: Option<String>, site_type: &str) -> String {
    let source = raw.unwrap_or_default();
    let mut cleaned: Vec<String> = Vec::new();
    for token in source
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        if !is_safe_index_file(token) {
            continue;
        }
        let lowered = token.to_ascii_lowercase();
        if !cleaned
            .iter()
            .any(|item| item.eq_ignore_ascii_case(&lowered))
        {
            cleaned.push(token.to_string());
        }
    }
    if cleaned.is_empty() {
        default_index_files(site_type).to_string()
    } else {
        cleaned.join(" ")
    }
}

fn normalize_stopped_page_html(raw: Option<String>) -> Option<String> {
    raw.map(|value| value.trim().to_string()).and_then(|value| {
        if value.is_empty() {
            None
        } else {
            Some(value)
        }
    })
}

async fn find_conflicting_domain(
    db: &sqlx::SqlitePool,
    domain: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar::<_, String>(
        "SELECT domain FROM domains WHERE lower(domain) = lower(?) LIMIT 1",
    )
    .bind(domain)
    .fetch_optional(db)
    .await
}

fn normalize_domain(raw: &str) -> Option<String> {
    let mut value = raw.trim().to_string();
    if value.is_empty() {
        return None;
    }

    if let Some((_, rest)) = value.split_once("://") {
        value = rest.to_string();
    }
    if let Some((host, _)) = value.split_once('/') {
        value = host.to_string();
    }

    let value = value.trim().trim_end_matches('.');
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return None;
    }

    let (host, port) = split_host_port(value)?;
    let host_lower = host.to_ascii_lowercase();
    if !is_valid_host(&host_lower) {
        return None;
    }

    Some(match port {
        Some(port) => format!("{}:{}", host_lower, port),
        None => host_lower,
    })
}

fn split_host_port(value: &str) -> Option<(&str, Option<u16>)> {
    if let Some((host, port_part)) = value.rsplit_once(':') {
        if !port_part.is_empty() && port_part.chars().all(|c| c.is_ascii_digit()) {
            let port = port_part.parse::<u16>().ok()?;
            if port == 0 || host.is_empty() {
                return None;
            }
            return Some((host, Some(port)));
        }
    }
    Some((value, None))
}

fn is_valid_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }

    let trimmed = host.strip_prefix("*.").unwrap_or(host);
    if trimmed.is_empty() || trimmed.len() > 253 {
        return false;
    }

    if trimmed.parse::<std::net::Ipv4Addr>().is_ok() {
        return true;
    }

    trimmed.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

async fn delete_site(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<()>> {
    if let Err(e) = openresty::ensure_management_available(state.as_ref()).await {
        return Json(ApiResponse::err(&e.to_string()));
    }

    let _ = sqlx::query("DELETE FROM domains WHERE site_id = ?")
        .bind(id)
        .execute(&state.db)
        .await;

    match sqlx::query("DELETE FROM sites WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await
    {
        Ok(_) => {
            if let Err(e) = openresty::remove_site(state.as_ref(), id).await {
                return Json(ApiResponse::err(&format!("OpenResty apply failed: {}", e)));
            }
            Json(ApiResponse::ok_msg("Site deleted"))
        }
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn start_site(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<()>> {
    if let Err(e) = openresty::ensure_management_available(state.as_ref()).await {
        return Json(ApiResponse::err(&e.to_string()));
    }

    match sqlx::query("UPDATE sites SET status='running', updated_at=datetime('now') WHERE id=?")
        .bind(id)
        .execute(&state.db)
        .await
    {
        Ok(_) => {
            if let Err(e) = openresty::sync_site(state.as_ref(), id).await {
                return Json(ApiResponse::err(&format!("OpenResty apply failed: {}", e)));
            }
            Json(ApiResponse::ok_msg("Site started"))
        }
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn stop_site(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<()>> {
    if let Err(e) = openresty::ensure_management_available(state.as_ref()).await {
        return Json(ApiResponse::err(&e.to_string()));
    }

    match sqlx::query("UPDATE sites SET status='stopped', updated_at=datetime('now') WHERE id=?")
        .bind(id)
        .execute(&state.db)
        .await
    {
        Ok(_) => {
            if let Err(e) = openresty::sync_site(state.as_ref(), id).await {
                return Json(ApiResponse::err(&format!("OpenResty apply failed: {}", e)));
            }
            Json(ApiResponse::ok_msg("Site stopped"))
        }
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}
