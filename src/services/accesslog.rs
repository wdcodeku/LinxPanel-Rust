//! Website access logs: rotation and analysis.
//!
//! OpenResty writes one log per site (`site-<id>.log`, format `linxpanel`).
//! Rotation renames it to `site-<id>.log-<date>-<time>` once a day (or when it
//! outgrows the size limit), tells OpenResty to reopen its logs, compresses
//! the old file a few minutes later (when nothing writes to it any more) and
//! deletes archives past the retention period. Renamed files no longer end in
//! `.log`, so the defense engine, which reads `*.log`, skips them.
//!
//! Analysis reads the current log and the archives that can hold the chosen
//! time range (plain or gzip) and builds a report: visits, visitors,
//! traffic, status codes, a timeline, top pages / visitors / referrers,
//! browsers and systems, search engines and bots, errors, slow requests and
//! requests that look like attacks.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use chrono::{Datelike, Local, NaiveDate, TimeZone, Timelike};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::AppState;

pub const SETTING_KEEP_DAYS: &str = "access_log_keep_days";
pub const SETTING_MAX_MB: &str = "access_log_max_mb";
const SETTING_ROTATED_ON: &str = "access_log_rotated_on";
pub const DEFAULT_KEEP_DAYS: u32 = 30;
pub const DEFAULT_MAX_MB: u32 = 200;
/// A rotated file is compressed once nothing has written to it for this long
/// (OpenResty may take a moment to reopen its logs).
const COMPRESS_AFTER: Duration = Duration::from_secs(300);

// ===== Settings =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Days archives are kept (1-365).
    pub keep_days: u32,
    /// A log is rotated early once it reaches this size (MB, 10-10240).
    pub max_mb: u32,
}

pub async fn load_settings(db: &SqlitePool) -> Settings {
    let read = |key: &'static str, default: u32| async move {
        crate::db::get_setting(db, key)
            .await
            .ok()
            .flatten()
            .and_then(|v| v.trim().parse::<u32>().ok())
            .unwrap_or(default)
    };
    Settings {
        keep_days: read(SETTING_KEEP_DAYS, DEFAULT_KEEP_DAYS)
            .await
            .clamp(1, 365),
        max_mb: read(SETTING_MAX_MB, DEFAULT_MAX_MB).await.clamp(10, 10240),
    }
}

pub fn validate(settings: &Settings) -> Result<(), String> {
    if !(1..=365).contains(&settings.keep_days) {
        return Err("Keep logs for 1 to 365 days".into());
    }
    if !(10..=10240).contains(&settings.max_mb) {
        return Err("The size limit is 10 to 10240 MB".into());
    }
    Ok(())
}

// ===== Files =====

pub fn log_dir(state: &AppState) -> PathBuf {
    super::defense::web_log_dir(&state.data_dir)
}

/// The current log of a site.
pub fn current_log(dir: &Path, site_id: i64) -> PathBuf {
    dir.join(format!("site-{}.log", site_id))
}

/// Whether `name` is a rotated log: the panel's `site-3.log-20261107-000501`
/// (`.gz`), or logrotate's `site-3.log.1` / `site-3.log.2.gz` from versions
/// that rotated with logrotate.
fn is_archive(name: &str) -> bool {
    if name.ends_with(".tmp") {
        return false;
    }
    if name.contains(".log-") {
        return true;
    }
    name.split_once(".log.").is_some_and(|(_, rest)| {
        let number = rest.strip_suffix(".gz").unwrap_or(rest);
        !number.is_empty() && number.chars().all(|c| c.is_ascii_digit())
    })
}

/// Rotated files of `base` ("site-3.log"), oldest first.
fn archives(dir: &Path, base: &str) -> Vec<PathBuf> {
    let mut files: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_prefix(base)
                .is_some_and(|rest| rest.starts_with('-') || rest.starts_with('.'))
                && is_archive(&name)
        })
        .map(|e| (mtime(&e.path()).unwrap_or(SystemTime::UNIX_EPOCH), e.path()))
        .collect();
    files.sort();
    files.into_iter().map(|(_, path)| path).collect()
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Usage {
    /// Bytes of the current log.
    pub current: u64,
    /// Bytes of rotated logs (mostly compressed).
    pub archived: u64,
    pub archives: usize,
    /// Unix time of the oldest archive.
    pub oldest: Option<i64>,
}

