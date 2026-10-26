//! The panel's own HTTP(S) server loop, built to stay up under attack.
//!
//! `axum::serve` sets no header-read timeout and no connection limits, so a
//! slowloris (connections that trickle headers forever) or a connection flood
//! exhausts the panel. Here:
//! - connections over the global cap, or over the per-IP cap, are closed at TCP
//!   accept - before any TLS handshake or HTTP parsing is spent on them;
//! - request headers must arrive within `HEADER_READ_TIMEOUT`;
//! - TLS handshakes (and plain-HTTP-to-HTTPS redirects) have their own deadline.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::extract::ConnectInfo;
use axum::Router;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::rustls::ServerConfig;
use tower::ServiceExt;

const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_connections: usize,
    pub max_per_ip: usize,
}

impl Limits {
    /// Defaults sized for an admin panel; override with LINXPANEL_MAX_CONNECTIONS
    /// and LINXPANEL_MAX_CONN_PER_IP.
    pub fn from_env() -> Self {
        let read = |key: &str, default: usize| {
            std::env::var(key)
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .filter(|v: &usize| *v > 0)
                .unwrap_or(default)
        };
        Self {
            max_connections: read("LINXPANEL_MAX_CONNECTIONS", 1024),
            max_per_ip: read("LINXPANEL_MAX_CONN_PER_IP", 48),
        }
    }
}

/// Counts open connections, globally and per client address.
pub struct Tracker {
    limits: Limits,
    open: Mutex<(usize, HashMap<IpAddr, usize>)>,
}

/// One admitted connection; releases its slot when dropped.
pub struct Permit {
    tracker: Arc<Tracker>,
    ip: IpAddr,
}

impl Tracker {
    pub fn new(limits: Limits) -> Arc<Self> {
        Arc::new(Self {
            limits,
            open: Mutex::new((0, HashMap::new())),
        })
    }

    /// Admits a connection, or None when a cap is reached. Loopback is exempt
    /// from the per-IP cap: behind a local reverse proxy every client is 127.0.0.1.
    pub fn try_admit(self: &Arc<Self>, ip: IpAddr) -> Option<Permit> {
        let ip = ip.to_canonical();
        let mut open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        if open.0 >= self.limits.max_connections {
            return None;
        }
        let per_ip = open.1.get(&ip).copied().unwrap_or(0);
        if per_ip >= self.limits.max_per_ip && !ip.is_loopback() {
            return None;
        }
        open.0 += 1;
        open.1.insert(ip, per_ip + 1);
        Some(Permit {
            tracker: self.clone(),
            ip,
        })
    }

    pub fn open_connections(&self) -> usize {
        self.open.lock().unwrap_or_else(|e| e.into_inner()).0
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        let mut open = self.tracker.open.lock().unwrap_or_else(|e| e.into_inner());
        open.0 = open.0.saturating_sub(1);
        if let Some(count) = open.1.get_mut(&self.ip) {
            *count -= 1;
            if *count == 0 {
                open.1.remove(&self.ip);
            }
        }
    }
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// Serves `app` until `shutdown` turns true, then lets open connections finish
/// their current request.
pub async fn serve(
    listener: TcpListener,
    tls: Option<Arc<ServerConfig>>,
    app: Router,
    tracker: Arc<Tracker>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let local_port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let acceptor = tls.map(tokio_rustls::TlsAcceptor::from);
    let mut builder = auto::Builder::new(TokioExecutor::new());
    builder
        .http1()
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_READ_TIMEOUT)
        .keep_alive(true);
    builder.http2().timer(TokioTimer::new());
    let mut rejected: u64 = 0;

    loop {
        let (stream, peer) = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(accepted) => accepted,
                Err(err) => {
                    // Usually EMFILE under a flood; back off instead of spinning.
                    tracing::warn!("accept failed: {}", err);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            },
            _ = shutdown.changed() => break,
        };

