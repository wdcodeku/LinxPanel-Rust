use axum::{
    extract::{Path, State},
    routing::{get, put},
    Json, Router,
};
use std::{collections::HashSet, sync::Arc};

use crate::models::*;
use crate::services::openresty;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/sites/{site_id}/domains", get(list_domains).post(add_domain))
        .route("/sites/{site_id}/domains/reorder", put(reorder_domains).post(reorder_domains))
        .route("/sites/{site_id}/domains/{id}", put(update_domain).delete(remove_domain))
}

async fn list_domains(
    State(state): State<Arc<AppState>>,
    Path(site_id): Path<i64>,
) -> Json<ApiResponse<Vec<Domain>>> {
    match sqlx::query_as::<_, Domain>("SELECT * FROM domains WHERE site_id = ? ORDER BY sort_order ASC, id ASC")
        .bind(site_id)
        .fetch_all(&state.db)
        .await
    {
        Ok(domains) => Json(ApiResponse::ok(domains)),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn add_domain(
    State(state): State<Arc<AppState>>,
    Path(site_id): Path<i64>,
    Json(req): Json<AddDomainRequest>,
) -> Json<ApiResponse<Domain>> {
    if let Err(e) = openresty::ensure_management_available(state.as_ref()).await {
        return Json(ApiResponse::err(&e.to_string()));
    }

    let domain = match normalize_domain(&req.domain) {
        Some(value) => value,
        None => return Json(ApiResponse::err("Invalid domain format")),
    };

    let duplicate = match has_duplicate_domain(&state.db, site_id, &domain, None).await {
        Ok(value) => value,
        Err(e) => return Json(ApiResponse::err(&e.to_string())),
    };
    if duplicate {
        return Json(ApiResponse::err("Domain already exists"));
    }

    let existing_count: i64 =
        match sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM domains WHERE site_id = ?")
            .bind(site_id)
            .fetch_one(&state.db)
            .await
        {
            Ok(value) => value,
            Err(e) => return Json(ApiResponse::err(&e.to_string())),
        };
    let sort_order: i64 = match sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM domains WHERE site_id = ?",
    )
    .bind(site_id)
    .fetch_one(&state.db)
    .await
    {
        Ok(value) => value,
        Err(e) => return Json(ApiResponse::err(&e.to_string())),
    };
    let is_primary = req.is_primary.unwrap_or(false) || existing_count == 0;

    // If setting as primary, unset existing primary
    if is_primary {
        let _ = sqlx::query("UPDATE domains SET is_primary = 0 WHERE site_id = ?")
            .bind(site_id)
            .execute(&state.db)
            .await;
    }

    let result =
        sqlx::query("INSERT INTO domains (site_id, domain, is_primary, sort_order) VALUES (?, ?, ?, ?)")
        .bind(site_id)
        .bind(&domain)
        .bind(is_primary)
        .bind(sort_order)
        .execute(&state.db)
        .await;

    match result {
        Ok(r) => {
            if let Err(e) = sync_site_domains(&state.db, site_id).await {
                return Json(ApiResponse::err(&e.to_string()));
            }
            if let Err(e) = openresty::sync_site(state.as_ref(), site_id).await {
                return Json(ApiResponse::err(&format!("OpenResty apply failed: {}", e)));
            }

            match sqlx::query_as::<_, Domain>("SELECT * FROM domains WHERE id = ?")
                .bind(r.last_insert_rowid())
                .fetch_one(&state.db)
                .await
            {
                Ok(d) => Json(ApiResponse::ok(d)),
                Err(e) => Json(ApiResponse::err(&e.to_string())),
            }
        }
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn update_domain(
    State(state): State<Arc<AppState>>,
    Path((site_id, id)): Path<(i64, i64)>,
    Json(req): Json<UpdateDomainRequest>,
) -> Json<ApiResponse<Domain>> {
    if let Err(e) = openresty::ensure_management_available(state.as_ref()).await {
        return Json(ApiResponse::err(&e.to_string()));
    }

    let domain = match normalize_domain(&req.domain) {
        Some(value) => value,
        None => return Json(ApiResponse::err("Invalid domain format")),
    };

    let duplicate = match has_duplicate_domain(&state.db, site_id, &domain, Some(id)).await {
        Ok(value) => value,
        Err(e) => return Json(ApiResponse::err(&e.to_string())),
    };
    if duplicate {
        return Json(ApiResponse::err("Domain already exists"));
    }

    match sqlx::query("UPDATE domains SET domain = ? WHERE id = ? AND site_id = ?")
        .bind(&domain)
        .bind(id)
        .bind(site_id)
        .execute(&state.db)
        .await
    {
        Ok(result) => {
            if result.rows_affected() == 0 {
                return Json(ApiResponse::err("Domain not found"));
            }
            if let Err(e) = sync_site_domains(&state.db, site_id).await {
                return Json(ApiResponse::err(&e.to_string()));
            }
            if let Err(e) = openresty::sync_site(state.as_ref(), site_id).await {
                return Json(ApiResponse::err(&format!("OpenResty apply failed: {}", e)));
            }
            match sqlx::query_as::<_, Domain>("SELECT * FROM domains WHERE id = ?")
                .bind(id)
                .fetch_one(&state.db)
                .await
            {
                Ok(domain) => Json(ApiResponse::ok(domain)),
                Err(e) => Json(ApiResponse::err(&e.to_string())),
            }
        }
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn reorder_domains(
    State(state): State<Arc<AppState>>,
    Path(site_id): Path<i64>,
    Json(req): Json<ReorderDomainsRequest>,
) -> Json<ApiResponse<Vec<Domain>>> {
    if let Err(e) = openresty::ensure_management_available(state.as_ref()).await {
        return Json(ApiResponse::err(&e.to_string()));
    }

    if req.ordered_ids.is_empty() {
        return Json(ApiResponse::err("Domain order is invalid"));
    }

    let existing_ids: Vec<i64> =
        match sqlx::query_scalar::<_, i64>("SELECT id FROM domains WHERE site_id = ? ORDER BY sort_order ASC, id ASC")
            .bind(site_id)
            .fetch_all(&state.db)
            .await
        {
            Ok(ids) => ids,
            Err(e) => return Json(ApiResponse::err(&e.to_string())),
        };

    let ordered_ids = req.ordered_ids;
    if ordered_ids.len() != existing_ids.len() {
        return Json(ApiResponse::err("Domain order is invalid"));
    }

    let expected: HashSet<i64> = existing_ids.into_iter().collect();
    let received: HashSet<i64> = ordered_ids.iter().copied().collect();
    if expected != received {
        return Json(ApiResponse::err("Domain order is invalid"));
    }

    let mut tx = match state.db.begin().await {
        Ok(tx) => tx,
        Err(e) => return Json(ApiResponse::err(&e.to_string())),
    };
    for (index, domain_id) in ordered_ids.iter().enumerate() {
        if let Err(e) = sqlx::query("UPDATE domains SET sort_order = ? WHERE id = ? AND site_id = ?")
            .bind(index as i64)
            .bind(domain_id)
            .bind(site_id)
            .execute(&mut *tx)
            .await
        {
            return Json(ApiResponse::err(&e.to_string()));
        }
    }
    if let Err(e) = tx.commit().await {
        return Json(ApiResponse::err(&e.to_string()));
    }

    if let Err(e) = sync_site_domains(&state.db, site_id).await {
        return Json(ApiResponse::err(&e.to_string()));
    }
    if let Err(e) = openresty::sync_site(state.as_ref(), site_id).await {
        return Json(ApiResponse::err(&format!("OpenResty apply failed: {}", e)));
    }

    match sqlx::query_as::<_, Domain>("SELECT * FROM domains WHERE site_id = ? ORDER BY sort_order ASC, id ASC")
        .bind(site_id)
        .fetch_all(&state.db)
        .await
    {
        Ok(domains) => Json(ApiResponse::ok(domains)),
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn remove_domain(
    State(state): State<Arc<AppState>>,
    Path((site_id, id)): Path<(i64, i64)>,
) -> Json<ApiResponse<()>> {
    if let Err(e) = openresty::ensure_management_available(state.as_ref()).await {
        return Json(ApiResponse::err(&e.to_string()));
    }

    match sqlx::query("DELETE FROM domains WHERE id = ? AND site_id = ?")
        .bind(id)
        .bind(site_id)
        .execute(&state.db)
        .await
    {
        Ok(result) => {
            if result.rows_affected() == 0 {
                return Json(ApiResponse::err("Domain not found"));
            }
            if let Err(e) = sync_site_domains(&state.db, site_id).await {
                return Json(ApiResponse::err(&e.to_string()));
            }
            if let Err(e) = openresty::sync_site(state.as_ref(), site_id).await {
                return Json(ApiResponse::err(&format!("OpenResty apply failed: {}", e)));
            }
            Json(ApiResponse::ok_msg("Domain removed"))
        }
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn sync_site_domains(db: &sqlx::SqlitePool, site_id: i64) -> Result<(), sqlx::Error> {
    let domains = sqlx::query_as::<_, Domain>(
        "SELECT * FROM domains WHERE site_id = ? ORDER BY sort_order ASC, id ASC",
    )
    .bind(site_id)
    .fetch_all(db)
    .await?;

    let mut domain_str: Vec<String> = Vec::with_capacity(domains.len());
    for (index, domain) in domains.iter().enumerate() {
        domain_str.push(domain.domain.clone());
        let expected_order = index as i64;
        let expected_primary = index == 0;
        if domain.sort_order != expected_order || domain.is_primary != expected_primary {
            sqlx::query("UPDATE domains SET sort_order = ?, is_primary = ? WHERE id = ?")
                .bind(expected_order)
                .bind(expected_primary)
                .bind(domain.id)
                .execute(db)
                .await?;
        }
    }

    sqlx::query("UPDATE sites SET domains = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(domain_str.join(","))
        .bind(site_id)
        .execute(db)
        .await?;

    Ok(())
}

async fn has_duplicate_domain(
    db: &sqlx::SqlitePool,
    site_id: i64,
    domain: &str,
    exclude_id: Option<i64>,
) -> Result<bool, sqlx::Error> {
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, domain FROM domains WHERE site_id = ?")
            .bind(site_id)
            .fetch_all(db)
            .await?;

    for (id, existing) in rows {
        if Some(id) == exclude_id {
            continue;
        }
        if existing.eq_ignore_ascii_case(domain) {
            return Ok(true);
        }
    }
    Ok(false)
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
