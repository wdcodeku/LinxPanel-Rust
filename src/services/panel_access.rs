//! How the panel itself is reached: the listen port, the security entrance (a
//! secret URL path that must be visited before the UI or API answers), allowed
//! client IPs, a bound domain, the response unauthorized visitors get, and HTTPS.

use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::rustls;

use crate::db;

pub const SETTING_ENTRANCE: &str = "security_entrance";
pub const SETTING_PANEL_SSL: &str = "panel_ssl";
pub const SETTING_PANEL_SSL_CERT_ID: &str = "panel_ssl_cert_id";
pub const SETTING_LISTEN_PORT: &str = "listen_port";
pub const SETTING_ALLOWED_IPS: &str = "allowed_ips";
pub const SETTING_BIND_DOMAIN: &str = "bind_domain";
pub const SETTING_DENY_RESPONSE: &str = "deny_response";

/// Every setting owned by this module; they are only writable through the
/// validating endpoints, never the generic settings PUT.
pub const MANAGED_SETTINGS: &[&str] = &[
    SETTING_ENTRANCE,
    SETTING_PANEL_SSL,
    SETTING_PANEL_SSL_CERT_ID,
    SETTING_LISTEN_PORT,
    SETTING_ALLOWED_IPS,
    SETTING_BIND_DOMAIN,
    SETTING_DENY_RESPONSE,
];

pub const ENTRANCE_COOKIE: &str = "lp_entrance";
/// How long a browser stays admitted after visiting the entrance.
const ENTRANCE_COOKIE_DAYS: i64 = 30;
/// Re-issue the cookie once it has less than this left, so active users never expire.
const ENTRANCE_REFRESH_DAYS: i64 = 15;

// ===== Security entrance =====

/// Normalizes an entrance to `/segment`, or None for "disabled".
///
/// Only one path segment of URL-safe characters is allowed, so it can never
/// collide with the UI's own routes or need escaping.
pub fn normalize_entrance(raw: &str) -> Result<Option<String>, String> {
    let trimmed = raw.trim().trim_matches('/');
    if trimmed.is_empty() {
        return Ok(None);
    }
    if !(6..=64).contains(&trimmed.len()) {
        return Err("Security entrance must be 6-64 characters".into());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Security entrance may only contain letters, digits, '-' and '_'".into());
    }
    const RESERVED: &[&str] = &["api", "assets", "login", "favicon.svg", "index.html"];
    if RESERVED.contains(&trimmed.to_ascii_lowercase().as_str()) {
        return Err("Security entrance conflicts with a built-in path".into());
    }
    Ok(Some(format!("/{}", trimmed)))
}

pub fn random_entrance() -> String {
    format!("/{}", db::generate_password(10).to_ascii_lowercase())
}

pub async fn load_entrance(pool: &SqlitePool) -> Option<String> {
    let value = db::get_setting(pool, SETTING_ENTRANCE)
        .await
        .ok()
        .flatten()?;
    normalize_entrance(&value).ok().flatten()
}

pub async fn save_entrance(pool: &SqlitePool, entrance: Option<&str>) -> Result<()> {
    db::set_setting(pool, SETTING_ENTRANCE, entrance.unwrap_or("")).await
}

#[derive(Serialize, Deserialize)]
struct EntranceClaims {
    /// The entrance the cookie was issued for; changing the entrance revokes it.
    e: String,
    exp: usize,
}

/// A separate key from session tokens, so an entrance cookie can never pass as a login.
fn entrance_key(jwt_secret: &str) -> Vec<u8> {
    format!("{}:security-entrance", jwt_secret).into_bytes()
}

pub enum EntranceCookie {
    Missing,
    Valid { refresh: bool },
}