        let Some(permit) = tracker.try_admit(peer.ip()) else {
            rejected += 1;
            // Log sparingly: a flood would otherwise flood the log too.
            if rejected.is_power_of_two() {
                tracing::warn!(
                    "Connection limit reached; dropped {} connections (latest from {})",
                    rejected,
                    peer.ip()
                );
            }
            drop(stream);
            continue;
        };
        let _ = stream.set_nodelay(true);

        let app = app.clone();
        let builder = builder.clone();
        let acceptor = acceptor.clone();
        let mut conn_shutdown = shutdown.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let io: Box<dyn Io> = match acceptor {
                None => Box::new(stream),
                Some(acceptor) => match accept_tls(stream, acceptor, local_port).await {
                    Some(tls) => Box::new(tls),
                    None => return,
                },
            };

            let service = hyper::service::service_fn(move |mut req: hyper::Request<Incoming>| {
                req.extensions_mut().insert(ConnectInfo(peer));
                let app = app.clone();
                async move {
                    let response = app
                        .oneshot(req.map(axum::body::Body::new))
                        .await
                        .unwrap_or_else(|never: Infallible| match never {});
                    Ok::<_, Infallible>(response)
                }
            });

            let conn = builder.serve_connection_with_upgrades(TokioIo::new(io), service);
            tokio::pin!(conn);
            let mut closing = false;
            loop {
                tokio::select! {
                    _ = conn.as_mut() => break,
                    changed = conn_shutdown.changed(), if !closing => {
                        if changed.is_err() || *conn_shutdown.borrow() {
                            conn.as_mut().graceful_shutdown();
                            closing = true;
                        }
                    }
                }
            }
        });
    }
}

/// TLS handshake with a deadline. Plain HTTP on the HTTPS port gets a redirect.
async fn accept_tls(
    stream: TcpStream,
    acceptor: tokio_rustls::TlsAcceptor,
    port: u16,
) -> Option<tokio_rustls::server::TlsStream<TcpStream>> {
    let result = tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        let mut first = [0u8; 1];
        if stream.peek(&mut first).await.ok()? == 0 {
            return None;
        }
        // 0x16 opens every TLS handshake; anything else is plain HTTP.
        if first[0] != 0x16 {
            super::panel_access::redirect_plain_http(stream, port).await;
            return None;
        }
        acceptor.accept(stream).await.ok()
    })
    .await;
    result.ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_global_and_per_ip_connections() {
        let tracker = Tracker::new(Limits {
            max_connections: 4,
            max_per_ip: 2,
        });
        let a: IpAddr = "203.0.113.1".parse().unwrap();
        let b: IpAddr = "203.0.113.2".parse().unwrap();
        let p1 = tracker.try_admit(a).unwrap();
        let _p2 = tracker.try_admit(a).unwrap();
        assert!(tracker.try_admit(a).is_none(), "per-IP cap");
        let _p3 = tracker.try_admit(b).unwrap();
        // v4-mapped v6 counts as the same client.
        assert!(tracker
            .try_admit("::ffff:203.0.113.1".parse().unwrap())
            .is_none());
        drop(p1);
        let _p4 = tracker.try_admit(a).expect("slot released on drop");
        let _p5 = tracker.try_admit("198.51.100.9".parse().unwrap()).unwrap();
        assert_eq!(tracker.open_connections(), 4);
        assert!(
            tracker
                .try_admit("198.51.100.10".parse().unwrap())
                .is_none(),
            "global cap"
        );
        // Loopback is exempt from the per-IP cap but not from the global one.
        let tracker = Tracker::new(Limits {
            max_connections: 10,
            max_per_ip: 1,
        });
        let local: IpAddr = "127.0.0.1".parse().unwrap();
        let _l1 = tracker.try_admit(local).unwrap();
        let _l2 = tracker
            .try_admit(local)
            .expect("loopback is not capped per IP");
    }
}