pub fn usage(dir: &Path, site_id: i64) -> Usage {
    let current = current_log(dir, site_id);
    let base = format!("site-{}.log", site_id);
    let mut usage = Usage {
        current: std::fs::metadata(&current).map(|m| m.len()).unwrap_or(0),
        ..Default::default()
    };
    for path in archives(dir, &base) {
        usage.archived += std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        usage.archives += 1;
        let at = mtime(&path).map(unix);
        usage.oldest = match (usage.oldest, at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }
    usage
}

fn unix(time: SystemTime) -> i64 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ===== Rotation =====

/// Background job: checks every ten minutes.
pub async fn run(state: Arc<AppState>, mut shutdown: tokio::sync::watch::Receiver<bool>) {
    // Give the panel a moment to start first.
    tokio::select! {
        _ = tokio::time::sleep(Duration::from_secs(60)) => {}
        _ = shutdown.changed() => return,
    }
    loop {
        if let Err(err) = maintain(&state).await {
            tracing::warn!("Access log rotation: {}", err);
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(600)) => {}
            _ = shutdown.changed() => break,
        }
    }
}

/// Rotates what is due, compresses what was rotated, deletes what expired.
pub async fn maintain(state: &AppState) -> Result<(), String> {
    let settings = load_settings(&state.db).await;
    let dir = log_dir(state);
    if !dir.is_dir() {
        return Ok(());
    }
    let today = Local::now().format("%Y-%m-%d").to_string();
    let last = crate::db::get_setting(&state.db, SETTING_ROTATED_ON)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    let daily = last != today;
    let limit = settings.max_mb as u64 * 1024 * 1024;
    let rotated = {
        let dir = dir.clone();
        tokio::task::spawn_blocking(move || rotate_files(&dir, daily, limit))
            .await
            .map_err(|e| e.to_string())?
    };
    if rotated > 0 {
        // Without this OpenResty keeps writing to the renamed files.
        if let Err(err) = super::openresty::reopen_logs(&state.db).await {
            tracing::warn!("Access log rotation: cannot reopen logs: {}", err);
        }
        tracing::info!("Access logs: rotated {} file(s)", rotated);
    }
    if daily {
        let _ = crate::db::set_setting(&state.db, SETTING_ROTATED_ON, &today).await;
    }
    let keep = Duration::from_secs(settings.keep_days as u64 * 86_400);
    tokio::task::spawn_blocking(move || {
        compress_rotated(&dir);
        expire(&dir, keep);
    })
    .await
    .map_err(|e| e.to_string())
}

/// Renames current logs: all non-empty ones on a new day, otherwise only
/// those over `limit` bytes. Returns how many were renamed.
fn rotate_files(dir: &Path, daily: bool, limit: u64) -> usize {
    let stamp = Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut count = 0;
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("log") {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if size == 0 || (!daily && size < limit) {
            continue;
        }
        let target = PathBuf::from(format!("{}-{}", path.display(), stamp));
        if target.exists() {
            continue;
        }
        if std::fs::rename(&path, &target).is_ok() {
            count += 1;
        }
    }
    count
}

/// Gzips rotated files nothing writes to any more.
fn compress_rotated(dir: &Path) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if !is_archive(&name) || name.ends_with(".gz") {
            continue;
        }
        let idle = mtime(&path)
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age >= COMPRESS_AFTER);
        if idle {
            if let Err(err) = gzip_file(&path) {
                tracing::warn!("Cannot compress {}: {}", path.display(), err);
            }
        }
    }
}

fn gzip_file(path: &Path) -> std::io::Result<()> {
    let gz = PathBuf::from(format!("{}.gz", path.display()));
    let tmp = PathBuf::from(format!("{}.gz.tmp", path.display()));
    {
        let mut input = std::fs::File::open(path)?;
        let output = std::fs::File::create(&tmp)?;
        let mut encoder = flate2::write::GzEncoder::new(output, flate2::Compression::default());
        std::io::copy(&mut input, &mut encoder)?;
        encoder.finish()?.sync_all()?;
    }
    // Keep the original time: analysis picks archives by it.
    if let Ok(meta) = std::fs::metadata(path) {
        if let Ok(modified) = meta.modified() {
            let _ = std::fs::File::options()
                .write(true)
                .open(&tmp)
                .and_then(|f| f.set_modified(modified));
        }
    }
    std::fs::rename(&tmp, &gz)?;
    std::fs::remove_file(path)
}

fn expire(dir: &Path, keep: Duration) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !is_archive(&name) {
            continue;
        }
        let old = mtime(&entry.path())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > keep);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

// ===== Parsing =====

/// One request from a `linxpanel` format line.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry<'a> {
    pub ip: &'a str,
    /// Unix time.
    pub time: i64,
    pub method: &'a str,
    /// Path and query, as requested.
    pub url: &'a str,
    pub status: u16,
    pub bytes: u64,
    pub referer: &'a str,
    pub agent: &'a str,
    /// Milliseconds; None for logs written before the field existed.
    pub ms: Option<u32>,
}

