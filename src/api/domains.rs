use axum::{
    extract::{Path, State},
    routing::{delete, get},
    Json, Router,
};
use std::sync::Arc;

use crate::models::*;
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/sites/{site_id}/domains", get(list_domains).post(add_domain))
        .route("/sites/{site_id}/domains/{id}", delete(remove_domain))
}

async fn list_domains(
    State(state): State<Arc<AppState>>,
    Path(site_id): Path<i64>,
) -> Json<ApiResponse<Vec<Domain>>> {
    match sqlx::query_as::<_, Domain>("SELECT * FROM domains WHERE site_id = ? ORDER BY is_primary DESC, id ASC")
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
    let is_primary = req.is_primary.unwrap_or(false);

    // If setting as primary, unset existing primary
    if is_primary {
        let _ = sqlx::query("UPDATE domains SET is_primary = 0 WHERE site_id = ?")
            .bind(site_id)
            .execute(&state.db)
            .await;
    }

    let result = sqlx::query("INSERT INTO domains (site_id, domain, is_primary) VALUES (?, ?, ?)")
        .bind(site_id)
        .bind(&req.domain)
        .bind(is_primary)
        .execute(&state.db)
        .await;

    match result {
        Ok(r) => {
            // Update sites.domains field
            update_site_domains(&state.db, site_id).await;

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

async fn remove_domain(
    State(state): State<Arc<AppState>>,
    Path((site_id, id)): Path<(i64, i64)>,
) -> Json<ApiResponse<()>> {
    match sqlx::query("DELETE FROM domains WHERE id = ? AND site_id = ?")
        .bind(id)
        .bind(site_id)
        .execute(&state.db)
        .await
    {
        Ok(_) => {
            update_site_domains(&state.db, site_id).await;
            Json(ApiResponse::ok_msg("Domain removed"))
        }
        Err(e) => Json(ApiResponse::err(&e.to_string())),
    }
}

async fn update_site_domains(db: &sqlx::SqlitePool, site_id: i64) {
    if let Ok(domains) =
        sqlx::query_as::<_, Domain>("SELECT * FROM domains WHERE site_id = ?")
            .bind(site_id)
            .fetch_all(db)
            .await
    {
        let domain_str: Vec<String> = domains.iter().map(|d| d.domain.clone()).collect();
        let _ = sqlx::query("UPDATE sites SET domains = ?, updated_at = datetime('now') WHERE id = ?")
            .bind(domain_str.join(","))
            .bind(site_id)
            .execute(db)
            .await;
    }
}