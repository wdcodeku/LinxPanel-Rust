use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, DecodingKey, Validation};

use crate::models::Claims;

pub async fn auth(mut req: Request, next: Next) -> Result<Response, StatusCode> {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = if let Some(t) = auth_header.strip_prefix("Bearer ") {
        t
    } else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    let state = req
        .extensions()
        .get::<std::sync::Arc<crate::AppState>>()
        .cloned()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let claims = decode::<Claims>(
        token,
        &DecodingKey::from_secret(state.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| StatusCode::UNAUTHORIZED)?
    .claims;

    req.extensions_mut().insert(claims);
    Ok(next.run(req).await)
}