use axum::{
    body::Body,
    extract::{ConnectInfo, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use std::net::SocketAddr;
use std::sync::Arc;

use crate::models::Claims;
use crate::services::panel_access::{self, EntranceCookie};
use crate::AppState;

pub async fn auth(mut req: Request, next: Next) -> Result<Response, StatusCode> {
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let token = if let Some(t) = auth_header.strip_prefix("Bearer ") {
        t.to_string()
    } else if is_websocket_route(req.uri().path()) {
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

    // Reject tokens issued before the user's last password change (or for a
    // deleted user).
    match crate::db::session_version(&state.db, claims.sub).await {
        Ok(Some(version)) if version == claims.ver => {}
        Ok(_) => return Err(StatusCode::UNAUTHORIZED),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    }

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

/// Browsers cannot set headers on a WebSocket handshake, so these routes accept `?token=`.
fn is_websocket_route(path: &str) -> bool {
    path.ends_with("/files/terminal/ws") || path.ends_with("/realtime/ws")
}

/// Hides the whole panel, UI and API alike, until the browser has visited the
/// security entrance. Visiting it sets a signed cookie and redirects to login.
pub async fn entrance_guard(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let Some(entrance) = state.entrance() else {
        return next.run(req).await;
    };

    let path = req.uri().path();
    // The installer's local health probe; remote callers must not learn a panel is here.
    let from_loopback = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .is_some_and(|ConnectInfo(peer)| peer.ip().is_loopback());
    if path == "/api/health" && from_loopback {
        return next.run(req).await;
    }
    if path == entrance || path.strip_suffix('/') == Some(entrance.as_str()) {
        let cookie =
            panel_access::entrance_set_cookie(&entrance, &state.jwt_secret, state.tls_active);
        return Response::builder()
            .status(StatusCode::FOUND)
            .header(header::LOCATION, "/login")
            .header(header::SET_COOKIE, cookie)
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::empty())
            .unwrap_or_default();
    }

    let cookie_header = req
        .headers()
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok());
    match panel_access::check_entrance_cookie(cookie_header, &entrance, &state.jwt_secret) {
        EntranceCookie::Valid { refresh } => {
            let mut response = next.run(req).await;
            if refresh {
                let cookie = panel_access::entrance_set_cookie(
                    &entrance,
                    &state.jwt_secret,
                    state.tls_active,
                );
                if let Ok(value) = HeaderValue::from_str(&cookie) {
                    response.headers_mut().append(header::SET_COOKIE, value);
                }
            }
            response
        }
        EntranceCookie::Missing => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(panel_access::ENTRANCE_DENIED_HTML))
            .unwrap_or_default(),
    }
}
