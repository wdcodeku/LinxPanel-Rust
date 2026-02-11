use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use std::sync::Arc;

use crate::models::*;
use crate::services::openresty;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/sites", get(list_sites).post(create_site))
        .route("/sites/{id}", get(get_site).put(update_site).delete(delete_site))
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
    } = req;

    let name = name.trim().to_string();
    if name.is_empty() {
        return Json(ApiResponse::err("Site name is required"));
    }

    let domains: Vec<String> = domains
        .into_iter()
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .collect();
    if domains.is_empty() {
        return Json(ApiResponse::err("At least one domain is required"));
    }

    let doc_root = doc_root
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| format!("/www/sites/{}", name));
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
    if site_type != "proxy" {
        let index_path = std::path::Path::new(&doc_root).join("index.html");
        if !index_path.exists() {
            let index_content = format!(
                "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title></head><body><h1>{}</h1><p>LinxPanel site is ready.</p></body></html>",
                name, name
            );
            let _ = std::fs::write(&index_path, index_content);
        }
    }
    let domains_str = domains.join(",");

    let result = sqlx::query(
        "INSERT INTO sites (name, domains, doc_root, site_type, proxy_pass) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&name)
    .bind(&domains_str)
    .bind(&doc_root)
    .bind(&site_type)
    .bind(&proxy_pass)
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
    let domains = req
        .domains
        .map(|d| d.join(","))
        .unwrap_or(existing.domains);
    let doc_root = req.doc_root.unwrap_or(existing.doc_root);
    let site_type = req.site_type.unwrap_or(existing.site_type);
    let proxy_pass = req.proxy_pass.or(existing.proxy_pass);
    let extra_config = req.extra_config.or(existing.extra_config);

    let result = sqlx::query(
        "UPDATE sites SET name=?, domains=?, doc_root=?, site_type=?, proxy_pass=?, extra_config=?, updated_at=datetime('now') WHERE id=?",
    )
    .bind(&name)
    .bind(&domains)
    .bind(&doc_root)
    .bind(&site_type)
    .bind(&proxy_pass)
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
