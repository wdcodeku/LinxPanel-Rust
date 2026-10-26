//! Active defense: watches website access logs, SSH authentication logs and the
//! panel's own rejections, and bans abusive addresses in the kernel (nftables).
//!
//! Designed to stay cheap under attack: logs are read incrementally from where
//! the last tick stopped (bounded per tick), counters are fixed-size buckets,
//! and the per-IP table is capped so a spray of addresses cannot grow memory.

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::{ipban, panel_access::IpNet};
use crate::db;

pub const SETTING_DEFENSE: &str = "defense";

/// How often logs are read and thresholds checked.
pub const TICK: std::time::Duration = std::time::Duration::from_secs(5);
/// Most bytes read from one log per tick, so a huge backlog cannot stall a tick.
const MAX_READ_PER_TICK: u64 = 8 * 1024 * 1024;
/// Most addresses tracked at once.
const MAX_TRACKED: usize = 100_000;
const BUCKET_SECS: u64 = 10;
/// Successful logins trust an address for this long.
const TRUST_SECS: u64 = 24 * 3600;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct DefenseConfig {
    /// Automatic banning on/off (manual bans work regardless).
    pub enabled: bool,
    pub ban_minutes: u64,
    /// IPs / CIDRs never banned.
    pub whitelist: String,

    pub web_enabled: bool,
    pub web_window: u64,
    pub web_max_requests: u64,
    /// 4xx responses (scans for /wp-login.php, /.env, ...) count separately.
    pub web_max_errors: u64,

    pub ssh_enabled: bool,
    pub ssh_window: u64,
    pub ssh_max_failures: u64,

    pub panel_enabled: bool,
    pub panel_window: u64,
    pub panel_max_denied: u64,

    /// nginx per-IP request rate limiting on every panel-managed site.
    pub cc_enabled: bool,
    /// Requests per second per IP.
    pub cc_rate: u64,
    pub cc_burst: u64,
    /// Concurrent connections per IP.
    pub cc_conn: u64,
}

impl Default for DefenseConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ban_minutes: 60,
            whitelist: String::new(),
            web_enabled: true,
            web_window: 60,
            web_max_requests: 600,
            web_max_errors: 120,
            ssh_enabled: true,
            ssh_window: 600,
            ssh_max_failures: 6,
            panel_enabled: true,
            panel_window: 300,
            panel_max_denied: 30,
            cc_enabled: false,
            // A page load fetches dozens of assets at once; the burst absorbs that.
            cc_rate: 30,
            cc_burst: 200,
            cc_conn: 64,
        }
    }
}

impl DefenseConfig {
    pub fn validate(&self) -> Result<(), String> {
        let in_range = |value: u64, min: u64, max: u64, what: &str| {
            if (min..=max).contains(&value) {
                Ok(())
            } else {
                Err(format!("{} must be between {} and {}", what, min, max))
            }
        };
        in_range(self.ban_minutes, 0, 525_600, "Ban duration")?;
        in_range(self.web_window, 10, 3600, "Web window")?;
        in_range(self.web_max_requests, 10, 1_000_000, "Web request limit")?;
        in_range(self.web_max_errors, 5, 1_000_000, "Web error limit")?;
        in_range(self.ssh_window, 30, 86_400, "SSH window")?;
        in_range(self.ssh_max_failures, 2, 1000, "SSH failure limit")?;
        in_range(self.panel_window, 30, 86_400, "Panel window")?;
        in_range(self.panel_max_denied, 3, 10_000, "Panel limit")?;
        in_range(self.cc_rate, 1, 10_000, "CC rate")?;
        in_range(self.cc_burst, 0, 100_000, "CC burst")?;
        in_range(self.cc_conn, 1, 100_000, "CC connections")?;
        super::panel_access::parse_ip_list(&self.whitelist).map(|_| ())
    }

    fn ban_seconds(&self) -> Option<u64> {
        (self.ban_minutes > 0).then_some(self.ban_minutes * 60)
    }
}