/// `07/Nov/2026:23:04:35 +0800` -> Unix time.
fn parse_time(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    if b.len() < 26 {
        return None;
    }
    let num = |range: std::ops::Range<usize>| -> Option<u32> { text.get(range)?.parse().ok() };
    let day = num(0..2)?;
    let month = match text.get(3..6)? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year = num(7..11)? as i32;
    let (h, m, s) = (num(12..14)?, num(15..17)?, num(18..20)?);
    let sign = match b[21] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let offset = (num(22..24)? * 3600 + num(24..26)? * 60) as i64 * sign;
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    let local = date.and_hms_opt(h, m, s)?.and_utc().timestamp();
    Some(local - offset)
}

/// The text up to the next `"` that ends a quoted field (nginx escapes
/// quotes inside values as `\x22`).
fn quoted(rest: &str) -> Option<(&str, &str)> {
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some((&rest[..end], rest[end + 1..].trim_start()))
}

pub fn parse_line(line: &str) -> Option<Entry<'_>> {
    let (ip, rest) = line.split_once(' ')?;
    let open = rest.find('[')?;
    let close = open + rest[open..].find(']')?;
    let time = parse_time(&rest[open + 1..close])?;
    let rest = rest[close + 1..].trim_start();
    let (request, rest) = quoted(rest)?;
    let mut parts = request.splitn(3, ' ');
    let method = parts.next().unwrap_or("");
    let url = parts.next().unwrap_or("");
    let mut fields = rest.splitn(3, ' ');
    let status = fields.next()?.parse().ok()?;
    let bytes = fields.next()?.parse().unwrap_or(0);
    let rest = fields.next().unwrap_or("");
    let (referer, rest) = quoted(rest).unwrap_or(("-", ""));
    let (agent, rest) = quoted(rest).unwrap_or(("-", ""));
    let ms = rest
        .split_whitespace()
        .next()
        .and_then(|t| t.parse::<f64>().ok())
        .map(|secs| (secs * 1000.0).round().min(u32::MAX as f64) as u32);
    Some(Entry {
        ip,
        time,
        method,
        url,
        status,
        bytes,
        referer,
        agent,
        ms,
    })
}

// ===== Classification =====

/// Search engines and other crawlers, matched in lower case.
const SPIDERS: &[(&str, &str)] = &[
    ("googlebot", "Googlebot"),
    ("baiduspider", "Baiduspider"),
    ("bingbot", "Bingbot"),
    ("yandex", "YandexBot"),
    ("sogou", "Sogou"),
    ("360spider", "360Spider"),
    ("haosouspider", "360Spider"),
    ("bytespider", "Bytespider"),
    ("yisouspider", "YisouSpider"),
    ("petalbot", "PetalBot"),
    ("duckduckbot", "DuckDuckBot"),
    ("applebot", "Applebot"),
    ("gptbot", "GPTBot"),
    ("claudebot", "ClaudeBot"),
    ("ccbot", "CCBot"),
    ("ahrefsbot", "AhrefsBot"),
    ("semrushbot", "SemrushBot"),
    ("mj12bot", "MJ12bot"),
    ("dotbot", "DotBot"),
    ("facebookexternalhit", "Facebook"),
    ("twitterbot", "Twitterbot"),
];

/// Agents of scripts and tools rather than people.
const TOOLS: &[&str] = &[
    "bot",
    "spider",
    "crawl",
    "slurp",
    "curl/",
    "wget/",
    "python",
    "go-http-client",
    "java/",
    "okhttp",
    "scrapy",
    "headless",
    "httpclient",
    "libwww",
    "masscan",
    "zgrab",
    "nmap",
    "nikto",
    "sqlmap",
];

/// Search engine / crawler name, or "" for people.
fn spider(agent_lower: &str) -> Option<&'static str> {
    SPIDERS
        .iter()
        .find(|(needle, _)| agent_lower.contains(needle))
        .map(|(_, name)| *name)
}

fn is_bot(agent_lower: &str) -> bool {
    agent_lower == "-" || agent_lower.is_empty() || TOOLS.iter().any(|t| agent_lower.contains(t))
}