pub fn check_entrance_cookie(
    cookie_header: Option<&str>,
    entrance: &str,
    jwt_secret: &str,
) -> EntranceCookie {
    let Some(value) = cookie_header.and_then(|header| {
        header.split(';').find_map(|pair| {
            let (name, value) = pair.trim().split_once('=')?;
            (name == ENTRANCE_COOKIE).then_some(value)
        })
    }) else {
        return EntranceCookie::Missing;
    };

    let claims = match decode::<EntranceClaims>(
        value,
        &DecodingKey::from_secret(&entrance_key(jwt_secret)),
        &Validation::default(),
    ) {
        Ok(data) => data.claims,
        Err(_) => return EntranceCookie::Missing,
    };
    if claims.e != entrance {
        return EntranceCookie::Missing;
    }
    let refresh_after = chrono::Utc::now() + chrono::Duration::days(ENTRANCE_REFRESH_DAYS);
    EntranceCookie::Valid {
        refresh: (claims.exp as i64) < refresh_after.timestamp(),
    }
}

/// A `Set-Cookie` value admitting this browser through `entrance`.
pub fn entrance_set_cookie(entrance: &str, jwt_secret: &str, secure: bool) -> String {
    let exp = (chrono::Utc::now() + chrono::Duration::days(ENTRANCE_COOKIE_DAYS)).timestamp();
    let token = encode(
        &Header::default(),
        &EntranceClaims {
            e: entrance.to_string(),
            exp: exp as usize,
        },
        &EncodingKey::from_secret(&entrance_key(jwt_secret)),
    )
    .unwrap_or_default();
    format!(
        "{}={}; Path=/; Max-Age={}; HttpOnly; SameSite=Lax{}",
        ENTRANCE_COOKIE,
        token,
        ENTRANCE_COOKIE_DAYS * 24 * 60 * 60,
        if secure { "; Secure" } else { "" }
    )
}

// ===== Listen port =====

pub fn validate_port(port: i64) -> Result<u16, String> {
    if (1..=65535).contains(&port) {
        Ok(port as u16)
    } else {
        Err("Port must be between 1 and 65535".into())
    }
}

/// The port saved from the panel, which overrides the PORT environment variable.
pub async fn load_listen_port(pool: &SqlitePool) -> Option<u16> {
    let value = db::get_setting(pool, SETTING_LISTEN_PORT)
        .await
        .ok()
        .flatten()?;
    validate_port(value.trim().parse().ok()?).ok()
}

/// DB setting, then the PORT environment variable, then 7200.
pub async fn effective_port(pool: &SqlitePool) -> u16 {
    if let Some(port) = load_listen_port(pool).await {
        return port;
    }
    std::env::var("PORT")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(7200)
}

// ===== Allowed IPs =====

/// An address or CIDR block, e.g. `203.0.113.7` or `10.0.0.0/8`.
#[derive(Debug, Clone, PartialEq)]
pub struct IpNet {
    addr: IpAddr,
    prefix: u8,
}

impl IpNet {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let raw = raw.trim();
        let (addr, prefix) = match raw.split_once('/') {
            Some((addr, prefix)) => (addr, Some(prefix)),
            None => (raw, None),
        };
        let addr: IpAddr = addr
            .parse()
            .map_err(|_| format!("Invalid IP address: {}", raw))?;
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix {
            Some(prefix) => prefix
                .parse::<u8>()
                .ok()
                .filter(|p| *p <= max)
                .ok_or_else(|| format!("Invalid prefix length: {}", raw))?,
            None => max,
        };
        Ok(Self { addr, prefix })
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        // An IPv4 client on a dual-stack socket shows up as ::ffff:a.b.c.d.
        let ip = ip.to_canonical();
        match (self.addr, ip) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = u32::MAX.checked_shl(32 - self.prefix as u32).unwrap_or(0);
                u32::from(net) & mask == u32::from(ip) & mask
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = u128::MAX.checked_shl(128 - self.prefix as u32).unwrap_or(0);
                u128::from(net) & mask == u128::from(ip) & mask
            }
            _ => false,
        }
    }
}

impl std::fmt::Display for IpNet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let max = if self.addr.is_ipv4() { 32 } else { 128 };
        if self.prefix == max {
            write!(f, "{}", self.addr)
        } else {
            write!(f, "{}/{}", self.addr, self.prefix)
        }
    }
}

