//! Alert notifications: channels (DingTalk, WeCom, Feishu, Telegram, generic
//! webhook, e-mail), the events that send them, and the monitor that watches
//! CPU / memory / disk, crashing applications and expiring certificates.
//!
//! Everything goes out through `curl` with its configuration on stdin, so
//! tokens and SMTP passwords never appear in the process list.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::shell;

pub const SETTING_KEY: &str = "notify";
/// Shown instead of a stored secret; saving it back keeps the stored value.
pub const MASK: &str = "••••••••";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Channel {
    pub id: String,
    pub name: String,
    /// dingtalk | wecom | feishu | telegram | webhook | email
    pub kind: String,
    pub enabled: bool,
    /// Webhook URL (Telegram: unused).
    pub url: String,
    /// DingTalk / Feishu signing secret; Telegram bot token.
    pub secret: String,
    /// Telegram chat id.
    pub chat_id: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    /// ssl | starttls | none
    pub security: String,
    pub username: String,
    pub password: String,
    pub from: String,
    /// Comma-separated recipients.
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Events {
    pub resource: bool,
    pub app_crash: bool,
    pub ssl_expiry: bool,
    pub ssl_days: u32,
    pub backup_failed: bool,
    pub login: bool,
    pub login_failed: bool,
}

impl Default for Events {
    fn default() -> Self {
        Self {
            resource: true,
            app_crash: true,
            ssl_expiry: true,
            ssl_days: 7,
            backup_failed: true,
            login: false,
            login_failed: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub channels: Vec<Channel>,
    pub events: Events,
    /// The same alert is not repeated within this many minutes.
    pub cooldown_minutes: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            channels: Vec::new(),
            events: Events::default(),
            cooldown_minutes: 30,
        }
    }
}

pub async fn load(pool: &SqlitePool) -> Config {
    crate::db::get_setting(pool, SETTING_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// The configuration for the browser: secrets replaced by `MASK`.
pub fn masked(mut config: Config) -> Config {
    for c in &mut config.channels {
        for field in [&mut c.secret, &mut c.password] {
            if !field.is_empty() {
                *field = MASK.to_string();
            }
        }
        // DingTalk / WeCom / Feishu webhook URLs carry their token.
        if !c.url.is_empty() && c.kind != "webhook" {
            if let Some(at) = c.url.find("access_token=").or_else(|| c.url.find("key=")) {
                let keep = c.url[..at].len() + c.url[at..].find('=').map(|i| i + 1).unwrap_or(0);
                c.url = format!("{}{}", &c.url[..keep], MASK);
            } else if let Some(at) = c.url.rfind('/') {
                if c.url.contains("/hook/") {
                    c.url = format!("{}/{}", &c.url[..at], MASK);
                }
            }
        }
    }
    config
}

/// Validates and stores `incoming`, keeping stored secrets where the browser
/// sent the mask back.
pub async fn save(pool: &SqlitePool, mut incoming: Config) -> Result<(), String> {
    let stored = load(pool).await;
    for c in &mut incoming.channels {
        if c.id.trim().is_empty() {
            c.id = uuid::Uuid::new_v4().simple().to_string()[..10].to_string();
        }
        let old = stored.channels.iter().find(|o| o.id == c.id);
        if let Some(old) = old {
            if c.secret == MASK {
                c.secret = old.secret.clone();
            }
            if c.password == MASK {
                c.password = old.password.clone();
            }
            if c.url.contains(MASK) {
                c.url = old.url.clone();
            }
        }
        validate(c)?;
    }
    if incoming.events.ssl_days > 90 {
        return Err("Warn at most 90 days before certificates expire".into());
    }
    crate::db::set_setting(
        pool,
        SETTING_KEY,
        &serde_json::to_string(&incoming).map_err(|e| e.to_string())?,
    )
    .await
    .map_err(|e| e.to_string())
}

fn printable(value: &str) -> bool {
    !value.chars().any(|c| c.is_control())
}

fn validate(c: &Channel) -> Result<(), String> {
    let name = if c.name.trim().is_empty() {
        c.kind.as_str()
    } else {
        c.name.as_str()
    };
    let url_ok = |u: &str| {
        (u.starts_with("https://") || u.starts_with("http://"))
            && u.len() < 1000
            && printable(u)
            && !u.contains(' ')
    };
    match c.kind.as_str() {
        "dingtalk" | "wecom" | "feishu" | "webhook" => {
            if !url_ok(&c.url) {
                return Err(format!("{}: enter the webhook URL (http/https)", name));
            }
        }
        "telegram" => {
            if c.secret.trim().is_empty() || c.chat_id.trim().is_empty() {
                return Err(format!("{}: bot token and chat id are required", name));
            }
        }
        "email" => {
            if c.smtp_host.trim().is_empty() || c.smtp_port == 0 {
                return Err(format!("{}: SMTP server and port are required", name));
            }
            if !c.from.contains('@') || recipients(&c.to).is_empty() {
                return Err(format!(
                    "{}: sender and recipient addresses are required",
                    name
                ));
            }
            if !matches!(c.security.as_str(), "ssl" | "starttls" | "none") {
                return Err(format!("{}: unknown encryption", name));
            }
        }
        other => return Err(format!("Unknown channel type: {}", other)),
    }
    for value in [
        &c.name,
        &c.secret,
        &c.chat_id,
        &c.smtp_host,
        &c.username,
        &c.password,
        &c.from,
        &c.to,
    ] {
        if !printable(value) || value.len() > 500 {
            return Err(format!("{}: invalid characters", name));
        }
    }
    Ok(())
}

fn recipients(to: &str) -> Vec<String> {
    to.split([',', ';', ' '])
        .map(str::trim)
        .filter(|a| a.contains('@'))
        .map(str::to_string)
        .collect()
}

// ===== Messages =====

#[derive(Debug, Clone)]
pub struct Alert {
    /// Same key within the cooldown: sent once.
    pub key: String,
    pub title: String,
    pub body: String,
}

async fn is_zh(pool: &SqlitePool) -> bool {
    let saved = crate::db::get_setting(pool, crate::cli::SETTING_LANGUAGE)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    crate::cli::normalize_language(&saved) != Some("en")
}

fn hostname() -> String {
    sysinfo::System::host_name().unwrap_or_else(|| "server".into())
}

fn last_sent() -> &'static Mutex<HashMap<String, Instant>> {
    static SENT: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    SENT.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Sends `alert` to every enabled channel (unless the same key was sent
/// within the cooldown) and records it in the alert history.
pub async fn send(pool: &SqlitePool, alert: Alert) {
    let config = load(pool).await;
    let cooldown = Duration::from_secs(u64::from(config.cooldown_minutes.max(1)) * 60);
    {
        let mut sent = last_sent().lock().unwrap_or_else(|e| e.into_inner());
        if sent
            .get(&alert.key)
            .is_some_and(|at| at.elapsed() < cooldown)
        {
            return;
        }
        sent.insert(alert.key.clone(), Instant::now());
    }
    let channels: Vec<Channel> = config.channels.into_iter().filter(|c| c.enabled).collect();
    let zh = is_zh(pool).await;
    let (title, body) = (alert.title.clone(), alert.body.clone());
    let results = tokio::task::spawn_blocking(move || {
        channels
            .iter()
            .map(|c| (c.name.clone(), deliver(c, &title, &body, zh)))
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    let summary = if results.is_empty() {
        "no channel".to_string()
    } else {
        results
            .iter()
            .map(|(name, r)| match r {
                Ok(()) => format!("{}: ok", name),
                Err(e) => format!("{}: {}", name, e),
            })
            .collect::<Vec<_>>()
            .join("; ")
    };
    let _ = sqlx::query("INSERT INTO alert_log (title, body, result) VALUES (?, ?, ?)")
        .bind(&alert.title)
        .bind(&alert.body)
        .bind(&summary)
        .execute(pool)
        .await;
    let _ = sqlx::query("DELETE FROM alert_log WHERE id NOT IN (SELECT id FROM alert_log ORDER BY id DESC LIMIT 500)")
        .execute(pool)
        .await;
}

/// Sends a test message through one channel, now (no cooldown).
pub async fn test(pool: &SqlitePool, channel: Channel) -> Result<(), String> {
    let stored = load(pool).await;
    let mut channel = channel;
    if let Some(old) = stored.channels.iter().find(|o| o.id == channel.id) {
        if channel.secret == MASK {
            channel.secret = old.secret.clone();
        }
        if channel.password == MASK {
            channel.password = old.password.clone();
        }
        if channel.url.contains(MASK) {
            channel.url = old.url.clone();
        }
    }
    validate(&channel)?;
    let zh = is_zh(pool).await;
    let (title, body) = if zh {
        (
            "测试通知".to_string(),
            "这是一条来自 LinxPanel 的测试消息，收到即表示通知渠道配置正确。".to_string(),
        )
    } else {
        (
            "Test notification".to_string(),
            "A test message from LinxPanel: this channel works.".to_string(),
        )
    };
    tokio::task::spawn_blocking(move || deliver(&channel, &title, &body, zh))
        .await
        .map_err(|e| e.to_string())?
}

fn full_text(title: &str, body: &str, zh: bool) -> String {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S %Z");
    if zh {
        format!(
            "【LinxPanel 告警】{}\n服务器：{}\n{}\n时间：{}",
            title,
            hostname(),
            body,
            now
        )
    } else {
        format!(
            "[LinxPanel alert] {}\nServer: {}\n{}\nTime: {}",
            title,
            hostname(),
            body,
            now
        )
    }
}

/// A value inside a double-quoted curl config string.
fn quoted(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{}\"", escaped)
}

pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    ring::hmac::sign(&key, data).as_ref().to_vec()
}

fn urlencode(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{:02X}", b)
            }
        })
        .collect()
}

/// DingTalk "加签": sign = base64(HMAC-SHA256(secret, "{ts}\n{secret}")).
pub fn dingtalk_url(url: &str, secret: &str, ts_millis: i64) -> String {
    if secret.is_empty() {
        return url.to_string();
    }
    let sign = base64(&hmac_sha256(
        secret.as_bytes(),
        format!("{}\n{}", ts_millis, secret).as_bytes(),
    ));
    let sep = if url.contains('?') { '&' } else { '?' };
    format!(
        "{}{}timestamp={}&sign={}",
        url,
        sep,
        ts_millis,
        urlencode(&sign)
    )
}

/// Feishu signature: base64(HMAC-SHA256(key = "{ts}\n{secret}", message = "")).
pub fn feishu_sign(secret: &str, ts_secs: i64) -> String {
    base64(&hmac_sha256(
        format!("{}\n{}", ts_secs, secret).as_bytes(),
        b"",
    ))
}

/// POSTs JSON; checks the HTTP status and the platform's own error code.
fn post_json(url: &str, body: &serde_json::Value) -> Result<(), String> {
    let config = format!(
        "url = {}\nrequest = \"POST\"\nheader = \"Content-Type: application/json; charset=utf-8\"\ndata-binary = {}\nsilent\nshow-error\nmax-time = 20\nwrite-out = \"\\n%{{http_code}}\"\n",
        quoted(url),
        quoted(&body.to_string())
    );
    let output =
        shell::run_with_input("curl", &["-K", "-"], Some(&config), Duration::from_secs(30))?;
    if !output.ok() {
        return Err(output.error_text());
    }
    let text = output.stdout.trim_end();
    let (response, code) = text.rsplit_once('\n').unwrap_or(("", text));
    let code: u16 = code.trim().parse().unwrap_or(0);
    if !(200..300).contains(&code) {
        return Err(format!(
            "HTTP {} {}",
            code,
            response.chars().take(200).collect::<String>()
        ));
    }
    // DingTalk / WeCom: errcode; Feishu: code / StatusCode; Telegram: ok.
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(response) {
        let failed = json
            .get("errcode")
            .and_then(|v| v.as_i64())
            .is_some_and(|v| v != 0)
            || json
                .get("code")
                .and_then(|v| v.as_i64())
                .is_some_and(|v| v != 0)
            || json
                .get("StatusCode")
                .and_then(|v| v.as_i64())
                .is_some_and(|v| v != 0)
            || json.get("ok").and_then(|v| v.as_bool()) == Some(false);
        if failed {
            return Err(response.chars().take(300).collect());
        }
    }
    Ok(())
}

fn deliver(c: &Channel, title: &str, body: &str, zh: bool) -> Result<(), String> {
    let text = full_text(title, body, zh);
    match c.kind.as_str() {
        "dingtalk" => post_json(
            &dingtalk_url(
                &c.url,
                c.secret.trim(),
                chrono::Utc::now().timestamp_millis(),
            ),
            &serde_json::json!({ "msgtype": "text", "text": { "content": text } }),
        ),
        "wecom" => post_json(
            &c.url,
            &serde_json::json!({ "msgtype": "text", "text": { "content": text } }),
        ),
        "feishu" => {
            let mut body = serde_json::json!({ "msg_type": "text", "content": { "text": text } });
            if !c.secret.trim().is_empty() {
                let ts = chrono::Utc::now().timestamp();
                body["timestamp"] = serde_json::json!(ts.to_string());
                body["sign"] = serde_json::json!(feishu_sign(c.secret.trim(), ts));
            }
            post_json(&c.url, &body)
        }
        "telegram" => {
            let base = if c.url.trim().is_empty() {
                "https://api.telegram.org"
            } else {
                c.url.trim().trim_end_matches('/')
            };
            post_json(
                &format!("{}/bot{}/sendMessage", base, c.secret.trim()),
                &serde_json::json!({ "chat_id": c.chat_id.trim(), "text": text }),
            )
        }
        "webhook" => post_json(
            &c.url,
            &serde_json::json!({
                "title": title,
                "text": text,
                "body": body,
                "host": hostname(),
                "time": chrono::Utc::now().to_rfc3339(),
                "source": "linxpanel",
            }),
        ),
        "email" => send_mail(c, title, &text),
        other => Err(format!("Unknown channel type: {}", other)),
    }
}

fn send_mail(c: &Channel, title: &str, text: &str) -> Result<(), String> {
    let to = recipients(&c.to);
    let subject = format!("[LinxPanel] {} - {}", title, hostname());
    let message = format!(
        "From: LinxPanel <{from}>\r\nTo: {to}\r\nSubject: =?UTF-8?B?{subject}?=\r\nDate: {date}\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n{body}\r\n",
        from = c.from.trim(),
        to = to.join(", "),
        subject = base64(subject.as_bytes()),
        date = chrono::Local::now().to_rfc2822(),
        body = base64(text.replace('\n', "\r\n").as_bytes())
            .as_bytes()
            .chunks(76)
            .map(|line| String::from_utf8_lossy(line).to_string())
            .collect::<Vec<_>>()
            .join("\r\n"),
    );
    let scheme = if c.security == "ssl" { "smtps" } else { "smtp" };
    let mut config = format!(
        "url = {}\nmail-from = {}\nsilent\nshow-error\nmax-time = 30\nupload-file = \"-\"\n",
        quoted(&format!(
            "{}://{}:{}",
            scheme,
            c.smtp_host.trim(),
            c.smtp_port
        )),
        quoted(c.from.trim()),
    );
    for rcpt in &to {
        config.push_str(&format!("mail-rcpt = {}\n", quoted(rcpt)));
    }
    if c.security == "starttls" {
        config.push_str("ssl-reqd\n");
    }
    if !c.username.is_empty() {
        config.push_str(&format!(
            "user = {}\n",
            quoted(&format!("{}:{}", c.username, c.password))
        ));
    }
    // The config goes in through a private file, the message through stdin.
    let dir = std::env::temp_dir();
    let path = dir.join(format!("lp-mail-{}.conf", uuid::Uuid::new_v4().simple()));
    {
        use std::io::Write;
        #[cfg(unix)]
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&path).map_err(|e| e.to_string())?;
        file.write_all(config.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let path_text = path.to_string_lossy().to_string();
    let result = shell::run_with_input(
        "curl",
        &["-K", &path_text],
        Some(&message),
        Duration::from_secs(45),
    );
    let _ = std::fs::remove_file(&path);
    let output = result?;
    if output.ok() {
        Ok(())
    } else {
        Err(output.error_text())
    }
}

// ===== Monitor =====

#[derive(Default)]
struct Watch {
    over: HashMap<&'static str, u32>,
    restarts: HashMap<i64, u32>,
    last_ssl_check: Option<Instant>,
}

fn thresholds(settings: &HashMap<String, String>) -> Option<(f64, f64, f64)> {
    if settings.get("alerts_enabled").map(String::as_str) != Some("true") {
        return None;
    }
    let get = |k: &str, d: f64| {
        settings
            .get(k)
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(d)
    };
    Some((
        get("alert_cpu", 90.0),
        get("alert_memory", 90.0),
        get("alert_disk", 85.0),
    ))
}

async fn settings_map(pool: &SqlitePool) -> HashMap<String, String> {
    sqlx::query_as::<_, (String, String)>("SELECT key, value FROM settings")
        .fetch_all(pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// Background: checks every minute and sends what is due.
pub async fn run(state: Arc<crate::AppState>, mut shutdown: tokio::sync::watch::Receiver<bool>) {
    let mut watch = Watch::default();
    // Give the metrics engine time to fill its history.
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(90)) => {}
        _ = shutdown.changed() => return,
    }
    loop {
        check(&state, &mut watch).await;
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(60)) => {}
            _ = shutdown.changed() => return,
        }
    }
}