fn browser(a: &str) -> &'static str {
    if a.contains("micromessenger") {
        "WeChat"
    } else if a.contains("edg/") || a.contains("edge/") {
        "Edge"
    } else if a.contains("opr/") || a.contains("opera") {
        "Opera"
    } else if a.contains("qqbrowser") {
        "QQ Browser"
    } else if a.contains("ucbrowser") {
        "UC Browser"
    } else if a.contains("firefox/") {
        "Firefox"
    } else if a.contains("chrome/") || a.contains("crios/") {
        "Chrome"
    } else if a.contains("safari/") {
        "Safari"
    } else if a.contains("msie") || a.contains("trident/") {
        "Internet Explorer"
    } else {
        "Other"
    }
}

fn system(a: &str) -> &'static str {
    if a.contains("android") {
        "Android"
    } else if a.contains("iphone") || a.contains("ipad") || a.contains("ipod") {
        "iOS"
    } else if a.contains("windows") {
        "Windows"
    } else if a.contains("mac os x") || a.contains("macintosh") {
        "macOS"
    } else if a.contains("harmonyos") {
        "HarmonyOS"
    } else if a.contains("linux") {
        "Linux"
    } else {
        "Other"
    }
}

/// What an attack-looking request tries, matched on the lower-cased URL
/// (also after decoding %xx).
const ATTACKS: &[(&str, &[&str])] = &[
    (
        "sql",
        &[
            "union select",
            "union all select",
            "select+",
            "information_schema",
            "sleep(",
            "benchmark(",
            "' or '1'='1",
            "\" or \"1\"=\"1",
            "or 1=1",
            "extractvalue(",
            "updatexml(",
            "waitfor delay",
        ],
    ),
    (
        "xss",
        &[
            "<script",
            "javascript:",
            "onerror=",
            "onload=",
            "<svg",
            "<iframe",
            "alert(",
        ],
    ),
    (
        "traversal",
        &[
            "../",
            "..\\",
            "/etc/passwd",
            "/etc/shadow",
            "win.ini",
            "boot.ini",
            "/proc/self/",
        ],
    ),
    (
        "rce",
        &[
            "${jndi:",
            "eval(",
            "base64_decode",
            "system(",
            "exec(",
            "passthru(",
            "shell_exec",
            "/bin/sh",
            "/bin/bash",
            "cmd.exe",
            "phpinfo(",
            "allow_url_include",
            "auto_prepend_file",
            "think\\app",
            "invokefunction",
        ],
    ),
    (
        "scan",
        &[
            "/.env",
            "/.git/",
            "/.svn/",
            "/.hg/",
            "/.ds_store",
            "/.aws/",
            "/.ssh/",
            "wp-config.php.",
            "/phpmyadmin",
            "/pma/",
            "/adminer",
            "/vendor/phpunit",
            "/cgi-bin/",
            "/actuator",
            "/.well-known/security",
            "/solr/",
            "/boaform/",
            "/hnap1",
            "/web.config",
            "/server-status",
            ".bak",
            ".sql",
            ".swp",
        ],
    ),
];

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        out.push(v);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn attack_kind(url: &str) -> Option<&'static str> {
    let lower = url.to_ascii_lowercase();
    let decoded = percent_decode(&lower);
    // The well-known paths only count on the path itself.
    ATTACKS.iter().find_map(|(kind, needles)| {
        needles
            .iter()
            .any(|n| lower.contains(n) || decoded.contains(n))
            .then_some(*kind)
    })
}

fn referer_host(referer: &str) -> Option<&str> {
    let rest = referer
        .strip_prefix("https://")
        .or_else(|| referer.strip_prefix("http://"))?;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
    let host = host.split(':').next()?;
    (!host.is_empty()).then_some(host)
}

fn url_path(url: &str) -> &str {
    url.split(['?', '#']).next().unwrap_or(url)
}