/// Parses a comma, space or newline separated list; empty means "any IP".
pub fn parse_ip_list(raw: &str) -> Result<Vec<IpNet>, String> {
    raw.split([',', ' ', '\n', '\r', ';'])
        .filter(|item| !item.trim().is_empty())
        .map(IpNet::parse)
        .collect()
}

pub fn format_ip_list(list: &[IpNet]) -> String {
    list.iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

// ===== Bound domain =====

pub fn normalize_domain(raw: &str) -> Result<Option<String>, String> {
    let raw = raw.trim().trim_end_matches('.');
    if raw.is_empty() {
        return Ok(None);
    }
    if raw.starts_with("*.") {
        return Err("A bound domain cannot be a wildcard".into());
    }
    crate::services::cert::validate_domain(raw).map(Some)
}

/// The hostname part of a Host header (no port, no IPv6 brackets), lowercased.
pub fn host_name(host: &str) -> String {
    let host = host.trim();
    let name = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or_default()
    } else {
        host.rsplit_once(':').map_or(host, |(name, _)| name)
    };
    name.trim_end_matches('.').to_ascii_lowercase()
}

// ===== Unauthorized response =====

/// What a visitor gets when the entrance, IP allowlist or bound domain rejects them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DenyResponse {
    /// A 404 page explaining how to recover (`Lp ...`); the default.
    Hint,
    /// Plain web-server-style pages that do not reveal a panel is running.
    #[serde(rename = "404")]
    NotFound,
    #[serde(rename = "403")]
    Forbidden,
    #[serde(rename = "400")]
    BadRequest,
}

impl DenyResponse {
    pub fn parse(raw: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(raw.trim().to_string())).ok()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hint => "hint",
            Self::NotFound => "404",
            Self::Forbidden => "403",
            Self::BadRequest => "400",
        }
    }
}

/// Why a request was turned away; picks the recovery hint shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    Entrance,
    Ip,
    Domain,
}