async fn check(state: &Arc<crate::AppState>, watch: &mut Watch) {
    let config = load(&state.db).await;
    if !config.channels.iter().any(|c| c.enabled) {
        return;
    }
    let zh = is_zh(&state.db).await;
    let pool = &state.db;

    // CPU and memory: the last minute's average, high for 3 minutes in a row.
    if config.events.resource {
        if let Some((cpu_max, mem_max, disk_max)) = thresholds(&settings_map(pool).await) {
            let snapshot = state.metrics.snapshot();
            let now = crate::services::metrics::epoch_millis();
            let recent: Vec<_> = snapshot
                .history
                .iter()
                .filter(|s| now.saturating_sub(s.ts) <= 60_000)
                .collect();
            if !recent.is_empty() {
                let cpu = recent.iter().map(|s| f64::from(s.cpu_percent)).sum::<f64>()
                    / recent.len() as f64;
                let mem = recent.iter().map(|s| s.mem_percent).sum::<f64>() / recent.len() as f64;
                for (key, value, max) in [("cpu", cpu, cpu_max), ("memory", mem, mem_max)] {
                    let count = watch.over.entry(key).or_insert(0);
                    *count = if value >= max { *count + 1 } else { 0 };
                    if *count >= 3 {
                        let (title, body) = match (key, zh) {
                            ("cpu", true) => (
                                "CPU 使用率过高".to_string(),
                                format!(
                                    "CPU 使用率已连续 3 分钟超过 {:.0}%（当前 {:.1}%）",
                                    max, value
                                ),
                            ),
                            ("cpu", false) => (
                                "High CPU usage".to_string(),
                                format!(
                                    "CPU has been above {:.0}% for 3 minutes (now {:.1}%)",
                                    max, value
                                ),
                            ),
                            (_, true) => (
                                "内存使用率过高".to_string(),
                                format!(
                                    "内存使用率已连续 3 分钟超过 {:.0}%（当前 {:.1}%）",
                                    max, value
                                ),
                            ),
                            (_, false) => (
                                "High memory usage".to_string(),
                                format!(
                                    "Memory has been above {:.0}% for 3 minutes (now {:.1}%)",
                                    max, value
                                ),
                            ),
                        };
                        send(
                            pool,
                            Alert {
                                key: format!("resource:{}", key),
                                title,
                                body,
                            },
                        )
                        .await;
                    }
                }
            }
            let disks = sysinfo::Disks::new_with_refreshed_list();
            for disk in disks.list() {
                let total = disk.total_space();
                if total < 1 << 30 {
                    continue; // tiny / pseudo filesystems
                }
                let mount = disk.mount_point().to_string_lossy().to_string();
                if mount.starts_with("/boot") || mount.starts_with("/snap") {
                    continue;
                }
                let used = (total - disk.available_space()) as f64 * 100.0 / total as f64;
                if used >= disk_max {
                    let (title, body) = if zh {
                        (
                            "磁盘空间不足".to_string(),
                            format!(
                                "{} 已使用 {:.1}%（阈值 {:.0}%），剩余 {} GB",
                                mount,
                                used,
                                disk_max,
                                disk.available_space() >> 30
                            ),
                        )
                    } else {
                        (
                            "Low disk space".to_string(),
                            format!(
                                "{} is {:.1}% full (threshold {:.0}%), {} GB free",
                                mount,
                                used,
                                disk_max,
                                disk.available_space() >> 30
                            ),
                        )
                    };
                    send(
                        pool,
                        Alert {
                            key: format!("disk:{}", mount),
                            title,
                            body,
                        },
                    )
                    .await;
                }
            }
        }
    }

    // Applications that crashed (the supervisor restarted them).
    if config.events.app_crash {
        if let Ok(apps) = super::appproc::list(pool).await {
            for app in apps {
                let st = state.supervisor.state_of(app.id).await;
                let before = watch
                    .restarts
                    .insert(app.id, st.restarts)
                    .unwrap_or(st.restarts);
                if st.restarts > before {
                    let (title, body) = if zh {
                        (
                            format!("应用崩溃：{}", app.name),
                            format!(
                                "应用 {} 异常退出，已自动重启（累计 {} 次）。\n原因：{}",
                                app.name, st.restarts, st.last_error
                            ),
                        )
                    } else {
                        (format!("Application crashed: {}", app.name), format!("{} exited unexpectedly and was restarted ({} restarts so far).\nReason: {}", app.name, st.restarts, st.last_error))
                    };
                    send(
                        pool,
                        Alert {
                            key: format!("app:{}", app.id),
                            title,
                            body,
                        },
                    )
                    .await;
                }
            }
        }
    }

    // Certificates close to expiry: twice a day.
    if config.events.ssl_expiry
        && watch
            .last_ssl_check
            .is_none_or(|t| t.elapsed() > Duration::from_secs(12 * 3600))
    {
        watch.last_ssl_check = Some(Instant::now());
        if let Ok(certs) = super::cert::list(pool).await {
            for cert in certs {
                if cert.status == "unknown"
                    || cert.days_remaining > i64::from(config.events.ssl_days)
                {
                    continue;
                }
                let domains = cert.record.domains.clone();
                let (title, body) = match (cert.days_remaining < 0, zh) {
                    (true, true) => (
                        "SSL 证书已过期".to_string(),
                        format!("{} 的证书已过期 {} 天。", domains, -cert.days_remaining),
                    ),
                    (true, false) => (
                        "SSL certificate expired".to_string(),
                        format!(
                            "The certificate for {} expired {} days ago.",
                            domains, -cert.days_remaining
                        ),
                    ),
                    (false, true) => (
                        "SSL 证书即将过期".to_string(),
                        format!(
                            "{} 的证书将在 {} 天后过期，请检查自动续期。",
                            domains, cert.days_remaining
                        ),
                    ),
                    (false, false) => (
                        "SSL certificate expiring".to_string(),
                        format!(
                            "The certificate for {} expires in {} days; check its renewal.",
                            domains, cert.days_remaining
                        ),
                    ),
                };
                // At most daily per certificate.
                let key = format!("ssl:{}", cert.record.id);
                let fresh = last_sent()
                    .lock()
                    .map(|m| {
                        m.get(&key)
                            .is_none_or(|t| t.elapsed() > Duration::from_secs(20 * 3600))
                    })
                    .unwrap_or(true);
                if fresh {
                    send(pool, Alert { key, title, body }).await;
                }
            }
        }
    }
}

