//! Realtime metrics endpoints: a WebSocket push stream plus REST fallbacks.

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;

use crate::models::*;
use crate::AppState;

/// Ping cadence; also bounds how long a dead peer stays subscribed.
const PING_INTERVAL: Duration = Duration::from_secs(30);

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/realtime/ws", get(realtime_ws))
        .route("/realtime/snapshot", get(realtime_snapshot))
}

async fn realtime_snapshot(State(state): State<Arc<AppState>>) -> Json<ApiResponse<MetricsSnapshot>> {
    Json(ApiResponse::ok(state.metrics.snapshot()))
}

async fn realtime_ws(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    // Backfill first so the client can paint a full chart immediately.
    let snapshot = state.metrics.snapshot();
    let hello = serde_json::json!({ "type": "snapshot", "data": snapshot });
    if socket
        .send(Message::Text(hello.to_string().into()))
        .await
        .is_err()
    {
        return;
    }

    let mut rx = state.metrics.subscribe();
    let mut shutdown = state.shutdown.subscribe();
    let mut ping = tokio::time::interval(PING_INTERVAL);
    ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            frame = rx.recv() => match frame {
                Ok(frame) => {
                    let Ok(payload) = serde_json::to_string(&frame) else { continue };
                    if socket.send(Message::Text(payload.into())).await.is_err() {
                        break;
                    }
                }
                // A lagging client skips ahead rather than being disconnected.
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            },
            _ = ping.tick() => {
                if socket.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            _ = shutdown.changed() => {
                let _ = socket.send(Message::Close(None)).await;
                break;
            }
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Close(_))) | None => break,
                Some(Err(_)) => break,
                // Clients have nothing to say; pongs and stray frames are ignored.
                Some(Ok(_)) => {}
            }
        }
    }
}
