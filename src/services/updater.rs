//! Panel self-update: finds the latest release and upgrades by running the
//! release's own installer, which verifies the download, backs up the binary
//! and database, restarts the panel and rolls back if the new version fails
//! its health check.
//!
//! The installer stops this process, so it must not run inside it: under
//! systemd it gets its own transient unit (stopping the panel kills everything
//! in the panel's cgroup), elsewhere its own process group.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::shell;
use crate::db;

pub const SETTING_UPDATE: &str = "update";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_REPO: &str = "wdcodeku/LinxPanel-Rust";
const UNIT: &str = "linxpanel-update";
/// How often the release is looked up in the background.
const CHECK_EVERY_SECS: i64 = 6 * 3600;
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UpdateConfig {
    /// Install new releases unattended at `auto_time`.
    pub auto_update: bool,
    /// Local server time, "HH:MM".
    pub auto_time: String,
    /// "github" | "proxy" (a GitHub acceleration prefix) | "mirror".
    pub source: String,
    /// e.g. https://ghfast.top/
    pub proxy: String,
    /// A copy of the release files (latest.json, install.sh, packages).
    pub mirror: String,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            auto_update: false,
            auto_time: "04:00".into(),
            source: "github".into(),
            proxy: String::new(),
            mirror: String::new(),
        }
    }
}

fn valid_base_url(value: &str) -> bool {
    (value.starts_with("https://") || value.starts_with("http://"))
        && value.len() < 300
        && !value
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '\'')
}

impl UpdateConfig {
    pub fn validate(&self) -> Result<(), String> {
        if parse_hhmm(&self.auto_time).is_none() {
            return Err("Update time must be HH:MM".into());
        }
        match self.source.as_str() {
            "github" => Ok(()),
            "proxy" if valid_base_url(&self.proxy) => Ok(()),
            "mirror" if valid_base_url(&self.mirror) => Ok(()),
            "proxy" | "mirror" => Err("Enter an http(s):// address".into()),
            _ => Err("Unknown update source".into()),
        }
    }

    fn proxy_prefix(&self) -> &str {
        if self.source == "proxy" {
            self.proxy.trim()
        } else {
            ""
        }
    }

    fn mirror_base(&self) -> Option<&str> {
        (self.source == "mirror").then(|| self.mirror.trim().trim_end_matches('/'))
    }
}

