use axum::{extract::Request, http::StatusCode, middleware::Next, response::Response};
use jsonwebtoken::{decode, DecodingKey, Validation};

use crate::models::Claims;

pub async fn auth(mut req: Request, next: Next) -> Result<Response, StatusCode> {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = if let Some(t) = auth_header.strip_prefix("Bearer ") {
        t.to_string()
    } else if req.uri().path().ends_with("/files/terminal/ws") {
        req.uri()
            .query()
            .and_then(extract_token_from_query)
            .ok_or(StatusCode::UNAUTHORIZED)?
            .to_string()
    } else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    let state = req
        .extensions()
        .get::<std::sync::Arc<crate::AppState>>()
        .cloned()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let claims = decode::<Claims>(
        &token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| StatusCode::UNAUTHORIZED)?
    .claims;

    req.extensions_mut().insert(claims);
    Ok(next.run(req).await)
}

fn extract_token_from_query(query: &str) -> Option<&str> {
    query.split('&').find_map(|pair| {
        let mut parts = pair.splitn(2, '=');
        match (parts.next(), parts.next()) {
            (Some("token"), Some(value)) if !value.is_empty() => Some(value),
            _ => None,
        }
    })
}