/// Status and HTML body for a rejected request.
pub fn deny_page(mode: DenyResponse, reason: DenyReason) -> (u16, String) {
    let plain = |code: u16, text: &str| {
        (
            code,
            format!(
                "<html>\r\n<head><title>{code} {text}</title></head>\r\n<body>\r\n\
                 <center><h1>{code} {text}</h1></center>\r\n</body>\r\n</html>\r\n"
            ),
        )
    };
    let (zh, en) = match reason {
        DenyReason::Entrance => (
            "请使用正确的安全入口访问面板。忘记入口地址？请在服务器上执行 <code>Lp entrance</code> 查看。",
            "This panel only answers at its security entrance URL. Forgot it? Run <code>Lp entrance</code> on the server.",
        ),
        DenyReason::Ip => (
            "当前 IP 不在面板的授权 IP 列表中。如需解除限制，请在服务器上执行 <code>Lp allow-ip off</code>。",
            "Your IP address is not on the panel's allowlist. To lift the restriction, run <code>Lp allow-ip off</code> on the server.",
        ),
        DenyReason::Domain => (
            "面板已绑定域名，请通过绑定的域名访问。如需解除绑定，请在服务器上执行 <code>Lp domain off</code>。",
            "The panel is bound to a domain; open it through that domain. To unbind it, run <code>Lp domain off</code> on the server.",
        ),
    };
    match mode {
        DenyResponse::NotFound => plain(404, "Not Found"),
        DenyResponse::Forbidden => plain(403, "Forbidden"),
        DenyResponse::BadRequest => plain(400, "Bad Request"),
        DenyResponse::Hint => (
            404,
            format!(
                r#"<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>404</title>
<style>body{{font-family:system-ui,sans-serif;max-width:560px;margin:12vh auto;padding:0 16px;color:#333;line-height:1.6}}
code{{background:#f2f2f2;padding:2px 6px;border-radius:4px;white-space:nowrap}}</style></head>
<body><h2>无法访问面板</h2><p>{zh}</p><hr><h2>Access denied</h2><p>{en}</p></body></html>"#
            ),
        ),
    }
}

// ===== Policy =====

/// Everything the access guard checks, loaded once and swapped on change.
#[derive(Debug, Clone)]
pub struct AccessPolicy {
    pub entrance: Option<String>,
    pub allowed_ips: Vec<IpNet>,
    pub domain: Option<String>,
    pub deny: DenyResponse,
}

impl Default for AccessPolicy {
    fn default() -> Self {
        Self {
            entrance: None,
            allowed_ips: Vec::new(),
            domain: None,
            deny: DenyResponse::Hint,
        }
    }
}

impl AccessPolicy {
    pub async fn load(pool: &SqlitePool) -> Self {
        let get = |key: &'static str| async move {
            db::get_setting(pool, key)
                .await
                .ok()
                .flatten()
                .unwrap_or_default()
        };
        Self {
            entrance: load_entrance(pool).await,
            // An unparsable entry is dropped rather than locking everyone out.
            allowed_ips: get(SETTING_ALLOWED_IPS)
                .await
                .split([',', ' ', '\n'])
                .filter_map(|item| IpNet::parse(item).ok())
                .collect(),
            domain: normalize_domain(&get(SETTING_BIND_DOMAIN).await)
                .ok()
                .flatten(),
            deny: DenyResponse::parse(&get(SETTING_DENY_RESPONSE).await)
                .unwrap_or(DenyResponse::Hint),
        }
    }

    pub fn ip_allowed(&self, ip: IpAddr) -> bool {
        self.allowed_ips.is_empty() || self.allowed_ips.iter().any(|net| net.contains(ip))
    }

    pub fn host_allowed(&self, host: Option<&str>) -> bool {
        match &self.domain {
            None => true,
            Some(domain) => host.is_some_and(|host| host_name(host) == *domain),
        }
    }
}

// ===== Panel HTTPS =====

pub fn ssl_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("panel-ssl")
}

/// Certificate and key the panel serves: a certificate from the certificate
/// store when one is selected, otherwise the panel's own self-signed pair.
pub async fn ssl_paths(pool: &SqlitePool, data_dir: &Path) -> (PathBuf, PathBuf, bool) {
    let cert_id = db::get_setting(pool, SETTING_PANEL_SSL_CERT_ID)
        .await
        .ok()
        .flatten()
        .and_then(|value| value.trim().parse::<i64>().ok());
    if let Some(id) = cert_id {
        if let Ok(record) = crate::services::cert::get(pool, id).await {
            return (
                PathBuf::from(record.cert_path),
                PathBuf::from(record.key_path),
                false,
            );
        }
    }
    let dir = ssl_dir(data_dir);
    (dir.join("cert.pem"), dir.join("key.pem"), true)
}

pub async fn ssl_enabled(pool: &SqlitePool) -> bool {
    db::get_setting(pool, SETTING_PANEL_SSL)
        .await
        .ok()
        .flatten()
        .is_some_and(|value| value == "on")
}

/// Writes a self-signed certificate covering localhost, this host's name and
/// `extra_names` (typically the server's public and private IPs).
pub fn generate_self_signed(data_dir: &Path, extra_names: &[String]) -> Result<()> {
    let mut names = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    if let Some(host) = sysinfo::System::host_name() {
        names.push(host);
    }
    names.extend(extra_names.iter().cloned());
    names.retain(|name| !name.trim().is_empty());
    names.sort();
    names.dedup();

    let mut params = rcgen::CertificateParams::new(names).context("Invalid certificate name")?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "LinxPanel");
    let today = chrono::Utc::now().date_naive();
    use chrono::Datelike;
    params.not_before = rcgen::date_time_ymd(today.year(), 1, 1);
    params.not_after = rcgen::date_time_ymd(today.year() + 10, 1, 1);

    let key = rcgen::KeyPair::generate().context("Cannot generate key")?;
    let cert = params
        .self_signed(&key)
        .context("Cannot sign certificate")?;

    let dir = ssl_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    write_private(&dir.join("key.pem"), &key.serialize_pem())?;
    std::fs::write(dir.join("cert.pem"), cert.pem())?;
    Ok(())
}

fn write_private(path: &Path, contents: &str) -> Result<()> {
    std::fs::write(path, contents)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Builds the rustls config, failing on a missing, unreadable or mismatched pair.
pub fn load_tls_config(cert_path: &Path, key_path: &Path) -> Result<Arc<rustls::ServerConfig>> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};

    let certs = CertificateDer::pem_file_iter(cert_path)
        .with_context(|| format!("Cannot read {}", cert_path.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("Invalid certificate {}", cert_path.display()))?;
    if certs.is_empty() {
        return Err(anyhow!("No certificate found in {}", cert_path.display()));
    }
    let key = PrivateKeyDer::from_pem_file(key_path)
        .with_context(|| format!("Invalid private key {}", key_path.display()))?;

    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("Certificate and private key do not match")?;
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Arc::new(config))
}