fn parse_hhmm(value: &str) -> Option<(u32, u32)> {
    let (h, m) = value.trim().split_once(':')?;
    let (h, m) = (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

pub async fn load_config(pool: &SqlitePool) -> UpdateConfig {
    db::get_setting(pool, SETTING_UPDATE)
        .await
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub async fn save_config(pool: &SqlitePool, config: &UpdateConfig) -> anyhow::Result<()> {
    db::set_setting(pool, SETTING_UPDATE, &serde_json::to_string(config)?).await
}

fn repo() -> String {
    std::env::var("LINXPANEL_GITHUB_REPO")
        .ok()
        .filter(|r| {
            r.split('/').count() == 2
                && r.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c))
        })
        .unwrap_or_else(|| DEFAULT_REPO.into())
}

// ===== Versions =====

/// "v1.2.3" / "1.2.3" -> (1, 2, 3). Anything else (pre-releases included) is
/// rejected, which also keeps tags safe to put in URLs.
pub fn parse_version(tag: &str) -> Option<(u64, u64, u64)> {
    let mut parts = tag
        .trim()
        .strip_prefix('v')
        .unwrap_or(tag.trim())
        .split('.');
    let mut next = || -> Option<u64> {
        let part = parts.next()?;
        (!part.is_empty() && part.len() <= 9 && part.bytes().all(|b| b.is_ascii_digit()))
            .then(|| part.parse().ok())
            .flatten()
    };
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

pub fn is_newer(tag: &str, current: &str) -> bool {
    match (parse_version(tag), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

// ===== Release lookup =====

#[derive(Debug, Clone, Serialize, Default)]
pub struct Release {
    pub tag: String,
    pub notes: String,
    pub url: String,
    pub published_at: String,
}

fn fetch(url: &str) -> Result<String, String> {
    let output = shell::run_with_timeout(
        "curl",
        &[
            "-fsSL",
            "--retry",
            "2",
            "--connect-timeout",
            "10",
            "--max-time",
            "25",
            url,
        ],
        FETCH_TIMEOUT,
    )?;
    if output.ok() {
        Ok(output.stdout)
    } else {
        Err(output.error_text())
    }
}

fn release_from_json(text: &str) -> Result<Release, String> {
    let json: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let field = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| json[*name].as_str())
            .unwrap_or_default()
            .to_string()
    };
    let release = Release {
        tag: field(&["tag_name", "tag"]),
        // Release notes are shown as plain text; keep them bounded.
        notes: field(&["body", "notes"]).chars().take(20_000).collect(),
        url: field(&["html_url", "url"]),
        published_at: field(&["published_at"]),
    };
    if parse_version(&release.tag).is_none() {
        return Err(format!("Unexpected release tag: {:?}", release.tag));
    }
    Ok(release)
}

/// The tag GitHub redirects /releases/latest to (works through mirrors of
/// github.com that do not proxy the API).
fn latest_tag_from_redirect(prefix: &str, repo: &str) -> Result<Release, String> {
    let url = format!("{}https://github.com/{}/releases/latest", prefix, repo);
    let output = shell::run_with_timeout(
        "curl",
        &[
            "-sS",
            "-o",
            "/dev/null",
            "-w",
            "%{redirect_url}",
            "--max-time",
            "25",
            &url,
        ],
        FETCH_TIMEOUT,
    )?;
    let location = output.stdout.trim();
    let tag = location.rsplit('/').next().unwrap_or_default();
    if parse_version(tag).is_none() {
        return Err(format!("Could not read the latest release from {}", url));
    }
    Ok(Release {
        tag: tag.to_string(),
        url: format!("https://github.com/{}/releases/tag/{}", repo, tag),
        ..Release::default()
    })
}

/// Looks up the latest release. Blocking (runs curl).
pub fn latest_release(config: &UpdateConfig) -> Result<Release, String> {
    if !shell::has_binary("curl") {
        return Err("curl is not installed".into());
    }
    if let Some(mirror) = config.mirror_base() {
        return release_from_json(&fetch(&format!("{}/latest.json", mirror))?);
    }
    let repo = repo();
    let prefix = config.proxy_prefix();
    let api = format!("https://api.github.com/repos/{}/releases/latest", repo);
    // The API carries the release notes; acceleration proxies usually only
    // cover github.com, so fall back to the redirect there.
    let direct = if prefix.is_empty() {
        fetch(&api).and_then(|text| release_from_json(&text))
    } else {
        Err(String::new())
    };
    direct.or_else(|api_error| {
        latest_tag_from_redirect(prefix, &repo).map_err(|err| {
            if api_error.is_empty() {
                err
            } else {
                format!("{}; {}", api_error, err)
            }
        })
    })
}

// ===== Running an update =====

/// Where update state lives: <data>/update/{state.json, update.log, exit}.
fn work_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("update")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunState {
    pub from: String,
    pub target: String,
    pub started_at: i64,
    pub auto: bool,
    /// Process group leader when not under systemd.
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct LastResult {
    pub target: String,
    pub success: bool,
    pub finished_at: i64,
    pub message: String,
}

/// The panel's install root (the installer's LINXPANEL_INSTALL_ROOT), when
/// this process is the installed binary. Development runs cannot self-update.
pub fn install_root(data_dir: &Path) -> Option<PathBuf> {
    let root = data_dir.parent()?.to_path_buf();
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let installed = root.join("bin").join("linxpanel").canonicalize().ok()?;
    (exe == installed).then_some(root)
}

fn read_state(dir: &Path) -> Option<RunState> {
    serde_json::from_str(&std::fs::read_to_string(dir.join("state.json")).ok()?).ok()
}

fn still_running(state: &RunState) -> bool {
    match state.pid {
        Some(pid) => Path::new(&format!("/proc/{}", pid)).exists(),
        None => shell::run("systemctl", &["is-active", "--quiet", UNIT])
            .map(|o| o.ok())
            .unwrap_or(false),
    }
}

/// Runs the upgrade wrapper; its values arrive as environment variables, never
/// spliced into the script text.
const WRAPPER: &str = r#"set -u
exec >>"$LP_LOG" 2>&1
echo "[LinxPanel] $(date '+%F %T') updating ${LP_FROM} -> ${LP_TARGET}"
if ! curl -fsSL --retry 3 --connect-timeout 15 --max-time 180 -o "$LP_DIR/install.sh" "$LP_INSTALLER_URL"; then
  echo "[LinxPanel] Could not download the installer: $LP_INSTALLER_URL"
  echo 1 > "$LP_DIR/exit"
  exit 1
fi
bash "$LP_DIR/install.sh" </dev/null
code=$?
echo "[LinxPanel] installer exited with status $code"
echo "$code" > "$LP_DIR/exit"
exit "$code"
"#;

/// Starts the upgrade to `release` in the background and returns at once.
pub fn start(
    data_dir: &Path,
    config: &UpdateConfig,
    release: &Release,
    auto: bool,
) -> Result<(), String> {
    if parse_version(&release.tag).is_none() {
        return Err("Invalid release tag".into());
    }
    let root = install_root(data_dir).ok_or(
        "This panel was not installed by the installer; update it with `Lp update` or by reinstalling",
    )?;
    let dir = work_dir(data_dir);
    if let Some(state) = read_state(&dir) {
        if still_running(&state) {
            return Err("An update is already running".into());
        }
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(dir.join("exit"));
    let log = dir.join("update.log");
    // Keep the previous run for reference, not forever.
    let _ = std::fs::rename(&log, dir.join("update.prev.log"));
    let wrapper = dir.join("run.sh");
    std::fs::write(&wrapper, WRAPPER).map_err(|e| e.to_string())?;

    let repo = repo();
    let prefix = config.proxy_prefix();
    let installer_url = match config.mirror_base() {
        Some(mirror) => format!("{}/install.sh", mirror),
        // Pinned to the release's own installer.
        None => format!(
            "{}https://raw.githubusercontent.com/{}/{}/scripts/install.sh",
            prefix, repo, release.tag
        ),
    };
    let mut env: Vec<(&str, String)> = vec![
        ("LP_LOG", log.to_string_lossy().into_owned()),
        ("LP_DIR", dir.to_string_lossy().into_owned()),
        ("LP_FROM", CURRENT_VERSION.into()),
        ("LP_TARGET", release.tag.clone()),
        ("LP_INSTALLER_URL", installer_url),
        (
            "LINXPANEL_INSTALL_ROOT",
            root.to_string_lossy().into_owned(),
        ),
        ("LINXPANEL_INSTALL_MODE", "binary".into()),
        ("LINXPANEL_VERSION", release.tag.clone()),
        ("LINXPANEL_GITHUB_REPO", repo.clone()),
        ("LINXPANEL_GITHUB_PROXY", prefix.to_string()),
        ("LINXPANEL_LANG", "en".into()),
        (
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
        ),
        (
            "HOME",
            std::env::var("HOME").unwrap_or_else(|_| "/root".into()),
        ),
    ];
    if let Some(mirror) = config.mirror_base() {
        env.push(("LINXPANEL_RELEASE_BASE_URL", mirror.to_string()));
    }

    let mut state = RunState {
        from: CURRENT_VERSION.into(),
        target: release.tag.clone(),
        started_at: chrono::Utc::now().timestamp(),
        auto,
        pid: None,
    };

    if super::sysservice::systemd_running() {
        // A leftover failed unit of the same name would block the new one.
        let _ = shell::run_privileged("systemctl", &["reset-failed", UNIT]);
        let mut args: Vec<String> = vec![
            format!("--unit={}", UNIT),
            "--collect".into(),
            "--quiet".into(),
            "--description=LinxPanel update".into(),
        ];
        args.extend(
            env.iter()
                .map(|(key, value)| format!("--setenv={}={}", key, value)),
        );
        args.push("/bin/bash".into());
        args.push(wrapper.to_string_lossy().into_owned());
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let output = shell::run_privileged("systemd-run", &args)?;
        if !output.ok() {
            return Err(format!(
                "Could not start the update: {}",
                output.error_text()
            ));
        }
    } else {
        use std::os::unix::process::CommandExt;
        let mut command = std::process::Command::new("/bin/bash");
        command
            .arg(&wrapper)
            .env_clear()
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            // Its own group: signals aimed at the panel do not reach it.
            .process_group(0);
        let child = command
            .spawn()
            .map_err(|e| format!("Could not start the update: {}", e))?;
        state.pid = Some(child.id());
        // Reap it whenever it ends (if this process is still around).
        std::thread::spawn(move || {
            let mut child = child;
            let _ = child.wait();
        });
    }
    std::fs::write(
        dir.join("state.json"),
        serde_json::to_string(&state).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    tracing::warn!(
        "Started panel update {} -> {}",
        CURRENT_VERSION,
        release.tag
    );
    Ok(())
}

fn log_tail(dir: &Path, lines: usize) -> String {
    let text = std::fs::read_to_string(dir.join("update.log"))
        .or_else(|_| std::fs::read_to_string(dir.join("update.prev.log")))
        .unwrap_or_default();
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// Running state, or the outcome of a finished run (which is then recorded
/// and the state cleared).
pub async fn progress(pool: &SqlitePool, data_dir: &Path) -> (Option<RunState>, String) {
    let dir = work_dir(data_dir);
    let log = log_tail(&dir, 60);
    let Some(state) = read_state(&dir) else {
        return (None, log);
    };
    if still_running(&state) {
        return (Some(state), log);
    }
    // Finished (or died with the old process). The installer rolls back on a
    // failed health check, so the running version tells the truth.
    let exit = std::fs::read_to_string(dir.join("exit"))
        .ok()
        .and_then(|t| t.trim().parse::<i32>().ok());
    let upgraded = parse_version(&state.target) == parse_version(CURRENT_VERSION);
    let success = upgraded && exit.unwrap_or(0) == 0;
    let message = if success {
        format!("Updated from {} to {}", state.from, state.target)
    } else if upgraded {
        format!(
            "{} is running, but the installer reported an error",
            state.target
        )
    } else {
        let last = log
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("no log")
            .to_string();
        format!(
            "Update to {} failed; still on {}: {}",
            state.target, CURRENT_VERSION, last
        )
    };
    let result = LastResult {
        target: state.target.clone(),
        success,
        finished_at: chrono::Utc::now().timestamp(),
        message,
    };
    let _ = db::set_setting(
        pool,
        "update_last_result",
        &serde_json::to_string(&serde_json::json!({
            "target": result.target, "success": result.success,
            "finished_at": result.finished_at, "message": result.message,
        }))
        .unwrap_or_default(),
    )
    .await;
    if !result.success {
        // Auto-update will not retry a release that failed until a newer one appears.
        let _ = db::set_setting(pool, "update_failed_tag", &state.target).await;
    }
    let _ = std::fs::remove_file(dir.join("state.json"));
    if result.success {
        tracing::info!("{}", result.message);
    } else {
        tracing::warn!("{}", result.message);
    }
    (None, log)
}

pub async fn last_result(pool: &SqlitePool) -> Option<serde_json::Value> {
    db::get_setting(pool, "update_last_result")
        .await
        .ok()
        .flatten()
        .and_then(|t| serde_json::from_str(&t).ok())
}

// ===== Cached check + background schedule =====

#[derive(Debug, Clone, Serialize, Default)]
pub struct CheckResult {
    pub current: String,
    pub latest: Option<Release>,
    pub has_update: bool,
    pub checked_at: i64,
    pub error: String,
}

/// The last check, shared by the API and the scheduler.
pub struct Checker {
    last: Mutex<Option<CheckResult>>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            last: Mutex::new(None),
        }
    }

    pub fn cached(&self) -> Option<CheckResult> {
        self.last.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub async fn check(&self, config: &UpdateConfig) -> CheckResult {
        let config = config.clone();
        let found = tokio::task::spawn_blocking(move || latest_release(&config))
            .await
            .unwrap_or_else(|e| Err(e.to_string()));
        let previous = self.cached();
        let result = match found {
            Ok(release) => CheckResult {
                current: CURRENT_VERSION.into(),
                has_update: is_newer(&release.tag, CURRENT_VERSION),
                latest: Some(release),
                checked_at: chrono::Utc::now().timestamp(),
                error: String::new(),
            },
            // Keep showing the last known release when a check fails.
            Err(error) => CheckResult {
                current: CURRENT_VERSION.into(),
                latest: previous.as_ref().and_then(|p| p.latest.clone()),
                has_update: previous.as_ref().is_some_and(|p| p.has_update),
                checked_at: chrono::Utc::now().timestamp(),
                error,
            },
        };
        *self.last.lock().unwrap_or_else(|e| e.into_inner()) = Some(result.clone());
        result
    }
}

impl Default for Checker {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether `now` (local "HH:MM") falls in the 10 minutes after `at`.
fn in_window(at: (u32, u32), now: (u32, u32)) -> bool {
    let start = at.0 * 60 + at.1;
    let now = now.0 * 60 + now.1;
    (now + 1440 - start) % 1440 < 10
}

/// Checks for releases every few hours and, when auto-update is on, installs
/// a new one in the configured window.
pub async fn run(
    state: std::sync::Arc<crate::AppState>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    // Let startup settle (and a just-finished update record its result).
    let mut next_check = chrono::Utc::now().timestamp() + 90;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(60)) => {}
            _ = shutdown.changed() => return,
        }
        let (running, _) = progress(&state.db, &state.data_dir).await;
        if running.is_some() {
            continue;
        }
        let config = load_config(&state.db).await;
        let now = chrono::Utc::now().timestamp();
        if now >= next_check {
            let result = state.updates.check(&config).await;
            next_check = now + CHECK_EVERY_SECS;
            if !result.error.is_empty() {
                tracing::debug!("Update check failed: {}", result.error);
            }
        }
        if !config.auto_update {
            continue;
        }
        let Some(at) = parse_hhmm(&config.auto_time) else {
            continue;
        };
        let local = chrono::Local::now();
        use chrono::Timelike;
        if !in_window(at, (local.hour(), local.minute())) {
            continue;
        }
        // Fresh information right before acting.
        let result = state.updates.check(&config).await;
        let Some(release) = result.latest.filter(|_| result.has_update) else {
            continue;
        };
        let failed = db::get_setting(&state.db, "update_failed_tag")
            .await
            .ok()
            .flatten();
        if failed.as_deref() == Some(release.tag.as_str()) {
            continue;
        }
        let data_dir = state.data_dir.clone();
        let release_clone = release.clone();
        let started =
            tokio::task::spawn_blocking(move || start(&data_dir, &config, &release_clone, true))
                .await
                .unwrap_or_else(|e| Err(e.to_string()));
        if let Err(err) = started {
            tracing::warn!("Automatic update to {} not started: {}", release.tag, err);
            let _ = db::set_setting(&state.db, "update_failed_tag", &release.tag).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_and_compare() {
        assert_eq!(parse_version("v0.1.10"), Some((0, 1, 10)));
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        for bad in [
            "v1.2",
            "1.2.3.4",
            "v1.2.3-rc1",
            "v1.x.3",
            "",
            "v1.2.3/../x",
            "v+1.2.3",
        ] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
        assert!(is_newer("v0.1.10", "0.1.9"), "numeric, not lexical");
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"), "never downgrade");
        assert!(!is_newer("garbage", "0.1.0"));
    }

    #[test]
    fn release_json_from_github_and_mirror() {
        let github = r#"{"tag_name":"v0.2.0","body":"notes","html_url":"https://x/r","published_at":"2026-10-27T00:00:00Z"}"#;
        let release = release_from_json(github).unwrap();
        assert_eq!(
            (release.tag.as_str(), release.notes.as_str()),
            ("v0.2.0", "notes")
        );
        let mirror = r#"{"tag":"v0.2.0","notes":"n"}"#;
        assert_eq!(release_from_json(mirror).unwrap().tag, "v0.2.0");
        assert!(release_from_json(r#"{"tag_name":"v0.2.0;rm -rf /"}"#).is_err());
        assert!(release_from_json("not json").is_err());
    }

    #[test]
    fn config_validation_and_window() {
        assert!(UpdateConfig::default().validate().is_ok());
        let bad_time = UpdateConfig {
            auto_time: "25:00".into(),
            ..Default::default()
        };
        assert!(bad_time.validate().is_err());
        let proxy = UpdateConfig {
            source: "proxy".into(),
            proxy: "https://ghfast.top/".into(),
            ..Default::default()
        };
        assert!(proxy.validate().is_ok());
        assert_eq!(proxy.proxy_prefix(), "https://ghfast.top/");
        let bad_proxy = UpdateConfig {
            source: "proxy".into(),
            proxy: "https://x/ ; rm".into(),
            ..Default::default()
        };
        assert!(bad_proxy.validate().is_err());
        let mirror = UpdateConfig {
            source: "mirror".into(),
            mirror: "https://dl.example.com/lp/".into(),
            ..Default::default()
        };
        assert_eq!(mirror.mirror_base(), Some("https://dl.example.com/lp"));
        assert_eq!(mirror.proxy_prefix(), "", "a mirror ignores the proxy");

        assert!(in_window((4, 0), (4, 0)));
        assert!(in_window((4, 0), (4, 9)));
        assert!(!in_window((4, 0), (4, 10)));
        assert!(!in_window((4, 0), (3, 59)));
        assert!(in_window((23, 55), (0, 2)), "wraps past midnight");
    }
}