pub async fn load_config(pool: &SqlitePool) -> DefenseConfig {
    db::get_setting(pool, SETTING_DEFENSE)
        .await
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub async fn save_config(pool: &SqlitePool, config: &DefenseConfig) -> anyhow::Result<()> {
    db::set_setting(pool, SETTING_DEFENSE, &serde_json::to_string(config)?).await
}

fn now() -> u64 {
    chrono::Utc::now().timestamp().max(0) as u64
}

// ===== Counters =====

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    WebRequest,
    WebError,
    SshFailure,
    PanelDenied,
}

impl Kind {
    fn source(self) -> &'static str {
        match self {
            Kind::WebRequest | Kind::WebError => "web",
            Kind::SshFailure => "ssh",
            Kind::PanelDenied => "panel",
        }
    }
}

/// Counts in fixed 10s buckets; the window sum is the sliding total.
#[derive(Debug, Default)]
struct Rate {
    buckets: VecDeque<(u64, u64)>,
}

impl Rate {
    fn add(&mut self, at: u64, n: u64) {
        let bucket = at - at % BUCKET_SECS;
        match self.buckets.back_mut() {
            Some((start, count)) if *start == bucket => *count += n,
            _ => self.buckets.push_back((bucket, n)),
        }
        // Nothing needs more than a day of history.
        while self.buckets.len() > (86_400 / BUCKET_SECS) as usize {
            self.buckets.pop_front();
        }
    }

    fn total(&mut self, at: u64, window: u64) -> u64 {
        let oldest = at.saturating_sub(window);
        while self
            .buckets
            .front()
            .is_some_and(|(start, _)| start + BUCKET_SECS <= oldest)
        {
            self.buckets.pop_front();
        }
        self.buckets.iter().map(|(_, n)| n).sum()
    }

    fn last(&self) -> u64 {
        self.buckets.back().map(|(start, _)| *start).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub at: i64,
    pub ip: String,
    pub source: String,
    pub action: String,
    pub detail: String,
}

/// Shared engine state; one per panel process.
pub struct Engine {
    counters: Mutex<HashMap<(IpAddr, Kind), Rate>>,
    trusted: Mutex<HashMap<IpAddr, u64>>,
    offsets: Mutex<HashMap<PathBuf, u64>>,
    journal_since: Mutex<u64>,
    pub web_log_dir: PathBuf,
    started: Instant,
}

impl Engine {
    pub fn new(data_dir: &Path) -> Arc<Self> {
        Arc::new(Self {
            counters: Mutex::new(HashMap::new()),
            trusted: Mutex::new(HashMap::new()),
            offsets: Mutex::new(HashMap::new()),
            journal_since: Mutex::new(now()),
            web_log_dir: web_log_dir(data_dir),
            started: Instant::now(),
        })
    }

    /// Records an event against an address (called from request paths: cheap).
    pub fn note(&self, ip: IpAddr, kind: Kind) {
        self.note_weighted(ip, kind, 1);
    }

    /// Like `note`, for events that weigh more than one (a failed login).
    pub fn note_weighted(&self, ip: IpAddr, kind: Kind, weight: u64) {
        self.note_n(ip.to_canonical(), kind, weight, now());
    }

    fn note_n(&self, ip: IpAddr, kind: Kind, n: u64, at: u64) {
        let mut counters = self.counters.lock().unwrap_or_else(|e| e.into_inner());
        if counters.len() >= MAX_TRACKED && !counters.contains_key(&(ip, kind)) {
            // Under a very wide spray, drop the quietest entries first.
            let cutoff = at.saturating_sub(BUCKET_SECS * 3);
            counters.retain(|_, rate| rate.last() >= cutoff);
            if counters.len() >= MAX_TRACKED {
                return;
            }
        }
        counters.entry((ip, kind)).or_default().add(at, n);
    }

    /// A successful login: never auto-ban this address for a day.
    pub fn trust(&self, ip: IpAddr) {
        self.trusted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(ip.to_canonical(), now() + TRUST_SECS);
    }

    fn is_trusted(&self, ip: &IpAddr) -> bool {
        self.trusted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(ip)
            .is_some_and(|until| *until > now())
    }

    fn forget(&self, ip: &IpAddr) {
        self.counters
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(addr, _), _| addr != ip);
    }