// ===== Report =====

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Bucket {
    Hour,
    Day,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Totals {
    pub requests: u64,
    /// Distinct client addresses.
    pub visitors: u64,
    pub bytes: u64,
    /// Requests that are crawlers or scripts.
    pub bots: u64,
    pub status_4xx: u64,
    pub status_5xx: u64,
    pub attacks: u64,
    /// Mean response time (ms) over requests that logged one.
    pub avg_ms: u32,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Point {
    /// Bucket start, Unix time.
    pub t: i64,
    pub requests: u64,
    pub visitors: u64,
    pub bytes: u64,
    pub errors: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Count {
    pub key: String,
    pub count: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IpRow {
    pub ip: String,
    pub count: u64,
    pub bytes: u64,
    pub errors: u64,
    pub attacks: u64,
    pub last: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub time: i64,
    pub ip: String,
    pub method: String,
    pub url: String,
    pub status: u16,
    pub ms: u32,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Report {
    pub from: i64,
    pub to: i64,
    pub bucket: String,
    pub totals: Totals,
    pub timeline: Vec<Point>,
    pub status: Vec<Count>,
    pub pages: Vec<Count>,
    pub ips: Vec<IpRow>,
    pub referers: Vec<Count>,
    pub browsers: Vec<Count>,
    pub systems: Vec<Count>,
    pub spiders: Vec<Count>,
    pub not_found: Vec<Count>,
    pub server_errors: Vec<Count>,
    pub slow: Vec<Request>,
    pub attack_kinds: Vec<Count>,
    pub attacks: Vec<Request>,
    /// Files read and lines scanned.
    pub files: usize,
    pub lines: u64,
    /// The scan stopped early (very large logs); totals cover what was read.
    pub partial: bool,
}

/// Distinct keys tracked per table, so a flood of random URLs cannot use up
/// memory; later new keys are not counted individually.
const MAX_KEYS: usize = 200_000;
const MAX_LINES: u64 = 30_000_000;
const MAX_SCAN: Duration = Duration::from_secs(90);

#[derive(Default)]
struct Table(HashMap<String, (u64, u64)>);

impl Table {
    fn add(&mut self, key: &str, bytes: u64) {
        if let Some(v) = self.0.get_mut(key) {
            v.0 += 1;
            v.1 += bytes;
        } else if self.0.len() < MAX_KEYS {
            self.0.insert(key.to_string(), (1, bytes));
        }
    }
    fn top(self, n: usize) -> Vec<Count> {
        let mut rows: Vec<Count> = self
            .0
            .into_iter()
            .map(|(key, (count, bytes))| Count { key, count, bytes })
            .collect();
        rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.key.cmp(&b.key)));
        rows.truncate(n);
        rows
    }
}

#[derive(Default)]
struct IpStats {
    count: u64,
    bytes: u64,
    errors: u64,
    attacks: u64,
    last: i64,
}

fn hash_ip(ip: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ip.hash(&mut h);
    h.finish()
}

/// Start of the local hour / day containing `t`.
fn bucket_start(t: i64, bucket: Bucket) -> i64 {
    let Some(local) = Local.timestamp_opt(t, 0).single() else {
        return t;
    };
    let start = match bucket {
        Bucket::Hour => local.date_naive().and_hms_opt(local.hour(), 0, 0),
        Bucket::Day => local.date_naive().and_hms_opt(0, 0, 0),
    };
    start
        .and_then(|naive| Local.from_local_datetime(&naive).earliest())
        .map(|dt| dt.timestamp())
        .unwrap_or(t)
}

/// Every bucket start in [from, to), so quiet hours show as zero.
fn bucket_starts(from: i64, to: i64, bucket: Bucket) -> Vec<i64> {
    let mut starts = Vec::new();
    let mut t = bucket_start(from, bucket);
    let step = match bucket {
        Bucket::Hour => 3600,
        Bucket::Day => 86_400,
    };
    while t < to && starts.len() < 2000 {
        starts.push(t);
        // Days are not always 24 h long (DST): find the next start from noon.
        let next = bucket_start(t + step + step / 2, bucket);
        t = if next > t { next } else { t + step };
    }
    starts
}

/// Files of a site that can hold requests in [from, to), oldest first.
fn files_for(dir: &Path, site_id: i64, from: i64) -> Vec<PathBuf> {
    let base = format!("site-{}.log", site_id);
    let mut files: Vec<PathBuf> = archives(dir, &base)
        .into_iter()
        // A file last written before `from` holds nothing newer.
        .filter(|p| mtime(p).map(unix).unwrap_or(i64::MAX) >= from)
        .collect();
    let current = current_log(dir, site_id);
    if current.exists() {
        files.push(current);
    }
    files
}

fn open_lines(path: &Path) -> std::io::Result<Box<dyn BufRead>> {
    let file = std::fs::File::open(path)?;
    let reader: Box<dyn Read> = if path.extension().and_then(|e| e.to_str()) == Some("gz") {
        Box::new(flate2::read::MultiGzDecoder::new(file))
    } else {
        Box::new(file)
    };
    Ok(Box::new(BufReader::with_capacity(256 * 1024, reader)))
}

pub fn analyze(dir: &Path, site_id: i64, from: i64, to: i64, own_hosts: &[String]) -> Report {
    let span = to - from;
    let bucket = if span <= 2 * 86_400 {
        Bucket::Hour
    } else {
        Bucket::Day
    };
    let starts = bucket_starts(from, to, bucket);
    let mut timeline: Vec<Point> = starts
        .iter()
        .map(|&t| Point {
            t,
            ..Default::default()
        })
        .collect();
    let mut timeline_visitors: Vec<HashSet<u64>> = vec![HashSet::new(); timeline.len()];

    let mut totals = Totals::default();
    let mut visitors: HashSet<u64> = HashSet::new();
    let mut ms_sum: u64 = 0;
    let mut ms_count: u64 = 0;
    let mut status = Table::default();
    let mut pages = Table::default();
    let mut referers = Table::default();
    let mut browsers = Table::default();
    let mut systems = Table::default();
    let mut spiders = Table::default();
    let mut not_found = Table::default();
    let mut server_errors = Table::default();
    let mut attack_kinds = Table::default();
    let mut ips: HashMap<String, IpStats> = HashMap::new();
    let mut slow: Vec<Request> = Vec::new();
    let mut attacks: Vec<Request> = Vec::new();

    let files = files_for(dir, site_id, from);
    let started = Instant::now();
    let mut lines = 0u64;
    let mut partial = false;
    let mut buf = String::new();
    'files: for path in &files {
        let Ok(mut reader) = open_lines(path) else {
            continue;
        };
        loop {
            buf.clear();
            match reader.read_line(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            lines += 1;
            if lines % 100_000 == 0 && (lines >= MAX_LINES || started.elapsed() > MAX_SCAN) {
                partial = true;
                break 'files;
            }
            let Some(e) = parse_line(buf.trim_end()) else {
                continue;
            };
            if e.time < from || e.time >= to {
                continue;
            }
            totals.requests += 1;
            totals.bytes += e.bytes;
            let ip_hash = hash_ip(e.ip);
            visitors.insert(ip_hash);
            if let Some(ms) = e.ms {
                ms_sum += ms as u64;
                ms_count += 1;
            }
            let error = e.status >= 400;
            if (400..500).contains(&e.status) {
                totals.status_4xx += 1;
            } else if e.status >= 500 {
                totals.status_5xx += 1;
            }
            // Timeline: buckets are sorted; find the last start <= time.
            let slot = starts.partition_point(|&s| s <= e.time);
            if slot > 0 {
                let p = &mut timeline[slot - 1];
                p.requests += 1;
                p.bytes += e.bytes;
                if error {
                    p.errors += 1;
                }
                timeline_visitors[slot - 1].insert(ip_hash);
            }
            status.add(&e.status.to_string(), e.bytes);
            let path = url_path(e.url);
            let agent = e.agent.to_ascii_lowercase();
            let crawler = spider(&agent);
            let bot = crawler.is_some() || is_bot(&agent);
            if bot {
                totals.bots += 1;
                spiders.add(crawler.unwrap_or("Other bots / scripts"), e.bytes);
            } else {
                browsers.add(browser(&agent), e.bytes);
                systems.add(system(&agent), e.bytes);
                if e.status < 400 {
                    pages.add(path, e.bytes);
                }
            }
            if let Some(host) = referer_host(e.referer) {
                if !own_hosts.iter().any(|h| h.eq_ignore_ascii_case(host)) {
                    referers.add(&host.to_ascii_lowercase(), 0);
                }
            }
            if e.status == 404 {
                not_found.add(path, e.bytes);
            } else if e.status >= 500 {
                server_errors.add(path, e.bytes);
            }
            let kind = attack_kind(e.url);
            if let Some(kind) = kind {
                totals.attacks += 1;
                attack_kinds.add(kind, 0);
                if attacks.len() >= 200 {
                    attacks.remove(0);
                }
                attacks.push(Request {
                    time: e.time,
                    ip: e.ip.to_string(),
                    method: e.method.to_string(),
                    url: e.url.chars().take(300).collect(),
                    status: e.status,
                    ms: e.ms.unwrap_or(0),
                    kind: kind.to_string(),
                });
            }
            if ips.len() < MAX_KEYS || ips.contains_key(e.ip) {
                let row = ips.entry(e.ip.to_string()).or_default();
                row.count += 1;
                row.bytes += e.bytes;
                row.last = row.last.max(e.time);
                if error {
                    row.errors += 1;
                }
                if kind.is_some() {
                    row.attacks += 1;
                }
            }
            // The 30 slowest requests (over 1 s).
            if let Some(ms) = e.ms.filter(|&ms| ms >= 1000) {
                if slow.len() < 30 || slow.last().is_some_and(|r| r.ms < ms) {
                    let row = Request {
                        time: e.time,
                        ip: e.ip.to_string(),
                        method: e.method.to_string(),
                        url: e.url.chars().take(300).collect(),
                        status: e.status,
                        ms,
                        kind: String::new(),
                    };
                    let at = slow.partition_point(|r| r.ms >= ms);
                    slow.insert(at, row);
                    slow.truncate(30);
                }
            }
        }
    }

    totals.visitors = visitors.len() as u64;
    totals.avg_ms = if ms_count > 0 {
        (ms_sum / ms_count) as u32
    } else {
        0
    };
    for (point, set) in timeline.iter_mut().zip(timeline_visitors) {
        point.visitors = set.len() as u64;
    }
    let mut ip_rows: Vec<IpRow> = ips
        .into_iter()
        .map(|(ip, s)| IpRow {
            ip,
            count: s.count,
            bytes: s.bytes,
            errors: s.errors,
            attacks: s.attacks,
            last: s.last,
        })
        .collect();
    ip_rows.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.ip.cmp(&b.ip)));
    ip_rows.truncate(50);
    let mut status = status.top(20);
    status.sort_by(|a, b| a.key.cmp(&b.key));
    attacks.reverse();

    Report {
        from,
        to,
        bucket: match bucket {
            Bucket::Hour => "hour".into(),
            Bucket::Day => "day".into(),
        },
        totals,
        timeline,
        status,
        pages: pages.top(50),
        ips: ip_rows,
        referers: referers.top(30),
        browsers: browsers.top(12),
        systems: systems.top(12),
        spiders: spiders.top(20),
        not_found: not_found.top(30),
        server_errors: server_errors.top(30),
        slow,
        attack_kinds: attack_kinds.top(10),
        attacks,
        files: files.len(),
        lines,
        partial,
    }
}

/// [from, to) for a named range in the server's time zone.
pub fn range(name: &str) -> Option<(i64, i64)> {
    let now = Local::now();
    let midnight = |date: NaiveDate| {
        date.and_hms_opt(0, 0, 0)
            .and_then(|n| Local.from_local_datetime(&n).earliest())
            .map(|d| d.timestamp())
    };
    let today = now.date_naive();
    let end = now.timestamp() + 1;
    match name {
        "today" => Some((midnight(today)?, end)),
        "yesterday" => Some((midnight(today.pred_opt()?)?, midnight(today)?)),
        "24h" => Some((end - 86_400, end)),
        "7d" => Some((midnight(today - chrono::Days::new(6))?, end)),
        "30d" => Some((midnight(today - chrono::Days::new(29))?, end)),
        "month" => Some((midnight(today.with_day(1)?)?, end)),
        _ => None,
    }
}

// ===== Tail =====

/// The newest lines of the current log (newest first), optionally only
/// those containing `filter` (case-insensitive). Reads at most the last 8 MB.
pub fn tail(path: &Path, limit: usize, filter: &str) -> Result<Vec<String>, String> {
    const WINDOW: u64 = 8 * 1024 * 1024;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.to_string()),
    };
    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
    let start = size.saturating_sub(WINDOW);
    file.seek(SeekFrom::Start(start))
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::with_capacity((size - start) as usize);
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0); // probably cut in the middle
    }
    let needle = filter.trim().to_lowercase();
    Ok(lines
        .into_iter()
        .rev()
        .filter(|l| needle.is_empty() || l.to_lowercase().contains(&needle))
        .take(limit)
        .map(String::from)
        .collect())
}

