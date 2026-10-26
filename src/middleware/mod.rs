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
use crate::services::panel_access::{self, DenyReason, EntranceCookie};
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

/// Gatekeeper in front of the whole panel, UI and API alike. In order it enforces
/// the allowed-IP list, the bound domain and the security entrance; visiting the
/// entrance sets a signed cookie and redirects to login.
pub async fn access_guard(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let policy = state.access();
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(peer)| peer.ip());
    let path = req.uri().path();

    // The installer's local health probe; remote callers must not learn a panel is here.
    if path == "/api/health" && peer.is_some_and(|ip| ip.is_loopback()) {
        return next.run(req).await;
    }

    if !peer.is_some_and(|ip| policy.ip_allowed(ip)) {
        tracing::debug!("Rejected {:?}: not in allowed IPs", peer);
        return deny(&policy, DenyReason::Ip);
    }

    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    if !policy.host_allowed(host) {
        return deny(&policy, DenyReason::Domain);
    }

    let Some(entrance) = policy.entrance.as_deref() else {
        return next.run(req).await;
    };
    if path == entrance || path.strip_suffix('/') == Some(entrance) {
        let cookie =
            panel_access::entrance_set_cookie(entrance, &state.jwt_secret, state.tls_active);
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
    match panel_access::check_entrance_cookie(cookie_header, entrance, &state.jwt_secret) {
        EntranceCookie::Valid { refresh } => {
            let cookie = refresh.then(|| {
                panel_access::entrance_set_cookie(entrance, &state.jwt_secret, state.tls_active)
            });
            let mut response = next.run(req).await;
            if let Some(value) = cookie.and_then(|c| HeaderValue::from_str(&c).ok()) {
                response.headers_mut().append(header::SET_COOKIE, value);
            }
            response
        }
        EntranceCookie::Missing => deny(&policy, DenyReason::Entrance),
    }
}

fn deny(policy: &panel_access::AccessPolicy, reason: DenyReason) -> Response {
    let (status, body) = panel_access::deny_page(policy.deny, reason);
    Response::builder()
        .status(StatusCode::from_u16(status).unwrap_or(StatusCode::NOT_FOUND))
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-store")
        .body(Body::from(body))
        .unwrap_or_default()
}