    /// Addresses over a threshold, with a description.
    fn offenders(&self, config: &DefenseConfig, at: u64) -> Vec<(IpAddr, Kind, String)> {
        let mut counters = self.counters.lock().unwrap_or_else(|e| e.into_inner());
        let mut found = Vec::new();
        for ((ip, kind), rate) in counters.iter_mut() {
            let (enabled, window, limit, what) = match kind {
                Kind::WebRequest => (
                    config.web_enabled,
                    config.web_window,
                    config.web_max_requests,
                    "requests",
                ),
                Kind::WebError => (
                    config.web_enabled,
                    config.web_window,
                    config.web_max_errors,
                    "error responses (scanning)",
                ),
                Kind::SshFailure => (
                    config.ssh_enabled,
                    config.ssh_window,
                    config.ssh_max_failures,
                    "failed SSH logins",
                ),
                Kind::PanelDenied => (
                    config.panel_enabled,
                    config.panel_window,
                    config.panel_max_denied,
                    "rejected panel requests",
                ),
            };
            if !enabled {
                continue;
            }
            let total = rate.total(at, window);
            if total >= limit {
                found.push((*ip, *kind, format!("{} {} in {}s", total, what, window)));
            }
        }
        // Drop idle entries while we hold the lock.
        counters.retain(|_, rate| rate.last() + 86_400 > at);
        found
    }

    pub fn tracked(&self) -> usize {
        self.counters
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    pub fn uptime_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }
}

/// Per-site access logs the defense reads (nginx writes them).
pub fn web_log_dir(data_dir: &Path) -> PathBuf {
    data_dir
        .parent()
        .unwrap_or(data_dir)
        .join("logs")
        .join("web")
}

// ===== Log parsing =====

/// `ip - user [time] "request" status ...` -> (ip, status)
pub fn parse_access_line(line: &str) -> Option<(IpAddr, u16)> {
    let ip: IpAddr = line.split_whitespace().next()?.parse().ok()?;
    let mut quoted = line.splitn(3, '"');
    quoted.next()?;
    quoted.next()?;
    let status = quoted.next()?.split_whitespace().next()?.parse().ok()?;
    Some((ip.to_canonical(), status))
}

/// The sshd message of a log line: syslog lines ("... sshd[42]: msg", also
/// sshd-session) yield "msg", other daemons nothing; journal output (`-o cat`)
/// is the message already.
fn ssh_message(line: &str) -> &str {
    match line.find("]: ") {
        Some(i) if line[..i].contains("sshd") => &line[i + 3..],
        Some(_) => "",
        None => line,
    }
}

/// SSH log line -> (ip, failed?) for lines worth counting. Accepted logins
/// return `false` so the address can be trusted.
///
/// Usernames and client banners are attacker-controlled and appear in these
/// lines, so markers must start the message and the address is taken from
/// sshd's own "<ip> port <n>" trailer, never from the first IP-looking word -
/// otherwise logging in as user "1.2.3.4" would get 1.2.3.4 banned.
pub fn parse_ssh_line(line: &str) -> Option<(IpAddr, bool)> {
    // One marker per attempt where possible: sshd logs several lines per failed
    // login (pam "authentication failure", "Disconnected from ..."), which are
    // left out so an attempt is not counted three times.
    const FAILURES: &[&str] = &[
        "Failed password for ",
        "Invalid user ",
        // Key-only servers: password bots log only this.
        "Connection closed by authenticating user ",
        "Connection closed by invalid user ",
        "maximum authentication attempts exceeded for ",
        "Did not receive identification string from ",
        "banner exchange: Connection from ",
        "Bad protocol version identification ",
    ];
    const ACCEPTED: &[&str] = &["Accepted password for ", "Accepted publickey for "];
    const NEGOTIATE: &str = "Unable to negotiate with ";

    let message = ssh_message(line.trim_end());
    let message = message.strip_prefix("error: ").unwrap_or(message);
    let parse = |token: &str| token.parse::<IpAddr>().ok().map(|ip| ip.to_canonical());

    // "Unable to negotiate with <ip> port <n>: <client's algorithm list>"
    if let Some(rest) = message.strip_prefix(NEGOTIATE) {
        return Some((parse(rest.split_whitespace().next()?)?, true));
    }
    let failed = FAILURES.iter().any(|marker| message.starts_with(marker));
    if !failed && !ACCEPTED.iter().any(|marker| message.starts_with(marker)) {
        return None;
    }
    // The last " port " is sshd's: client-controlled text only comes before it.
    let before = &message[..message.rfind(" port ")?];
    Some((parse(before.rsplit(' ').next()?)?, failed))
}