/// For callers outside the monitor (login, backups).
pub async fn event(
    pool: &SqlitePool,
    kind: &str,
    key: String,
    title_zh: String,
    title_en: String,
    body_zh: String,
    body_en: String,
) {
    let config = load(pool).await;
    let wanted = match kind {
        "login" => config.events.login,
        "login_failed" => config.events.login_failed,
        "backup_failed" => config.events.backup_failed,
        _ => true,
    };
    if !wanted || !config.channels.iter().any(|c| c.enabled) {
        return;
    }
    let zh = is_zh(pool).await;
    let (title, body) = if zh {
        (title_zh, body_zh)
    } else {
        (title_en, body_en)
    };
    send(pool, Alert { key, title, body }).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn signatures() {
        // Cross-checked with Python's hmac / base64.
        let url = dingtalk_url(
            "https://oapi.dingtalk.com/robot/send?access_token=abc",
            "SECxyz",
            1700000000000,
        );
        assert_eq!(
            url,
            "https://oapi.dingtalk.com/robot/send?access_token=abc&timestamp=1700000000000&sign=0PUR1j8g85Xg3vlFV%2FUrEcxXfF5HpCAGzcjrNfyJoyg%3D"
        );
        assert_eq!(
            feishu_sign("demo", 1700000000),
            "8oT2n3SMKFfEnDoiwer8BUM/SjKLwe9SqoEIHlhDTKo="
        );
    }

    #[test]
    fn config_strings_are_escaped() {
        assert_eq!(quoted("a\"b\\c\nd"), "\"a\\\"b\\\\c\\nd\"");
    }

    #[test]
    fn secrets_are_masked_and_kept() {
        let config = Config {
            channels: vec![Channel {
                id: "1".into(),
                kind: "dingtalk".into(),
                url: "https://oapi.dingtalk.com/robot/send?access_token=SECRET".into(),
                secret: "SEC".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let m = masked(config);
        assert_eq!(m.channels[0].secret, MASK);
        assert!(
            !m.channels[0].url.contains("SECRET") && m.channels[0].url.contains("access_token=")
        );
    }

    #[test]
    fn validation() {
        let mut c = Channel {
            kind: "email".into(),
            smtp_host: "smtp.qq.com".into(),
            smtp_port: 465,
            security: "ssl".into(),
            from: "a@b.c".into(),
            to: "x@y.z, w@v.u".into(),
            ..Default::default()
        };
        assert!(validate(&c).is_ok());
        assert_eq!(recipients(&c.to).len(), 2);
        c.from = "nope".into();
        assert!(validate(&c).is_err());
        let w = Channel {
            kind: "webhook".into(),
            url: "file:///etc/passwd".into(),
            ..Default::default()
        };
        assert!(validate(&w).is_err());
    }
}