/// Answers a plain-HTTP request on the HTTPS port with a 301 to the same URL over https.
pub(crate) async fn redirect_plain_http(mut stream: TcpStream, port: u16) {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    while buffer.len() < 8192 && !buffer.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
        }
    }
    let Some(location) = https_location(&String::from_utf8_lossy(&buffer), port) else {
        let _ = stream
            .write_all(
                b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
            )
            .await;
        return;
    };
    let response = format!(
        "HTTP/1.1 301 Moved Permanently\r\nLocation: {}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n",
        location
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// The https:// URL for a raw HTTP request head, or None when it is malformed.
/// Host and path are allowlisted so nothing can be injected into the response.
fn https_location(head: &str, port: u16) -> Option<String> {
    let mut lines = head.split("\r\n");
    let path = lines.next()?.split(' ').nth(1)?;
    if !path.starts_with('/') || !path.bytes().all(|b| b.is_ascii_graphic()) {
        return None;
    }
    let host = lines
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.trim()
                .eq_ignore_ascii_case("host")
                .then(|| value.trim())
        })
        .unwrap_or("");
    if host.is_empty()
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
    {
        return None;
    }
    let host = if host.contains(':') {
        host.to_string()
    } else {
        format!("{}:{}", host, port)
    };
    Some(format!("https://{}{}", host, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entrance_normalization() {
        assert_eq!(normalize_entrance("").unwrap(), None);
        assert_eq!(normalize_entrance(" / ").unwrap(), None);
        assert_eq!(
            normalize_entrance("abc123x").unwrap().as_deref(),
            Some("/abc123x")
        );
        assert_eq!(
            normalize_entrance("/abc123x/").unwrap().as_deref(),
            Some("/abc123x")
        );
        assert!(normalize_entrance("short").is_err());
        assert!(normalize_entrance("/has/slash").is_err());
        assert!(normalize_entrance("bad%char").is_err());
        assert!(normalize_entrance("assets").is_err());
        assert!(normalize_entrance(&random_entrance()).unwrap().is_some());
    }

    #[test]
    fn entrance_cookie_roundtrip() {
        let cookie = entrance_set_cookie("/secret1", "key", false);
        let value = cookie.split(';').next().unwrap();
        assert!(matches!(
            check_entrance_cookie(Some(value), "/secret1", "key"),
            EntranceCookie::Valid { refresh: false }
        ));
        // A changed entrance or key revokes the cookie.
        assert!(matches!(
            check_entrance_cookie(Some(value), "/other12", "key"),
            EntranceCookie::Missing
        ));
        assert!(matches!(
            check_entrance_cookie(Some(value), "/secret1", "other"),
            EntranceCookie::Missing
        ));
        assert!(matches!(
            check_entrance_cookie(None, "/secret1", "key"),
            EntranceCookie::Missing
        ));
    }

    #[test]
    fn ip_allowlist_matching() {
        let list = parse_ip_list("203.0.113.7, 10.0.0.0/8\n2001:db8::/32").unwrap();
        let policy = AccessPolicy {
            allowed_ips: list.clone(),
            ..AccessPolicy::default()
        };
        assert!(policy.ip_allowed("203.0.113.7".parse().unwrap()));
        assert!(!policy.ip_allowed("203.0.113.8".parse().unwrap()));
        assert!(policy.ip_allowed("10.20.30.40".parse().unwrap()));
        assert!(policy.ip_allowed("::ffff:10.1.2.3".parse().unwrap()));
        assert!(policy.ip_allowed("2001:db8:1::5".parse().unwrap()));
        assert!(!policy.ip_allowed("2001:db9::5".parse().unwrap()));
        assert!(AccessPolicy::default().ip_allowed("198.51.100.1".parse().unwrap()));
        assert_eq!(
            format_ip_list(&list),
            "203.0.113.7,10.0.0.0/8,2001:db8::/32"
        );
        assert!(IpNet::parse("0.0.0.0/0")
            .unwrap()
            .contains("8.8.8.8".parse().unwrap()));
        assert!(parse_ip_list("1.2.3.4/33").is_err());
        assert!(parse_ip_list("not-an-ip").is_err());
    }

    #[test]
    fn domain_binding() {
        assert_eq!(
            normalize_domain(" Panel.Example.com. ").unwrap().as_deref(),
            Some("panel.example.com")
        );
        assert_eq!(normalize_domain("").unwrap(), None);
        assert!(normalize_domain("*.example.com").is_err());
        let policy = AccessPolicy {
            domain: Some("panel.example.com".into()),
            ..AccessPolicy::default()
        };
        assert!(policy.host_allowed(Some("panel.example.com:7200")));
        assert!(policy.host_allowed(Some("PANEL.example.com")));
        assert!(!policy.host_allowed(Some("1.2.3.4:7200")));
        assert!(!policy.host_allowed(None));
        assert_eq!(host_name("[2001:db8::1]:7200"), "2001:db8::1");
    }

    #[test]
    fn deny_response_modes() {
        assert_eq!(DenyResponse::parse("404"), Some(DenyResponse::NotFound));
        assert_eq!(DenyResponse::parse("hint"), Some(DenyResponse::Hint));
        assert_eq!(DenyResponse::parse("nope"), None);
        let (code, body) = deny_page(DenyResponse::Forbidden, DenyReason::Ip);
        assert_eq!(code, 403);
        assert!(
            !body.contains("Lp"),
            "plain pages must not reveal the panel"
        );
        let (code, body) = deny_page(DenyResponse::Hint, DenyReason::Domain);
        assert_eq!(code, 404);
        assert!(body.contains("Lp domain off"));
    }

    #[test]
    fn plain_http_redirect_location() {
        let head = "GET /login?x=1 HTTP/1.1\r\nHost: 1.2.3.4:7200\r\n\r\n";
        assert_eq!(
            https_location(head, 7200).as_deref(),
            Some("https://1.2.3.4:7200/login?x=1")
        );
        let head = "GET / HTTP/1.1\r\nhost: panel.example.com\r\n\r\n";
        assert_eq!(
            https_location(head, 8888).as_deref(),
            Some("https://panel.example.com:8888/")
        );
        assert!(https_location("GET / HTTP/1.1\r\nHost: evil\r\nX\r\n\r\n", 1).is_some());
        assert!(https_location("GET / HTTP/1.1\r\nHost: a b\r\n\r\n", 1).is_none());
        assert!(https_location("garbage", 1).is_none());
    }

    #[test]
    fn self_signed_certificate_loads() {
        let dir = std::env::temp_dir().join(format!("lp-ssl-test-{}", uuid::Uuid::new_v4()));
        generate_self_signed(&dir, &["203.0.113.5".to_string()]).unwrap();
        let ssl = ssl_dir(&dir);
        assert!(load_tls_config(&ssl.join("cert.pem"), &ssl.join("key.pem")).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