/// Empties a site's current log (OpenResty keeps its file handle).
pub fn clear(path: &Path) -> Result<(), String> {
    match std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
    {
        Ok(mut file) => file.flush().map_err(|e| e.to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.to_string()),
    }
}

/// Deletes a site's rotated logs.
pub fn remove_archives(dir: &Path, site_id: i64) -> usize {
    let base = format!("site-{}.log", site_id);
    archives(dir, &base)
        .into_iter()
        .filter(|p| std::fs::remove_file(p).is_ok())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = r#"203.0.113.9 - - [07/Nov/2026:23:04:35 +0800] "GET /blog/post?id=1 HTTP/1.1" 200 5120 "https://www.google.com/search?q=x" "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36" 0.123"#;

    #[test]
    fn lines_parse() {
        let e = parse_line(LINE).unwrap();
        assert_eq!(e.ip, "203.0.113.9");
        assert_eq!(e.method, "GET");
        assert_eq!(e.url, "/blog/post?id=1");
        assert_eq!(e.status, 200);
        assert_eq!(e.bytes, 5120);
        assert_eq!(e.referer, "https://www.google.com/search?q=x");
        assert_eq!(e.ms, Some(123));
        // 2026-11-07 15:04:35 UTC
        assert_eq!(e.time, 1_794_063_875);
        // Older lines without the response time, and a garbage request.
        let old = r#"::1 - - [01/Jan/2026:00:00:00 +0000] "\x16\x03\x01" 400 157 "-" "-""#;
        let e = parse_line(old).unwrap();
        assert_eq!(e.status, 400);
        assert_eq!(e.url, "");
        assert_eq!(e.ms, None);
        assert!(parse_line("garbage").is_none());
    }

    #[test]
    fn agents_and_attacks_are_classified() {
        let chrome = "mozilla/5.0 (windows nt 10.0) chrome/130.0 safari/537.36";
        assert_eq!(browser(chrome), "Chrome");
        assert_eq!(system(chrome), "Windows");
        assert_eq!(browser("mozilla/5.0 (iphone) micromessenger/8.0"), "WeChat");
        assert_eq!(
            spider("mozilla/5.0 (compatible; baiduspider/2.0)"),
            Some("Baiduspider")
        );
        assert!(is_bot("curl/8.5.0"));
        assert!(!is_bot(chrome));
        assert_eq!(
            attack_kind("/index.php?id=1%20UNION%20SELECT%201,2"),
            Some("sql")
        );
        assert_eq!(attack_kind("/../../etc/passwd"), Some("traversal"));
        assert_eq!(attack_kind("/.env"), Some("scan"));
        assert_eq!(attack_kind("/?q=%3Cscript%3Ealert(1)"), Some("xss"));
        assert_eq!(attack_kind("/blog/post?id=1"), None);
        assert_eq!(
            referer_host("https://user@www.Example.com:8443/a?b"),
            Some("www.Example.com")
        );
        assert_eq!(referer_host("-"), None);
        assert!(is_archive("site-3.log-20261107-000501"));
        assert!(is_archive("site-3.log-20261107-000501.gz"));
        assert!(is_archive("site-3.log.1"));
        assert!(is_archive("site-3.log.2.gz"));
        assert!(!is_archive("site-3.log"));
        assert!(!is_archive("site-3.log-20261107-000501.gz.tmp"));
        assert!(!is_archive("site-3.log.bak"));
    }

    #[test]
    fn rotation_compression_and_report() {
        let dir = std::env::temp_dir().join(format!("lp-acc-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let now = Local::now();
        let stamp = now.format("%d/%b/%Y:%H:%M:%S %z").to_string();
        let mut text = String::new();
        for i in 0..50 {
            text.push_str(&format!(
                "10.0.0.{} - - [{}] \"GET /p{} HTTP/1.1\" {} 100 \"-\" \"Mozilla/5.0 (X11; Linux x86_64) Firefox/131.0\" {}\n",
                i % 5,
                stamp,
                i % 3,
                if i % 10 == 0 { 404 } else { 200 },
                if i == 7 { "2.500" } else { "0.010" }
            ));
        }
        text.push_str(&format!(
            "198.51.100.7 - - [{}] \"GET /.env HTTP/1.1\" 404 0 \"-\" \"curl/8.5\" 0.001\n",
            stamp
        ));
        let log = dir.join("site-9.log");
        std::fs::write(&log, &text).unwrap();
        // Not a new day and under the limit: nothing to rotate.
        assert_eq!(rotate_files(&dir, false, 1 << 20), 0);
        assert_eq!(rotate_files(&dir, true, 1 << 20), 1);
        assert!(!log.exists());
        let rotated = archives(&dir, "site-9.log");
        assert_eq!(rotated.len(), 1);
        gzip_file(&rotated[0]).unwrap();
        let rotated = archives(&dir, "site-9.log");
        assert!(rotated[0].to_string_lossy().ends_with(".gz"));
        // New writes go to a fresh file; the report reads both.
        std::fs::write(
            &log,
            LINE.replace("07/Nov/2026:23:04:35 +0800", &stamp) + "\n",
        )
        .unwrap();

        let (from, to) = range("today").unwrap();
        let report = analyze(&dir, 9, from, to, &["www.google.com".to_string()]);
        assert_eq!(report.files, 2);
        assert_eq!(report.totals.requests, 52);
        assert_eq!(report.totals.visitors, 7);
        assert_eq!(report.totals.status_4xx, 6);
        assert_eq!(report.totals.bots, 1);
        assert_eq!(report.totals.attacks, 1);
        assert_eq!(report.attacks[0].kind, "scan");
        assert_eq!(report.slow.len(), 1);
        assert_eq!(report.slow[0].ms, 2500);
        assert_eq!(report.bucket, "hour");
        assert_eq!(report.timeline.iter().map(|p| p.requests).sum::<u64>(), 52);
        assert!(report.referers.is_empty(), "own host is not a referrer");
        assert_eq!(report.browsers[0].key, "Firefox");
        assert_eq!(report.not_found[0].key, "/p0");

        let lines = tail(&log, 10, "").unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(remove_archives(&dir, 9), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