/// Reads what was appended to `path` since the last call. The first sighting of
/// a file starts at its end (history is not replayed); truncation restarts at 0.
fn read_new(offsets: &Mutex<HashMap<PathBuf, u64>>, path: &Path) -> Option<String> {
    let len = std::fs::metadata(path).ok()?.len();
    let mut offsets = offsets.lock().unwrap_or_else(|e| e.into_inner());
    let offset = offsets.entry(path.to_path_buf()).or_insert(len);
    if len < *offset {
        *offset = 0;
    }
    if len == *offset {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    file.seek(SeekFrom::Start(*offset)).ok()?;
    let mut buffer = Vec::new();
    file.take(MAX_READ_PER_TICK).read_to_end(&mut buffer).ok()?;
    // Only whole lines; a partial last line is read next time.
    let end = buffer.iter().rposition(|b| *b == b'\n').map(|i| i + 1)?;
    buffer.truncate(end);
    *offset += end as u64;
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

fn auth_log() -> Option<PathBuf> {
    ["/var/log/auth.log", "/var/log/secure"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

/// Reads new log lines and feeds the counters. Blocking (file and journal IO).
fn collect(engine: &Engine, config: &DefenseConfig) {
    let at = now();
    if config.web_enabled {
        if let Ok(entries) = std::fs::read_dir(&engine.web_log_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("log") {
                    continue;
                }
                let Some(text) = read_new(&engine.offsets, &path) else {
                    continue;
                };
                let mut per_ip: HashMap<IpAddr, (u64, u64)> = HashMap::new();
                for (ip, status) in text.lines().filter_map(parse_access_line) {
                    let entry = per_ip.entry(ip).or_default();
                    entry.0 += 1;
                    if (400..500).contains(&status) {
                        entry.1 += 1;
                    }
                }
                for (ip, (requests, errors)) in per_ip {
                    engine.note_n(ip, Kind::WebRequest, requests, at);
                    if errors > 0 {
                        engine.note_n(ip, Kind::WebError, errors, at);
                    }
                }
            }
        }
    }

    if config.ssh_enabled {
        let text = match auth_log() {
            Some(path) => read_new(&engine.offsets, &path),
            None => journal_ssh_lines(engine),
        };
        for (ip, failed) in text
            .iter()
            .flat_map(|t| t.lines())
            .filter_map(parse_ssh_line)
        {
            if failed {
                engine.note_n(ip, Kind::SshFailure, 1, at);
            } else {
                engine.trust(ip);
            }
        }
    }
}

/// On journald-only systems: sshd messages since the previous call.
fn journal_ssh_lines(engine: &Engine) -> Option<String> {
    if !super::shell::has_binary("journalctl") {
        return None;
    }
    let since = {
        let mut since = engine
            .journal_since
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let previous = *since;
        *since = now();
        previous
    };
    let since_arg = format!("@{}", since);
    let output = super::shell::run_best_effort(
        "journalctl",
        &[
            "-q",
            "--no-pager",
            "-o",
            "cat",
            "-u",
            "ssh",
            "-u",
            "sshd",
            "--since",
            &since_arg,
        ],
    )
    .ok()?;
    output.ok().then_some(output.stdout)
}

// ===== Bans =====

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct BanRecord {
    pub ip: String,
    pub source: String,
    pub reason: String,
    pub created_at: i64,
    /// Unix time; 0 = permanent.
    pub expires_at: i64,
}

/// Addresses that must never be banned: loopback, this server's own
/// addresses, the configured whitelist and recently logged-in admins.
fn protected(engine: &Engine, config: &DefenseConfig, ip: &IpAddr) -> Option<&'static str> {
    if ip.is_loopback() || ip.is_unspecified() {
        return Some("loopback address");
    }
    if local_addresses().contains(ip) {
        return Some("this server's own address");
    }
    let whitelist = super::panel_access::parse_ip_list(&config.whitelist).unwrap_or_default();
    if whitelist.iter().any(|net: &IpNet| net.contains(*ip)) {
        return Some("whitelisted");
    }
    if engine.is_trusted(ip) {
        return Some("recently logged in to the panel or SSH");
    }
    None
}

fn local_addresses() -> Vec<IpAddr> {
    let mut found = Vec::new();
    if let Ok(text) = std::fs::read_to_string("/proc/net/fib_trie") {
        // "/32 host LOCAL" entries follow the address line.
        let lines: Vec<&str> = text.lines().collect();
        for pair in lines.windows(2) {
            if pair[1].contains("/32 host LOCAL") {
                if let Some(ip) = pair[0]
                    .split_whitespace()
                    .last()
                    .and_then(|s| s.parse().ok())
                {
                    found.push(ip);
                }
            }
        }
    }
    if let Ok(text) = std::fs::read_to_string("/proc/net/if_inet6") {
        for line in text.lines() {
            if let Some(hex) = line.split_whitespace().next().filter(|h| h.len() == 32) {
                let bytes: Vec<u8> = (0..16)
                    .filter_map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
                    .collect();
                if let Ok(octets) = <[u8; 16]>::try_from(bytes) {
                    found.push(IpAddr::from(octets));
                }
            }
        }
    }
    found
}

pub async fn log_event(pool: &SqlitePool, ip: &str, source: &str, action: &str, detail: &str) {
    let _ = sqlx::query(
        "INSERT INTO defense_events (at, ip, source, action, detail) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(now() as i64)
    .bind(ip)
    .bind(source)
    .bind(action)
    .bind(detail)
    .execute(pool)
    .await;
}

/// Bans an address in the kernel and records it. `seconds: None` is permanent.
pub async fn ban(
    pool: &SqlitePool,
    ip: IpAddr,
    seconds: Option<u64>,
    source: &str,
    reason: &str,
) -> Result<(), String> {
    let ip = ip.to_canonical();
    tokio::task::spawn_blocking(move || ipban::ban(ip, seconds))
        .await
        .map_err(|e| e.to_string())??;
    let at = now() as i64;
    let expires_at = seconds.map(|s| at + s as i64).unwrap_or(0);
    sqlx::query(
        "INSERT INTO ip_bans (ip, source, reason, created_at, expires_at) VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(ip) DO UPDATE SET source = excluded.source, reason = excluded.reason,
           created_at = excluded.created_at, expires_at = excluded.expires_at",
    )
    .bind(ip.to_string())
    .bind(source)
    .bind(reason)
    .bind(at)
    .bind(expires_at)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    log_event(pool, &ip.to_string(), source, "ban", reason).await;
    tracing::warn!("Banned {} ({}: {})", ip, source, reason);
    Ok(())
}

pub async fn unban(pool: &SqlitePool, ip: IpAddr) -> Result<(), String> {
    let ip = ip.to_canonical();
    tokio::task::spawn_blocking(move || ipban::unban(ip))
        .await
        .map_err(|e| e.to_string())??;
    sqlx::query("DELETE FROM ip_bans WHERE ip = ?")
        .bind(ip.to_string())
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    log_event(pool, &ip.to_string(), "manual", "unban", "").await;
    Ok(())
}

pub async fn list_bans(pool: &SqlitePool) -> Vec<BanRecord> {
    sqlx::query_as(
        "SELECT ip, source, reason, created_at, expires_at FROM ip_bans
         WHERE expires_at = 0 OR expires_at > ? ORDER BY created_at DESC",
    )
    .bind(now() as i64)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
}

pub async fn recent_events(pool: &SqlitePool, limit: i64) -> Vec<Event> {
    let rows: Vec<(i64, String, String, String, String)> = sqlx::query_as(
        "SELECT at, ip, source, action, detail FROM defense_events ORDER BY id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.into_iter()
        .map(|(at, ip, source, action, detail)| Event {
            at,
            ip,
            source,
            action,
            detail,
        })
        .collect()
}

/// Drops expired records and puts back kernel bans lost to a reboot or a
/// firewall reload.
pub async fn reconcile(pool: &SqlitePool) {
    let at = now() as i64;
    let _ = sqlx::query("DELETE FROM ip_bans WHERE expires_at > 0 AND expires_at <= ?")
        .bind(at)
        .execute(pool)
        .await;
    let _ = sqlx::query(
        "DELETE FROM defense_events WHERE id <= (SELECT id FROM defense_events ORDER BY id DESC LIMIT 1 OFFSET 2000)",
    )
    .execute(pool)
    .await;
    let bans = list_bans(pool).await;
    if bans.is_empty() || !ipban::available() {
        return;
    }
    let _ = tokio::task::spawn_blocking(move || {
        ipban::ensure_table()?;
        let present: std::collections::HashSet<String> =
            ipban::list()?.into_iter().map(|b| b.ip).collect();
        for record in bans.iter().filter(|b| !present.contains(&b.ip)) {
            let Ok(ip) = record.ip.parse::<IpAddr>() else {
                continue;
            };
            let seconds = (record.expires_at > 0).then(|| (record.expires_at - at).max(1) as u64);
            let _ = ipban::ban(ip, seconds);
        }
        Ok::<_, String>(())
    })
    .await;
}

/// The engine loop: collect, judge, ban; reconcile every minute.
pub async fn run(
    engine: Arc<Engine>,
    pool: SqlitePool,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    reconcile(&pool).await;
    let mut ticks: u64 = 0;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(TICK) => {}
            _ = shutdown.changed() => return,
        }
        ticks += 1;
        let config = load_config(&pool).await;
        if ticks % 12 == 0 {
            reconcile(&pool).await;
        }
        if !config.enabled {
            continue;
        }
        let collector = engine.clone();
        let collect_config = config.clone();
        let _ = tokio::task::spawn_blocking(move || collect(&collector, &collect_config)).await;

        let mut handled = std::collections::HashSet::new();
        for (ip, kind, reason) in engine.offenders(&config, now()) {
            // Several counters can trip at once (requests and errors): act once.
            if !handled.insert(ip) {
                continue;
            }
            if let Some(why) = protected(&engine, &config, &ip) {
                // Count once per window, not every tick.
                engine.forget(&ip);
                log_event(
                    &pool,
                    &ip.to_string(),
                    kind.source(),
                    "skip",
                    &format!("{} - not banned: {}", reason, why),
                )
                .await;
                continue;
            }
            engine.forget(&ip);
            if let Err(err) = ban(&pool, ip, config.ban_seconds(), kind.source(), &reason).await {
                tracing::warn!("Could not ban {}: {}", ip, err);
                log_event(&pool, &ip.to_string(), kind.source(), "error", &err).await;
            }
        }
    }
}

// ===== Kernel network hardening =====

pub const SYSCTL_PATH: &str = "/etc/sysctl.d/90-linxpanel.conf";

/// SYN-flood resistance and spoofing protection. Every value is safe on a
/// single-purpose web server; reverse-path filtering is "loose" (2) so cloud
/// VMs with several interfaces or asymmetric routes keep working.
pub const SYSCTL_PROFILE: &str = "\
# Generated by LinxPanel (Security > Defense)
# SYN floods: answer with cookies instead of filling the backlog.
net.ipv4.tcp_syncookies = 1
net.ipv4.tcp_max_syn_backlog = 8192
net.ipv4.tcp_synack_retries = 2
net.core.somaxconn = 4096
net.core.netdev_max_backlog = 8192
# Reclaim half-closed and dead connections sooner.
net.ipv4.tcp_fin_timeout = 15
net.ipv4.tcp_rfc1337 = 1
# Spoofing and redirect tricks.
net.ipv4.conf.all.rp_filter = 2
net.ipv4.conf.default.rp_filter = 2
net.ipv4.conf.all.accept_redirects = 0
net.ipv4.conf.default.accept_redirects = 0
net.ipv6.conf.all.accept_redirects = 0
net.ipv4.conf.all.send_redirects = 0
net.ipv4.conf.all.accept_source_route = 0
net.ipv6.conf.all.accept_source_route = 0
net.ipv4.icmp_echo_ignore_broadcasts = 1
net.ipv4.icmp_ignore_bogus_error_responses = 1
";

#[derive(Debug, Clone, Serialize)]
pub struct SysctlStatus {
    /// Our profile file is installed.
    pub applied: bool,
    pub syncookies: bool,
    pub somaxconn: u64,
}

fn read_proc(key: &str) -> Option<u64> {
    std::fs::read_to_string(format!("/proc/sys/{}", key.replace('.', "/")))
        .ok()?
        .trim()
        .parse()
        .ok()
}

pub fn sysctl_status() -> SysctlStatus {
    SysctlStatus {
        applied: Path::new(SYSCTL_PATH).exists(),
        syncookies: read_proc("net.ipv4.tcp_syncookies") == Some(1),
        somaxconn: read_proc("net.core.somaxconn").unwrap_or(0),
    }
}

/// Installs the profile and loads it. Keys the kernel lacks (no IPv6, inside
/// a container) are skipped by `sysctl -e`.
pub fn apply_sysctl() -> Result<String, String> {
    if let Some(dir) = Path::new(SYSCTL_PATH).parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(SYSCTL_PATH, SYSCTL_PROFILE)
        .map_err(|e| format!("Cannot write {}: {}", SYSCTL_PATH, e))?;
    let output = super::shell::run_privileged("sysctl", &["-e", "-p", SYSCTL_PATH])?;
    // Read-only keys (containers) fail individually; report, don't fail.
    let mut text = output.stdout;
    if !output.stderr.trim().is_empty() {
        text.push_str(&output.stderr);
    }
    Ok(text)
}

pub fn remove_sysctl() -> Result<(), String> {
    match std::fs::remove_file(SYSCTL_PATH) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

/// Addresses an operator may not ban by hand: loopback and this server's own.
pub fn never_ban(ip: &IpAddr) -> Option<&'static str> {
    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return Some("loopback or special address");
    }
    local_addresses()
        .contains(ip)
        .then_some("this server's own address")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_lines_parse() {
        let line = r#"203.0.113.9 - - [26/Oct/2026:15:00:00 +0000] "GET /wp-login.php HTTP/1.1" 404 153 "-" "curl/8" 0.001"#;
        assert_eq!(
            parse_access_line(line),
            Some(("203.0.113.9".parse().unwrap(), 404))
        );
        let line = r#"2001:db8::1 - - [x] "GET / HTTP/2.0" 200 5 "-" "ua" 0.1"#;
        assert_eq!(parse_access_line(line).unwrap().1, 200);
        assert_eq!(parse_access_line("garbage"), None);
        assert_eq!(
            parse_access_line(r#"1.2.3.4 - - [x] "GET \" / " 200"#).map(|x| x.1),
            None
        );
    }

    #[test]
    fn ssh_lines_parse() {
        let cases = [
            ("Failed password for root from 203.0.113.5 port 52011 ssh2", Some(true)),
            ("Oct 26 17:40:01 vm sshd[991]: Failed password for root from 203.0.113.5 port 52011 ssh2", Some(true)),
            ("Invalid user admin from 198.51.100.7 port 4444", Some(true)),
            ("pam_unix(sshd:auth): authentication failure; logname= uid=0 euid=0 tty=ssh ruser= rhost=203.0.113.6  user=root", None),
            ("Disconnected from invalid user admin 198.51.100.7 port 4444 [preauth]", None),
            ("Connection closed by authenticating user root 203.0.113.8 port 5555 [preauth]", Some(true)),
            ("error: maximum authentication attempts exceeded for root from 203.0.113.8 port 1 ssh2 [preauth]", Some(true)),
            ("Unable to negotiate with 203.0.113.9 port 2: no matching key exchange method found. Their offer: x port y", Some(true)),
            ("Accepted publickey for ops from 192.0.2.10 port 6000 ssh2: ED25519", Some(false)),
            ("Server listening on 0.0.0.0 port 22.", None),
            ("Received disconnect from 203.0.113.5 port 1: Bye", None),
            // Another daemon in auth.log.
            ("Oct 26 17:40:01 vm sudo[5]: Failed password for root from 203.0.113.5 port 1", None),
        ];
        for (line, expected) in cases {
            assert_eq!(
                parse_ssh_line(line).map(|(_, failed)| failed),
                expected,
                "{line}"
            );
        }
        assert_eq!(
            parse_ssh_line("Failed password for root from 203.0.113.5 port 52011 ssh2")
                .unwrap()
                .0,
            "203.0.113.5".parse::<IpAddr>().unwrap()
        );
    }

    #[test]
    fn ssh_usernames_cannot_steer_bans_or_trust() {
        let attacker: IpAddr = "203.0.113.66".parse().unwrap();
        // The username is an IP (the admin's) or carries a fake trailer.
        for line in [
            "Invalid user 192.0.2.10 from 203.0.113.66 port 4444",
            "Failed password for invalid user 192.0.2.10 from 203.0.113.66 port 4444 ssh2",
            "Invalid user x 192.0.2.10 port 1 from 203.0.113.66 port 4444",
            "Connection closed by invalid user 192.0.2.10 203.0.113.66 port 4444 [preauth]",
            "Bad protocol version identification '192.0.2.10 port 9' from 203.0.113.66 port 4444",
        ] {
            assert_eq!(parse_ssh_line(line), Some((attacker, true)), "{line}");
        }
        // A username that reads like a success line must not earn trust.
        assert_eq!(
            parse_ssh_line("Invalid user Accepted publickey for x from 203.0.113.66 port 4444"),
            Some((attacker, true))
        );
        assert_eq!(parse_ssh_line("Invalid user 192.0.2.10"), None);
    }

    #[test]
    fn sliding_window_counts() {
        let mut rate = Rate::default();
        rate.add(1000, 5);
        rate.add(1005, 5);
        rate.add(1030, 3);
        assert_eq!(rate.total(1035, 60), 13);
        assert_eq!(
            rate.total(1065, 60),
            13,
            "a bucket counts while it overlaps the window"
        );
        assert_eq!(rate.total(1075, 60), 3, "first bucket aged out");
        assert_eq!(rate.total(1200, 60), 0);
    }

    #[test]
    fn offenders_and_protection() {
        let dir = std::env::temp_dir().join(format!("lp-def-{}", uuid::Uuid::new_v4()));
        let engine = Engine::new(&dir);
        let config = DefenseConfig {
            web_max_requests: 100,
            ssh_max_failures: 3,
            whitelist: "198.51.100.0/24".into(),
            ..DefenseConfig::default()
        };
        let at = now();
        let attacker: IpAddr = "203.0.113.50".parse().unwrap();
        engine.note_n(attacker, Kind::WebRequest, 150, at);
        engine.note_n("203.0.113.51".parse().unwrap(), Kind::WebRequest, 20, at);
        for _ in 0..3 {
            engine.note("203.0.113.52".parse().unwrap(), Kind::SshFailure);
        }
        let mut found: Vec<IpAddr> = engine
            .offenders(&config, at)
            .into_iter()
            .map(|o| o.0)
            .collect();
        found.sort();
        assert_eq!(found, vec![attacker, "203.0.113.52".parse().unwrap()]);

        assert!(protected(&engine, &config, &"127.0.0.1".parse().unwrap()).is_some());
        assert!(protected(&engine, &config, &"198.51.100.20".parse().unwrap()).is_some());
        assert!(protected(&engine, &config, &attacker).is_none());
        engine.trust(attacker);
        assert!(protected(&engine, &config, &attacker).is_some());

        let disabled = DefenseConfig {
            web_enabled: false,
            ..config.clone()
        };
        assert!(engine
            .offenders(&disabled, at)
            .iter()
            .all(|o| o.1 != Kind::WebRequest));
    }

    #[test]
    fn incremental_reading() {
        let dir = std::env::temp_dir().join(format!("lp-log-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.log");
        std::fs::write(&path, "old line\n").unwrap();
        let offsets = Mutex::new(HashMap::new());
        assert_eq!(read_new(&offsets, &path), None, "history is skipped");
        std::fs::write(&path, "old line\nnew 1\npartial").unwrap();
        assert_eq!(read_new(&offsets, &path).as_deref(), Some("new 1\n"));
        std::fs::write(&path, "x\n").unwrap();
        assert_eq!(
            read_new(&offsets, &path).as_deref(),
            Some("x\n"),
            "truncation restarts"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn config_validation() {
        assert!(DefenseConfig::default().validate().is_ok());
        assert!(DefenseConfig {
            ssh_max_failures: 1,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(DefenseConfig {
            whitelist: "nope".into(),
            ..Default::default()
        }
        .validate()
        .is_err());
    }
}
