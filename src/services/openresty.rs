use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use sqlx::SqlitePool;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::models::{Domain, Site};
use crate::AppState;

const SETTING_OPENRESTY_ENABLED: &str = "openresty_enabled";
const SETTING_OPENRESTY_BIN: &str = "openresty_bin";
const SETTING_OPENRESTY_SITES_DIR: &str = "openresty_sites_dir";
const SETTING_OPENRESTY_CONF_PATH: &str = "openresty_conf_path";
const SETTING_PHP_FPM_ADDR: &str = "php_fpm_addr";
const SETTING_OPENRESTY_LISTEN_IPV6: &str = "openresty_listen_ipv6";

#[cfg(target_os = "windows")]
const DEFAULT_OPENRESTY_BIN: &str = "C:\\openresty\\nginx\\nginx.exe";
#[cfg(not(target_os = "windows"))]
const DEFAULT_OPENRESTY_BIN: &str = "/usr/local/openresty/nginx/sbin/nginx";

#[cfg(target_os = "windows")]
const DEFAULT_OPENRESTY_SITES_DIR: &str = "C:\\openresty\\nginx\\conf\\sites-enabled";
#[cfg(not(target_os = "windows"))]
const DEFAULT_OPENRESTY_SITES_DIR: &str = "/usr/local/openresty/nginx/conf/sites-enabled";

#[cfg(target_os = "linux")]
/// Repository install first, source build as fallback; see the script.
const QUICK_INSTALL_SCRIPT: &str = include_str!("../../scripts/openresty-install.sh");

#[derive(Debug, Clone, Serialize)]
pub struct OpenRestyStatus {
    pub supported: bool,
    pub installed: bool,
    pub enabled: bool,
    pub nginx_bin: String,
    pub nginx_conf: String,
    pub sites_dir: String,
    pub version: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenRestyInstallResult {
    pub changed: bool,
    pub status: OpenRestyStatus,
    pub log: String,
}

#[derive(Debug, Clone)]
struct OpenRestyRuntime {
    enabled: bool,
    nginx_bin: String,
    sites_dir: PathBuf,
    nginx_conf: Option<PathBuf>,
    php_fpm_addr: String,
    listen_ipv6: bool,
}

impl OpenRestyRuntime {
    fn site_conf_path(&self, site_id: i64) -> PathBuf {
        self.sites_dir
            .join(format!("linxpanel-site-{}.conf", site_id))
    }
}

#[derive(Debug, Clone)]
struct RuntimeInspection {
    runtime: OpenRestyRuntime,
    resolved_nginx_bin: Option<PathBuf>,
    detected_conf_path: Option<PathBuf>,
    detected_version: Option<String>,
    is_openresty: bool,
}

pub async fn get_status(state: &AppState) -> Result<OpenRestyStatus> {
    let inspection = inspect_runtime(&state.db).await?;
    Ok(to_public_status(inspection))
}

pub async fn ensure_management_available(state: &AppState) -> Result<()> {
    let status = get_status(state).await?;
    if status.installed {
        return Ok(());
    }
    Err(anyhow!(
        "OpenResty is required. Please install OpenResty first."
    ))
}

pub async fn quick_install(state: &AppState) -> Result<OpenRestyInstallResult> {
    let _guard = state.openresty_lock.lock().await;
    if !cfg!(any(target_os = "linux", target_os = "windows")) {
        return Err(anyhow!(
            "OpenResty quick install is supported on Windows and Linux only"
        ));
    }

    let before = inspect_runtime(&state.db).await?;
    let was_installed = before.is_openresty;
    let mut log = String::new();

    if was_installed {
        log.push_str("[LinxPanel] OpenResty already installed, skip package install.\n");
    } else {
        let install_log = install_openresty_package()?;
        log.push_str(&install_log);
        if !log.ends_with('\n') {
            log.push('\n');
        }
    }

    let after_install = inspect_runtime(&state.db).await?;
    if !after_install.is_openresty {
        return Err(anyhow!("OpenResty binary not found after installation"));
    }
    let resolved_bin = after_install
        .resolved_nginx_bin
        .clone()
        .ok_or_else(|| anyhow!("OpenResty binary not found after installation"))?;

    let conf_path = after_install
        .detected_conf_path
        .clone()
        .or_else(|| guess_conf_path_from_bin(&resolved_bin))
        .ok_or_else(|| anyhow!("OpenResty main config was not detected"))?;

    let sites_dir = prepare_sites_dir(&conf_path, &after_install.runtime.sites_dir, &mut log)?;
    persist_install_settings(
        &state.db,
        &resolved_bin,
        Some(conf_path.as_path()),
        &sites_dir,
    )
    .await?;

    let runtime = load_runtime(&state.db).await?;
    test_openresty(&runtime).context("OpenResty config test failed after install")?;

    let status = to_public_status(inspect_runtime(&state.db).await?);
    Ok(OpenRestyInstallResult {
        changed: !was_installed,
        status,
        log,
    })
}

pub async fn sync_site(state: &AppState, site_id: i64) -> Result<()> {
    let _guard = state.openresty_lock.lock().await;
    let runtime = load_runtime_for_sync(&state.db).await?;
    if !runtime.enabled {
        return Ok(());
    }
    if !cfg!(any(target_os = "linux", target_os = "windows")) {
        return Err(anyhow!(
            "OpenResty sync is supported on Windows and Linux only"
        ));
    }

    let site = sqlx::query_as::<_, Site>("SELECT * FROM sites WHERE id = ?")
        .bind(site_id)
        .fetch_optional(&state.db)
        .await
        .context("Failed to load site data")?;

    let defense = SiteDefense::load(state, &runtime).await;
    let desired_config = match site {
        Some(site) => Some(render_site_config(&state.db, &site, &runtime, &defense).await?),
        None => None,
    };

    apply_site_config(&runtime, site_id, desired_config)?;
    Ok(())
}

pub async fn remove_site(state: &AppState, site_id: i64) -> Result<()> {
    let _guard = state.openresty_lock.lock().await;
    let runtime = load_runtime_for_sync(&state.db).await?;
    if !runtime.enabled {
        return Ok(());
    }
    if !cfg!(any(target_os = "linux", target_os = "windows")) {
        return Err(anyhow!(
            "OpenResty sync is supported on Windows and Linux only"
        ));
    }

    apply_site_config(&runtime, site_id, None)?;
    Ok(())
}

fn apply_site_config(
    runtime: &OpenRestyRuntime,
    site_id: i64,
    desired_config: Option<String>,
) -> Result<()> {
    fs::create_dir_all(&runtime.sites_dir).with_context(|| {
        format!(
            "Failed to create OpenResty sites dir: {}",
            runtime.sites_dir.display()
        )
    })?;

    let conf_path = runtime.site_conf_path(site_id);
    let previous_content = fs::read_to_string(&conf_path).ok();
    if previous_content.as_deref() == desired_config.as_deref() {
        return Ok(());
    }

    if let Some(content) = desired_config.as_deref() {
        write_atomic(&conf_path, content)
            .with_context(|| format!("Failed to write site config: {}", conf_path.display()))?;
    } else if conf_path.exists() {
        fs::remove_file(&conf_path)
            .with_context(|| format!("Failed to remove site config: {}", conf_path.display()))?;
    }

    if let Err(err) = test_openresty(runtime) {
        restore_file(&conf_path, previous_content.as_deref())?;
        return Err(anyhow!("OpenResty config test failed: {}", err));
    }

    if let Err(err) = reload_openresty(runtime) {
        restore_file(&conf_path, previous_content.as_deref())?;
        let _ = test_openresty(runtime);
        let _ = reload_openresty(runtime);
        return Err(anyhow!("OpenResty reload failed: {}", err));
    }

    Ok(())
}

// ===== Defense: shared http-level settings, per-site logs and rate limits =====

/// Shared http-level settings for panel-managed sites. Included with the site
/// files and named to sort first, so the log format and zones exist before any
/// server block uses them.
const GLOBAL_CONF: &str = "00-linxpanel-global.conf";

/// What the global file currently provides; site files only use what exists.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct GlobalFeatures {
    log_format: bool,
    limit_zones: bool,
}

impl GlobalFeatures {
    fn read(runtime: &OpenRestyRuntime) -> Self {
        let text = fs::read_to_string(runtime.sites_dir.join(GLOBAL_CONF)).unwrap_or_default();
        Self {
            log_format: text.contains("log_format linxpanel "),
            limit_zones: text.contains("zone=lp_req:") && text.contains("zone=lp_conn:"),
        }
    }
}

/// Per-site defense directives, derived from the defense settings.
pub(crate) struct SiteDefense {
    log_dir: Option<PathBuf>,
    /// (burst, connections) when CC protection is on.
    cc: Option<(u64, u64)>,
}

impl SiteDefense {
    async fn load(state: &AppState, runtime: &OpenRestyRuntime) -> Self {
        let config = super::defense::load_config(&state.db).await;
        Self::new(
            GlobalFeatures::read(runtime),
            &config,
            &super::defense::web_log_dir(&state.data_dir),
        )
    }

    fn new(
        features: GlobalFeatures,
        config: &super::defense::DefenseConfig,
        log_dir: &Path,
    ) -> Self {
        Self {
            log_dir: (features.log_format && fs::create_dir_all(log_dir).is_ok())
                .then(|| log_dir.to_path_buf()),
            cc: (features.limit_zones && config.cc_enabled)
                .then_some((config.cc_burst, config.cc_conn)),
        }
    }

    fn server_directives(&self, site_id: i64) -> String {
        let mut out = String::new();
        if let Some(dir) = &self.log_dir {
            // One log per site: the auto-ban engine reads these, and they are
            // what an operator wants when a single site is under attack.
            let path = dir.join(format!("site-{}.log", site_id));
            out.push_str(&format!(
                "    access_log {} linxpanel;\n",
                quote_nginx_value(&path.to_string_lossy())
            ));
        }
        if let Some((burst, conn)) = self.cc {
            out.push_str(&format!(
                "    limit_req zone=lp_req burst={} nodelay;\n",
                burst
            ));
            out.push_str(&format!("    limit_conn lp_conn {};\n", conn));
        }
        out
    }
}

/// Directives an operator commonly sets in nginx.conf already; setting them
/// twice at http level fails `nginx -t`, so they are only added when absent.
const HARDENING_DIRECTIVES: &[(&str, &str)] = &[
    // Slowloris: a client gets 10s to send its headers, 20s between body reads.
    ("client_header_timeout", "10s"),
    ("client_body_timeout", "20s"),
    ("send_timeout", "30s"),
    ("keepalive_timeout", "30s"),
    // Free the memory of timed-out connections at once (RST instead of FIN wait).
    ("reset_timedout_connection", "on"),
    ("server_tokens", "off"),
];

fn directive_present(main_conf: &str, name: &str) -> bool {
    main_conf.lines().any(|line| {
        strip_comment(line)
            .split_whitespace()
            .next()
            .is_some_and(|word| word == name)
    })
}

/// level 2: everything; 1: log format and zones only; 0: no file.
fn render_global_conf(
    level: u8,
    config: &super::defense::DefenseConfig,
    main_conf: &str,
) -> Option<String> {
    if level == 0 {
        return None;
    }
    let mut out =
        String::from("# Generated by LinxPanel (Security > Defense). Edits are overwritten.\n");
    out.push_str(
        "log_format linxpanel '$remote_addr - $remote_user [$time_local] \"$request\" $status $body_bytes_sent \"$http_referer\" \"$http_user_agent\" $request_time';\n",
    );
    out.push_str(&format!(
        "limit_req_zone $binary_remote_addr zone=lp_req:20m rate={}r/s;\n",
        config.cc_rate
    ));
    out.push_str("limit_conn_zone $binary_remote_addr zone=lp_conn:10m;\n");
    if level >= 2 {
        for name in ["limit_req_status", "limit_conn_status"] {
            if !directive_present(main_conf, name) {
                out.push_str(&format!("{} 429;\n", name));
            }
        }
        for (name, value) in HARDENING_DIRECTIVES {
            if !directive_present(main_conf, name) {
                out.push_str(&format!("{} {};\n", name, value));
            }
        }
    }
    Some(out)
}

/// Writes the global defense settings and re-renders every site with them,
/// tested as a whole: when the full set does not pass `nginx -t` (a directive
/// set elsewhere already), it falls back to less, and finally back to the
/// previous files. Returns what was applied.
pub async fn apply_defense(state: &AppState) -> Result<String> {
    let _guard = state.openresty_lock.lock().await;
    // Without OpenResty there is nothing to configure yet; installing it
    // applies these settings.
    let Ok(runtime) = load_runtime_for_sync(&state.db).await else {
        return Ok("OpenResty is not installed".into());
    };
    if !runtime.enabled {
        return Ok("OpenResty is not managed by the panel".into());
    }
    fs::create_dir_all(&runtime.sites_dir)
        .with_context(|| format!("Failed to create {}", runtime.sites_dir.display()))?;
    let config = super::defense::load_config(&state.db).await;
    let log_dir = super::defense::web_log_dir(&state.data_dir);
    let main_conf = runtime
        .nginx_conf
        .as_ref()
        .and_then(|path| fs::read_to_string(path).ok())
        .unwrap_or_default();
    let sites = sqlx::query_as::<_, Site>("SELECT * FROM sites")
        .fetch_all(&state.db)
        .await
        .context("Failed to load sites")?;

    let global_path = runtime.sites_dir.join(GLOBAL_CONF);
    let mut paths = vec![global_path.clone()];
    paths.extend(sites.iter().map(|site| runtime.site_conf_path(site.id)));
    let previous: Vec<Option<String>> = paths.iter().map(|p| fs::read_to_string(p).ok()).collect();
    let restore_all = || {
        for (path, content) in paths.iter().zip(&previous) {
            let _ = restore_file(path, content.as_deref());
        }
    };

    let mut last_error = String::new();
    for level in [2u8, 1, 0] {
        let global = render_global_conf(level, &config, &main_conf);
        let features = GlobalFeatures {
            log_format: global.is_some(),
            limit_zones: global.is_some(),
        };
        let defense = SiteDefense::new(features, &config, &log_dir);
        let mut desired = vec![global];
        for site in &sites {
            // A site that cannot render keeps its current file.
            let path = runtime.site_conf_path(site.id);
            desired.push(
                render_site_config(&state.db, site, &runtime, &defense)
                    .await
                    .ok()
                    .or_else(|| fs::read_to_string(&path).ok()),
            );
        }
        let unchanged = paths
            .iter()
            .zip(&desired)
            .all(|(path, want)| fs::read_to_string(path).ok() == *want);
        if unchanged {
            return Ok(describe_level(level));
        }
        for (path, want) in paths.iter().zip(&desired) {
            let result = match want {
                Some(content) => write_atomic(path, content),
                None => restore_file(path, None),
            };
            if let Err(err) = result {
                restore_all();
                return Err(err);
            }
        }
        match test_openresty(&runtime) {
            Ok(()) => {
                if let Err(err) = reload_openresty(&runtime) {
                    restore_all();
                    let _ = reload_openresty(&runtime);
                    return Err(anyhow!("OpenResty reload failed: {}", err));
                }
                return Ok(describe_level(level));
            }
            Err(err) => last_error = err.to_string(),
        }
    }
    restore_all();
    Err(anyhow!("OpenResty config test failed: {}", last_error))
}

fn describe_level(level: u8) -> String {
    match level {
        2 => "Per-site logs, rate limits and connection timeouts are active",
        1 => "Per-site logs and rate limits are active (timeouts are set in nginx.conf)",
        _ => "nginx rejected the defense settings; sites run without them",
    }
    .into()
}

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let mut temp_path = path.to_path_buf();
    temp_path.set_extension(format!(
        "tmp.{}.{}",
        std::process::id(),
        chrono::Utc::now().timestamp_micros()
    ));

    fs::write(&temp_path, content)
        .with_context(|| format!("Failed to write temporary config: {}", temp_path.display()))?;
    if path.exists() {
        fs::remove_file(path)
            .with_context(|| format!("Failed to replace config: {}", path.display()))?;
    }
    fs::rename(&temp_path, path)
        .with_context(|| format!("Failed to move config into place: {}", path.display()))?;
    Ok(())
}

fn restore_file(path: &Path, previous: Option<&str>) -> Result<()> {
    match previous {
        Some(content) => fs::write(path, content)
            .with_context(|| format!("Failed to restore previous config: {}", path.display()))?,
        None => {
            if path.exists() {
                fs::remove_file(path).with_context(|| {
                    format!("Failed to remove invalid config: {}", path.display())
                })?;
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn detect_install_privilege_mode() -> Result<bool> {
    let uid_output = Command::new("id")
        .arg("-u")
        .output()
        .context("Failed to check current user")?;
    if !uid_output.status.success() {
        return Err(anyhow!("Failed to check current user"));
    }
    let uid = String::from_utf8_lossy(&uid_output.stdout)
        .trim()
        .to_string();
    if uid == "0" {
        return Ok(false);
    }

    let sudo_ok = Command::new("sudo")
        .arg("-n")
        .arg("true")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if sudo_ok {
        return Ok(true);
    }

    Err(anyhow!(
        "OpenResty quick install requires root privileges or passwordless sudo"
    ))
}

#[cfg(target_os = "linux")]
fn install_openresty_package() -> Result<String> {
    use std::os::unix::fs::OpenOptionsExt;
    let use_sudo = detect_install_privilege_mode()?;
    // From a private file rather than stdin: a command in the script that
    // reads stdin would otherwise swallow the rest of the script.
    let path = std::env::temp_dir().join(format!(
        "linxpanel-openresty-{}.sh",
        uuid::Uuid::new_v4().simple()
    ));
    {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .open(&path)
            .context("Cannot write the OpenResty install script")?;
        file.write_all(QUICK_INSTALL_SCRIPT.as_bytes())?;
    }
    let shell = if super::shell::has_binary("bash") {
        "bash"
    } else {
        "sh"
    };
    let mut command = Command::new(shell);
    command
        .arg(&path)
        .env("USE_SUDO", if use_sudo { "1" } else { "0" });
    // Through the shared runner, so the task window streams the output.
    let output = super::shell::run_command(command, None, std::time::Duration::from_secs(45 * 60));
    let _ = fs::remove_file(&path);
    let output =
        output.map_err(|e| anyhow!("Failed to run the OpenResty install script: {}", e))?;

    let text = format!("{}{}", output.stdout, output.stderr);
    if output.ok() {
        return Ok(text);
    }
    Err(anyhow!(summarize_linux_install_error(&text)))
}

#[cfg(target_os = "linux")]
fn summarize_linux_install_error(full_log: &str) -> String {
    let lower = full_log.to_ascii_lowercase();
    if lower.contains("could not get lock")
        || lower.contains("unable to acquire the dpkg frontend lock")
    {
        return "OpenResty install failed: another package operation held the apt lock for over 10 minutes; retry when it has finished".to_string();
    }
    if lower.contains("dpkg was interrupted")
        || lower.contains("you must manually run 'sudo dpkg --configure -a'")
    {
        return "OpenResty install failed: the package system is in a broken state; run `dpkg --configure -a && apt-get -f install -y` on the server, then retry".to_string();
    }
    // The script narrates each step; the last real one says where it stopped.
    let reason = full_log
        .lines()
        .rev()
        .filter_map(|line| line.trim().strip_prefix("[LinxPanel] "))
        .find(|line| {
            !line.contains("source fallback failed")
                && !line.starts_with("Building ")
                && !line.contains("falling back to a source build")
        })
        .or_else(|| {
            full_log
                .lines()
                .rev()
                .map(str::trim)
                .find(|l| !l.is_empty())
        })
        .unwrap_or("no output");
    format!(
        "OpenResty install failed: {} (the full log is in the task window)",
        reason
    )
}

#[cfg(target_os = "windows")]
fn install_openresty_package() -> Result<String> {
    if !is_windows_admin()? {
        return Err(anyhow!(
            "OpenResty quick install requires Administrator privileges"
        ));
    }

    let mut log = String::new();
    log.push_str("[LinxPanel] Starting OpenResty quick install (Windows)\n");

    let mut attempted = false;
    let mut success = false;

    if command_exists("winget") {
        attempted = true;
        log.push_str("[LinxPanel] Using winget to install OpenResty...\n");
        let output = Command::new("winget")
            .args([
                "install",
                "--id",
                "OpenResty.OpenResty",
                "-e",
                "--silent",
                "--accept-package-agreements",
                "--accept-source-agreements",
            ])
            .output()
            .context("Failed to execute winget")?;
        let result_text = combine_output(&output);
        if !result_text.trim().is_empty() {
            log.push_str(&result_text);
            log.push('\n');
        }
        if output.status.success() {
            success = has_openresty_binary_now();
            if !success {
                log.push_str("[LinxPanel] winget finished but OpenResty binary still missing.\n");
            }
        }
    } else {
        log.push_str("[LinxPanel] winget not found, skip.\n");
    }

    if !success && command_exists("choco") {
        attempted = true;
        log.push_str("[LinxPanel] Using choco to install OpenResty...\n");
        let output = Command::new("choco")
            .args(["install", "openresty", "-y", "--no-progress"])
            .output()
            .context("Failed to execute choco")?;
        let result_text = combine_output(&output);
        if !result_text.trim().is_empty() {
            log.push_str(&result_text);
            log.push('\n');
        }
        if output.status.success() {
            success = has_openresty_binary_now();
            if !success {
                log.push_str("[LinxPanel] choco finished but OpenResty binary still missing.\n");
            }
        }
    } else if !success {
        log.push_str("[LinxPanel] choco not found, skip.\n");
    }

    if !success && command_exists("scoop") {
        attempted = true;
        log.push_str("[LinxPanel] Using scoop to install OpenResty...\n");
        let output = Command::new("scoop")
            .args(["install", "openresty"])
            .output()
            .context("Failed to execute scoop")?;
        let result_text = combine_output(&output);
        if !result_text.trim().is_empty() {
            log.push_str(&result_text);
            log.push('\n');
        }
        if output.status.success() {
            success = has_openresty_binary_now();
            if !success {
                log.push_str("[LinxPanel] scoop finished but OpenResty binary still missing.\n");
            }
        }
    } else if !success {
        log.push_str("[LinxPanel] scoop not found, skip.\n");
    }

    if !success {
        attempted = true;
        log.push_str(
            "[LinxPanel] Trying fallback install from official OpenResty Windows package...\n",
        );
        match install_openresty_from_official_zip() {
            Ok(fallback_log) => {
                if !fallback_log.trim().is_empty() {
                    log.push_str(&fallback_log);
                    if !fallback_log.ends_with('\n') {
                        log.push('\n');
                    }
                }
                success = has_openresty_binary_now();
                if !success {
                    log.push_str("[LinxPanel] Fallback installer finished but OpenResty binary still missing.\n");
                }
            }
            Err(error) => {
                log.push_str(&format!(
                    "[LinxPanel] Fallback installer failed: {}\n",
                    error
                ));
            }
        }
    }

    if !attempted {
        return Err(anyhow!("No installer method available"));
    }
    if !success {
        return Err(anyhow!(
            "All OpenResty quick install methods failed. Please review install logs."
        ));
    }

    Ok(log)
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
fn install_openresty_package() -> Result<String> {
    Err(anyhow!(
        "OpenResty quick install is supported on Windows and Linux only"
    ))
}

#[cfg(target_os = "windows")]
fn is_windows_admin() -> Result<bool> {
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "[bool]([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)",
        ])
        .output()
        .context("Failed to detect Administrator permissions")?;
    if !output.status.success() {
        return Ok(false);
    }
    let text = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_ascii_lowercase();
    Ok(text == "true")
}

#[cfg(target_os = "windows")]
fn install_openresty_from_official_zip() -> Result<String> {
    let script = r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

Write-Output '[LinxPanel] Download index: https://openresty.org/download/'
$indexRes = Invoke-WebRequest -UseBasicParsing -Uri 'https://openresty.org/download/'
$matches = [regex]::Matches($indexRes.Content, 'href="(?<url>/download/openresty-[0-9\.]+(?:\.[0-9]+)?-win64\.zip)"')
if ($matches.Count -eq 0) {
  throw 'No official OpenResty Windows package found on download page.'
}

$urls = @()
foreach ($m in $matches) {
  $u = $m.Groups['url'].Value
  if ([string]::IsNullOrWhiteSpace($u)) { continue }
  if ($u -notmatch '^https?://') { $u = 'https://openresty.org' + $u }
  $urls += $u
}
$urls = $urls | Select-Object -Unique

$selected = $urls | Sort-Object {
  if ($_ -match 'openresty-(?<v>[0-9\.]+)-win64') {
    $parts = $Matches['v'].Split('.') | ForEach-Object { [int]$_ }
    while ($parts.Count -lt 4) { $parts += 0 }
    return [long]($parts[0] * 1000000000 + $parts[1] * 1000000 + $parts[2] * 1000 + $parts[3])
  }
  return [long]0
} -Descending | Select-Object -First 1

if ([string]::IsNullOrWhiteSpace($selected)) {
  throw 'Cannot resolve latest OpenResty Windows package URL.'
}
Write-Output ('[LinxPanel] Download package: ' + $selected)

$tmpRoot = Join-Path $env:TEMP ('linxpanel-openresty-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmpRoot | Out-Null
$zipPath = Join-Path $tmpRoot 'openresty-win64.zip'
$extractPath = Join-Path $tmpRoot 'extract'

Invoke-WebRequest -UseBasicParsing -Uri $selected -OutFile $zipPath
Expand-Archive -Path $zipPath -DestinationPath $extractPath -Force

$pkgDir = Get-ChildItem -Path $extractPath -Directory | Select-Object -First 1
if (-not $pkgDir) {
  throw 'OpenResty package extraction failed.'
}

$target = 'C:\openresty'
if (Test-Path $target) {
  Write-Output '[LinxPanel] Existing C:\openresty found, trying to stop nginx process...'
  Get-Process nginx -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
  Remove-Item -Path $target -Recurse -Force
}
Move-Item -Path $pkgDir.FullName -Destination $target -Force

$nginxExe = Join-Path $target 'nginx\nginx.exe'
if (-not (Test-Path $nginxExe)) {
  throw 'OpenResty binary not found after extraction.'
}
Write-Output ('[LinxPanel] OpenResty binary ready: ' + $nginxExe)
Write-Output '[LinxPanel] Fallback install completed.'
"#;

    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
        .context("Failed to execute OpenResty fallback installer")?;
    let text = combine_output(&output);
    if output.status.success() {
        return Ok(text);
    }
    let detail =
        last_non_empty_line(&text).unwrap_or_else(|| format!("Exit status: {}", output.status));
    Err(anyhow!(detail))
}

fn combine_output(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let mut text = String::new();
    if !stdout.trim().is_empty() {
        text.push_str(stdout.trim_end());
    }
    if !stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(stderr.trim_end());
    }
    text
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn last_non_empty_line(text: &str) -> Option<String> {
    text.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.trim().to_string())
}

fn prepare_sites_dir(
    conf_path: &Path,
    configured_sites_dir: &Path,
    log: &mut String,
) -> Result<PathBuf> {
    if conf_path.exists() {
        let conf_content = fs::read_to_string(conf_path)
            .with_context(|| format!("Failed to read nginx config: {}", conf_path.display()))?;
        let include_dirs = parse_include_dirs(conf_path, &conf_content);
        if let Some(selected_dir) = pick_preferred_include_dir(&include_dirs) {
            fs::create_dir_all(&selected_dir).with_context(|| {
                format!("Failed to create include dir: {}", selected_dir.display())
            })?;
            log.push_str(&format!(
                "[LinxPanel] Using nginx include directory: {}\n",
                selected_dir.display()
            ));
            return Ok(selected_dir);
        }

        let fallback_dir = conf_path
            .parent()
            .map(|parent| parent.join("sites-enabled"))
            .unwrap_or_else(|| configured_sites_dir.to_path_buf());
        fs::create_dir_all(&fallback_dir).with_context(|| {
            format!(
                "Failed to create sites directory: {}",
                fallback_dir.display()
            )
        })?;

        let changed = ensure_sites_include_in_http(conf_path, &fallback_dir)?;
        if changed {
            log.push_str("[LinxPanel] Added sites include directive into nginx http block.\n");
        } else {
            log.push_str("[LinxPanel] Nginx config already contains sites include directive.\n");
        }
        return Ok(fallback_dir);
    }

    let fallback_dir = if configured_sites_dir.as_os_str().is_empty() {
        PathBuf::from(DEFAULT_OPENRESTY_SITES_DIR)
    } else {
        configured_sites_dir.to_path_buf()
    };
    fs::create_dir_all(&fallback_dir).with_context(|| {
        format!(
            "Failed to create sites directory: {}",
            fallback_dir.display()
        )
    })?;
    log.push_str(&format!(
        "[LinxPanel] Main config not found ({}), using configured sites dir: {}\n",
        conf_path.display(),
        fallback_dir.display()
    ));
    Ok(fallback_dir)
}

fn parse_include_dirs(conf_path: &Path, content: &str) -> Vec<PathBuf> {
    let base_dir = conf_path.parent().unwrap_or_else(|| Path::new("/"));
    let mut dirs = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for raw_line in content.lines() {
        let line = strip_comment(raw_line).trim();
        if !line.starts_with("include ") {
            continue;
        }

        let mut value = line.trim_start_matches("include").trim();
        value = value.trim_end_matches(';').trim();
        value = value.trim_matches('"').trim_matches('\'').trim();
        if value.is_empty() || value.contains('$') {
            continue;
        }

        let include_dir = if let Some(star_idx) = value.find('*') {
            if !value.contains(".conf") {
                None
            } else {
                let prefix = value[..star_idx].trim_end_matches('/').trim();
                if prefix.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(prefix))
                }
            }
        } else if value.ends_with(".conf")
            && (value.contains("sites-enabled") || value.contains("conf.d"))
        {
            Path::new(value).parent().map(Path::to_path_buf)
        } else {
            None
        };

        if let Some(mut dir) = include_dir {
            if !dir.is_absolute() {
                dir = base_dir.join(dir);
            }
            let normalized = normalize_path_for_compare(&dir);
            if seen.insert(normalized) {
                dirs.push(dir);
            }
        }
    }

    dirs
}

fn pick_preferred_include_dir(dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter()
        .find(|path| path_contains(path, "sites-enabled"))
        .cloned()
        .or_else(|| {
            dirs.iter()
                .find(|path| path_contains(path, "conf.d"))
                .cloned()
        })
        .or_else(|| dirs.first().cloned())
}

fn path_contains(path: &Path, needle: &str) -> bool {
    path.to_string_lossy()
        .to_ascii_lowercase()
        .contains(&needle.to_ascii_lowercase())
}

fn ensure_sites_include_in_http(conf_path: &Path, sites_dir: &Path) -> Result<bool> {
    let content = fs::read_to_string(conf_path)
        .with_context(|| format!("Failed to read nginx config: {}", conf_path.display()))?;
    let normalized_sites_dir = normalize_path_for_compare(sites_dir);
    if content.replace('\\', "/").contains(&normalized_sites_dir) {
        return Ok(false);
    }

    let include_glob = format!("{}/{}", normalized_sites_dir, "*.conf");
    let include_line = format!("include {};", quote_nginx_value(&include_glob));

    let mut lines: Vec<String> = content.lines().map(|line| line.to_string()).collect();
    let http_start = lines
        .iter()
        .position(|line| is_http_block_start(line))
        .ok_or_else(|| anyhow!("Cannot locate http block in {}", conf_path.display()))?;
    let http_end = find_block_end_index(&lines, http_start)
        .ok_or_else(|| anyhow!("Cannot locate end of http block in {}", conf_path.display()))?;

    let indent = detect_block_indent(&lines, http_start);
    lines.insert(http_end, format!("{}{}", indent, include_line));

    let mut updated = lines.join("\n");
    if content.ends_with('\n') {
        updated.push('\n');
    }
    write_atomic(conf_path, &updated)?;
    Ok(true)
}

fn is_http_block_start(line: &str) -> bool {
    let line = strip_comment(line).trim_start();
    let Some(rest) = line.strip_prefix("http") else {
        return false;
    };
    rest.trim_start().starts_with('{')
}

fn find_block_end_index(lines: &[String], start_index: usize) -> Option<usize> {
    let mut depth: i32 = 0;
    let mut entered = false;

    for (index, line) in lines.iter().enumerate().skip(start_index) {
        let text = strip_comment(line);
        for ch in text.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    entered = true;
                }
                '}' => {
                    depth -= 1;
                    if entered && depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
        }
    }

    None
}

fn detect_block_indent(lines: &[String], block_start: usize) -> String {
    for line in lines.iter().skip(block_start + 1) {
        let no_comment = strip_comment(line);
        if no_comment.trim().is_empty() {
            continue;
        }
        if no_comment.trim_start().starts_with('}') {
            break;
        }
        let spaces = line.len().saturating_sub(line.trim_start().len());
        return " ".repeat(spaces);
    }
    "    ".to_string()
}

fn strip_comment(line: &str) -> &str {
    line.split('#').next().unwrap_or(line)
}

fn normalize_path_for_compare(path: &Path) -> String {
    let normalized = path
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_string();
    if normalized.is_empty() {
        "/".to_string()
    } else {
        normalized
    }
}

async fn render_site_config(
    db: &SqlitePool,
    site: &Site,
    runtime: &OpenRestyRuntime,
    defense: &SiteDefense,
) -> Result<String> {
    let domains = load_site_domains(db, site).await?;
    if domains.is_empty() {
        return Err(anyhow!("No valid domains available"));
    }

    let mut grouped: BTreeMap<u16, Vec<String>> = BTreeMap::new();
    let mut seen: HashSet<(u16, String)> = HashSet::new();
    for domain in domains {
        if let Some(binding) = parse_domain_binding(&domain) {
            if seen.insert((binding.port, binding.host.clone())) {
                grouped.entry(binding.port).or_default().push(binding.host);
            }
        }
    }
    if grouped.is_empty() {
        return Err(anyhow!("No valid domains available"));
    }

    // App-backed sites need their application's listen port to build the upstream.
    let app_port: Option<i64> = match site.app_id {
        Some(app_id) => sqlx::query_as::<_, (i64,)>("SELECT port FROM apps WHERE id = ?")
            .bind(app_id)
            .fetch_optional(db)
            .await?
            .map(|(port,)| port)
            .filter(|port| *port > 0),
        None => None,
    };

    let mut blocks = String::new();
    blocks.push_str("# Generated by LinxPanel\n");
    blocks.push_str(&format!("# site_id: {}\n", site.id));
    let index_files =
        normalize_index_files_for_nginx(site.index_files.as_str(), site.site_type.as_str());
    let php_index_file = index_files
        .split_whitespace()
        .find(|item| item.ends_with(".php"))
        .unwrap_or("index.php");
    let is_stopped = site.status != "running";

    let tls_enabled =
        site.ssl_enabled && site.ssl_cert_path.is_some() && site.ssl_key_path.is_some();

    for (port, hosts) in grouped {
        // With TLS on, plain HTTP on the same port would be unreachable, so emit a
        // small redirect server on :80 instead of silently dropping it.
        if tls_enabled && site.force_https && port != 80 {
            blocks.push('\n');
            blocks.push_str("server {\n");
            blocks.push_str("    listen 80;\n");
            if runtime.listen_ipv6 {
                blocks.push_str("    listen [::]:80;\n");
            }
            blocks.push_str(&format!("    server_name {};\n", hosts.join(" ")));
            blocks.push_str(&defense.server_directives(site.id));
            // Keep ACME HTTP-01 working: renewals must not be redirected away.
            blocks.push_str("    location ^~ /.well-known/acme-challenge/ {\n");
            blocks.push_str(&format!(
                "        root {};\n",
                quote_nginx_value(&site.doc_root)
            ));
            blocks.push_str("    }\n");
            blocks.push_str("    location / {\n");
            blocks.push_str("        return 301 https://$host$request_uri;\n");
            blocks.push_str("    }\n");
            blocks.push_str("}\n");
        }

        blocks.push('\n');
        blocks.push_str("server {\n");

        if tls_enabled {
            blocks.push_str(&format!("    listen {} ssl;\n", port));
            if runtime.listen_ipv6 {
                blocks.push_str(&format!("    listen [::]:{} ssl;\n", port));
            }
            blocks.push_str(&format!(
                "    ssl_certificate {};\n",
                quote_nginx_value(site.ssl_cert_path.as_deref().unwrap_or_default())
            ));
            blocks.push_str(&format!(
                "    ssl_certificate_key {};\n",
                quote_nginx_value(site.ssl_key_path.as_deref().unwrap_or_default())
            ));
            // Modern-but-compatible defaults: TLS 1.2+ with forward secrecy only.
            blocks.push_str("    ssl_protocols TLSv1.2 TLSv1.3;\n");
            blocks.push_str("    ssl_prefer_server_ciphers off;\n");
            blocks.push_str(
                "    ssl_ciphers ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305;\n",
            );
            blocks.push_str("    ssl_session_cache shared:LinxPanelSSL:10m;\n");
            blocks.push_str("    ssl_session_timeout 1d;\n");
            blocks.push_str("    ssl_session_tickets off;\n");
            if site.hsts {
                blocks.push_str(
                    "    add_header Strict-Transport-Security \"max-age=31536000; includeSubDomains\" always;\n",
                );
            }
        } else {
            blocks.push_str(&format!("    listen {};\n", port));
            if runtime.listen_ipv6 {
                blocks.push_str(&format!("    listen [::]:{};\n", port));
            }
        }

        blocks.push_str(&format!("    server_name {};\n", hosts.join(" ")));
        blocks.push_str(&defense.server_directives(site.id));
        // nginx's 1MB default breaks media uploads (WordPress, CMS admin panels).
        blocks.push_str("    client_max_body_size 256m;\n");

        if is_stopped {
            let stop_html =
                default_stopped_page_html(site.name.as_str(), site.stopped_page_html.as_deref());
            blocks.push_str("    default_type text/html;\n");
            blocks.push_str("    charset utf-8;\n");
            blocks.push_str("    add_header Cache-Control \"no-store, no-cache, must-revalidate, max-age=0\" always;\n");
            blocks.push_str("    add_header Pragma \"no-cache\" always;\n");
            blocks.push_str("    add_header Expires \"0\" always;\n");
            blocks.push_str("    location / {\n");
            blocks.push_str(&format!(
                "        return 200 {};\n",
                quote_nginx_single(stop_html.as_str())
            ));
            blocks.push_str("    }\n");
            blocks.push_str("}\n");
            continue;
        }

        // Never serve dotfiles (.env, .git, .htaccess...) - except ACME challenges.
        if !matches!(
            site.site_type.as_str(),
            "proxy" | "node" | "python" | "java" | "go" | "binary"
        ) {
            blocks.push_str("    location ~ /\\.(?!well-known/) {\n");
            blocks.push_str("        deny all;\n");
            blocks.push_str("    }\n");
        }

        // Rewrite ("pseudo-static") rules sit before the location blocks so they apply first.
        if let Some(rules) = site.rewrite_rules.as_deref() {
            let rendered = render_rewrite_rules(rules)?;
            if !rendered.is_empty() {
                blocks.push_str(&rendered);
            }
        }

        match site.site_type.as_str() {
            // Application runtimes are served by reverse-proxying to the managed process.
            "node" | "python" | "java" | "go" | "binary" => {
                let port = app_port.ok_or_else(|| {
                    anyhow!("This site type requires a linked application with a port")
                })?;
                let upstream = format!("http://127.0.0.1:{}", port);
                blocks.push_str("    location / {\n");
                blocks.push_str(&format!("        proxy_pass {};\n", upstream));
                blocks.push_str("        proxy_http_version 1.1;\n");
                // Carry WebSocket upgrades through; app frameworks commonly need this.
                blocks.push_str("        proxy_set_header Upgrade $http_upgrade;\n");
                // Forward the client's own Connection header: using $connection_upgrade
                // would need an http-level `map` that a per-site include cannot define.
                blocks.push_str("        proxy_set_header Connection $http_connection;\n");
                blocks.push_str("        proxy_set_header Host $host;\n");
                blocks.push_str("        proxy_set_header X-Real-IP $remote_addr;\n");
                blocks.push_str(
                    "        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n",
                );
                blocks.push_str("        proxy_set_header X-Forwarded-Proto $scheme;\n");
                blocks.push_str("        proxy_read_timeout 120s;\n");
                blocks.push_str("    }\n");
            }
            "proxy" => {
                let proxy = site.proxy_pass.as_deref().unwrap_or("").trim().to_string();
                if proxy.is_empty() {
                    return Err(anyhow!("Proxy address is required"));
                }
                if has_unsafe_nginx_chars(&proxy) {
                    return Err(anyhow!("Proxy address contains invalid characters"));
                }
                blocks.push_str("    location / {\n");
                blocks.push_str(&format!("        proxy_pass {};\n", proxy));
                blocks.push_str("        proxy_set_header Host $host;\n");
                blocks.push_str("        proxy_set_header X-Real-IP $remote_addr;\n");
                blocks.push_str(
                    "        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n",
                );
                blocks.push_str("        proxy_set_header X-Forwarded-Proto $scheme;\n");
                blocks.push_str("    }\n");
            }
            "php" => {
                blocks.push_str(&format!(
                    "    root {};\n",
                    quote_nginx_value(&site.doc_root)
                ));
                blocks.push_str(&format!("    index {};\n", index_files));
                blocks.push_str("    location / {\n");
                blocks.push_str(&format!(
                    "        try_files $uri $uri/ /{}?$query_string;\n",
                    php_index_file
                ));
                blocks.push_str("    }\n");
                blocks.push_str("    location ~ \\.php$ {\n");
                blocks.push_str("        include fastcgi_params;\n");
                blocks.push_str(
                    "        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;\n",
                );
                blocks.push_str(&format!("        fastcgi_index {};\n", php_index_file));
                blocks.push_str(&format!("        fastcgi_pass {};\n", runtime.php_fpm_addr));
                // Imports, updates and installers routinely exceed nginx's 60s default.
                blocks.push_str("        fastcgi_read_timeout 300s;\n");
                blocks.push_str("    }\n");
            }
            _ => {
                blocks.push_str(&format!(
                    "    root {};\n",
                    quote_nginx_value(&site.doc_root)
                ));
                blocks.push_str(&format!("    index {};\n", index_files));
                blocks.push_str("    location / {\n");
                blocks.push_str("        try_files $uri $uri/ =404;\n");
                blocks.push_str("    }\n");
            }
        }

        if let Some(extra) = site
            .extra_config
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            blocks.push('\n');
            for line in extra.lines() {
                blocks.push_str("    ");
                blocks.push_str(line);
                blocks.push('\n');
            }
        }
        blocks.push_str("}\n");
    }

    Ok(blocks)
}

async fn load_site_domains(db: &SqlitePool, site: &Site) -> Result<Vec<String>> {
    let rows = sqlx::query_as::<_, Domain>(
        "SELECT * FROM domains WHERE site_id = ? ORDER BY sort_order ASC, id ASC",
    )
    .bind(site.id)
    .fetch_all(db)
    .await
    .context("Failed to load site domains")?;

    if !rows.is_empty() {
        return Ok(rows.into_iter().map(|item| item.domain).collect());
    }

    Ok(site
        .domains
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

fn test_openresty(runtime: &OpenRestyRuntime) -> Result<()> {
    run_openresty(runtime, ["-t"].as_slice())
}

/// Applies a new configuration. When no master process is running (stopped,
/// crashed, or not started after a reboot) a reload cannot work, so the
/// server is started instead.
fn reload_openresty(runtime: &OpenRestyRuntime) -> Result<()> {
    match run_openresty(runtime, ["-s", "reload"].as_slice()) {
        Ok(()) => Ok(()),
        Err(err) if not_running(&err.to_string()) => start_openresty(runtime),
        Err(err) => Err(err),
    }
}

/// nginx's ways of saying there is no master process to signal.
fn not_running(error: &str) -> bool {
    error.contains("No such process")
        || error.contains("invalid PID number")
        || (error.contains("nginx.pid") && error.contains("failed"))
}

fn start_openresty(runtime: &OpenRestyRuntime) -> Result<()> {
    // Prefer the service, so the init system keeps supervising it.
    if super::sysservice::exists("openresty") {
        return super::sysservice::enable_and_restart("openresty")
            .map(|_| ())
            .map_err(|e| anyhow!("Cannot start OpenResty: {}", e));
    }
    run_openresty(runtime, [].as_slice()).context("Cannot start OpenResty")
}

fn run_openresty(runtime: &OpenRestyRuntime, args: &[&str]) -> Result<()> {
    let mut command = Command::new(&runtime.nginx_bin);
    command.args(args);
    if let Some(conf_path) = runtime.nginx_conf.as_ref() {
        command.arg("-c").arg(conf_path);
    }

    let output = command
        .output()
        .with_context(|| format!("Failed to execute {}", runtime.nginx_bin))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("Exit status: {}", output.status)
    };
    Err(anyhow!(detail))
}

async fn persist_install_settings(
    db: &SqlitePool,
    nginx_bin: &Path,
    nginx_conf: Option<&Path>,
    sites_dir: &Path,
) -> Result<()> {
    save_setting(db, SETTING_OPENRESTY_ENABLED, "true").await?;
    save_setting(db, SETTING_OPENRESTY_BIN, &nginx_bin.to_string_lossy()).await?;
    save_setting(
        db,
        SETTING_OPENRESTY_SITES_DIR,
        &sites_dir.to_string_lossy(),
    )
    .await?;
    save_setting(
        db,
        SETTING_OPENRESTY_CONF_PATH,
        &nginx_conf
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default(),
    )
    .await?;
    Ok(())
}

async fn save_setting(db: &SqlitePool, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .execute(db)
    .await
    .with_context(|| format!("Failed to save setting '{}'", key))?;
    Ok(())
}

async fn inspect_runtime(db: &SqlitePool) -> Result<RuntimeInspection> {
    let runtime = load_runtime(db).await?;
    let resolved_nginx_bin =
        resolve_openresty_bin().or_else(|| resolve_nginx_bin(&runtime.nginx_bin));

    let detected_conf_path = match runtime.nginx_conf.clone() {
        Some(path) => Some(path),
        None => resolved_nginx_bin
            .as_ref()
            .and_then(|bin| detect_conf_path(bin).ok().flatten()),
    };
    let detected_version = resolved_nginx_bin
        .as_ref()
        .and_then(|bin| detect_binary_version(bin).ok().flatten());
    let is_openresty = resolved_nginx_bin
        .as_ref()
        .map(|bin| {
            is_openresty_binary_path(bin)
                || detected_version
                    .as_deref()
                    .map(version_mentions_openresty)
                    .unwrap_or(false)
                || run_binary_capture(bin, ["-V"].as_slice())
                    .map(|value| version_mentions_openresty(&value))
                    .unwrap_or(false)
        })
        .unwrap_or(false);

    Ok(RuntimeInspection {
        runtime,
        resolved_nginx_bin,
        detected_conf_path,
        detected_version,
        is_openresty,
    })
}

fn to_public_status(inspection: RuntimeInspection) -> OpenRestyStatus {
    let supported = cfg!(any(target_os = "linux", target_os = "windows"));
    let installed = inspection.is_openresty;

    let nginx_bin = inspection
        .resolved_nginx_bin
        .as_ref()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| inspection.runtime.nginx_bin.clone());

    let nginx_conf = inspection
        .detected_conf_path
        .or(inspection.runtime.nginx_conf.clone())
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();

    let message = if !supported {
        "OpenResty sync is supported on Windows and Linux only".to_string()
    } else if installed {
        format!("OpenResty detected at {}", nginx_bin)
    } else if !nginx_bin.is_empty() && Path::new(&nginx_bin).exists() {
        format!(
            "OpenResty is not installed (detected non-OpenResty nginx: {})",
            nginx_bin
        )
    } else {
        "OpenResty is not installed".to_string()
    };

    OpenRestyStatus {
        supported,
        installed,
        enabled: inspection.runtime.enabled,
        nginx_bin,
        nginx_conf,
        sites_dir: inspection.runtime.sites_dir.to_string_lossy().to_string(),
        version: inspection.detected_version,
        message,
    }
}

fn resolve_nginx_bin(preferred: &str) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if !preferred.trim().is_empty() {
        candidates.push(PathBuf::from(preferred.trim()));
    }
    candidates.push(PathBuf::from(DEFAULT_OPENRESTY_BIN));
    #[cfg(target_os = "windows")]
    {
        candidates.push(PathBuf::from(
            "C:\\Program Files\\OpenResty\\nginx\\nginx.exe",
        ));
        candidates.push(PathBuf::from("C:\\openresty\\nginx\\nginx.exe"));
    }
    #[cfg(not(target_os = "windows"))]
    {
        candidates.push(PathBuf::from("/www/server/openresty/nginx/sbin/nginx"));
        candidates.push(PathBuf::from("/usr/local/openresty/bin/openresty"));
    }
    candidates.push(PathBuf::from("/usr/sbin/openresty"));
    candidates.push(PathBuf::from("/usr/bin/openresty"));
    candidates.push(PathBuf::from("/usr/sbin/nginx"));
    candidates.push(PathBuf::from("/usr/bin/nginx"));
    if let Some(path) = find_in_path("openresty") {
        candidates.push(path);
    }
    if let Some(path) = find_in_path("nginx") {
        candidates.push(path);
    }

    let mut seen: HashSet<String> = HashSet::new();
    for candidate in candidates {
        let normalized = normalize_path_for_compare(&candidate);
        if !seen.insert(normalized) {
            continue;
        }
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn resolve_openresty_bin() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(PathBuf::from(DEFAULT_OPENRESTY_BIN));
    #[cfg(not(target_os = "windows"))]
    {
        candidates.push(PathBuf::from("/www/server/openresty/nginx/sbin/nginx"));
        candidates.push(PathBuf::from("/usr/local/openresty/nginx/sbin/nginx"));
        candidates.push(PathBuf::from("/usr/local/openresty/bin/openresty"));
    }
    candidates.push(PathBuf::from("/usr/sbin/openresty"));
    candidates.push(PathBuf::from("/usr/bin/openresty"));
    if let Some(path) = find_in_path("openresty") {
        candidates.push(path);
    }

    let mut seen: HashSet<String> = HashSet::new();
    for candidate in candidates {
        let normalized = normalize_path_for_compare(&candidate);
        if !seen.insert(normalized) {
            continue;
        }
        if !is_executable_file(&candidate) {
            continue;
        }
        if is_openresty_binary_path(&candidate) {
            return Some(candidate);
        }
        if detect_binary_version(&candidate)
            .ok()
            .flatten()
            .as_deref()
            .map(version_mentions_openresty)
            .unwrap_or(false)
        {
            return Some(candidate);
        }
        if run_binary_capture(&candidate, ["-V"].as_slice())
            .map(|text| version_mentions_openresty(&text))
            .unwrap_or(false)
        {
            return Some(candidate);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn has_openresty_binary_now() -> bool {
    let Some(binary) = resolve_openresty_bin().or_else(|| resolve_nginx_bin(DEFAULT_OPENRESTY_BIN))
    else {
        return false;
    };
    if is_openresty_binary_path(&binary) {
        return true;
    }
    detect_binary_version(&binary)
        .ok()
        .flatten()
        .map(|value| version_mentions_openresty(&value))
        .unwrap_or(false)
}

fn is_openresty_binary_path(path: &Path) -> bool {
    path.to_string_lossy()
        .to_ascii_lowercase()
        .contains("openresty")
}

fn version_mentions_openresty(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("openresty/") || lower.contains("openresty ") || lower.contains("openresty-")
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_env = std::env::var_os("PATH")?;
    let entries = std::env::split_paths(&path_env);
    for dir in entries {
        let candidate = dir.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
        #[cfg(target_os = "windows")]
        {
            let candidate_exe = dir.join(format!("{}.exe", name));
            if is_executable_file(&candidate_exe) {
                return Some(candidate_exe);
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn command_exists(name: &str) -> bool {
    find_in_path(name).is_some()
}

#[cfg(target_family = "unix")]
fn is_executable_file(path: &Path) -> bool {
    if !path.exists() || !path.is_file() {
        return false;
    }
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(target_family = "unix"))]
fn is_executable_file(path: &Path) -> bool {
    path.exists() && path.is_file()
}

fn detect_binary_version(binary: &Path) -> Result<Option<String>> {
    let output = run_binary_capture(binary, ["-v"].as_slice())?;
    if let Some(version) = extract_version_token(&output, "openresty/") {
        return Ok(Some(version));
    }
    if let Some(version) = extract_version_token(&output, "nginx/") {
        return Ok(Some(version));
    }
    Ok(output
        .lines()
        .find(|line| line.contains("version"))
        .map(|line| line.trim().to_string()))
}

fn extract_version_token(text: &str, marker: &str) -> Option<String> {
    let start = text.find(marker)?;
    let token = text[start..]
        .split_whitespace()
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(token.to_string())
}

fn detect_conf_path(binary: &Path) -> Result<Option<PathBuf>> {
    let output = run_binary_capture(binary, ["-V"].as_slice())?;
    let mut conf_path: Option<PathBuf> = None;
    let mut prefix: Option<PathBuf> = None;

    for token in output.split_whitespace() {
        if let Some(value) = token.strip_prefix("--conf-path=") {
            let value = value.trim_matches('"').trim_matches('\'');
            if !value.is_empty() {
                conf_path = Some(PathBuf::from(value));
            }
        }
        if let Some(value) = token.strip_prefix("--prefix=") {
            let value = value.trim_matches('"').trim_matches('\'');
            if !value.is_empty() {
                prefix = Some(PathBuf::from(value));
            }
        }
    }

    let conf_path = conf_path.map(|path| {
        if path.is_absolute() {
            path
        } else if let Some(prefix) = prefix {
            prefix.join(path)
        } else {
            path
        }
    });

    Ok(conf_path)
}

fn guess_conf_path_from_bin(binary: &Path) -> Option<PathBuf> {
    let binary_text = binary.to_string_lossy();
    #[cfg(target_os = "windows")]
    {
        if is_openresty_binary_path(binary) {
            if let Some(nginx_dir) = binary.parent() {
                let candidate = nginx_dir.join("conf").join("nginx.conf");
                if candidate.exists() {
                    return Some(candidate);
                }
            }
            return Some(PathBuf::from("C:\\openresty\\nginx\\conf\\nginx.conf"));
        }
    }
    if binary_text.contains("/www/server/openresty/") {
        return Some(PathBuf::from("/www/server/openresty/nginx/conf/nginx.conf"));
    }
    if binary_text.contains("/usr/local/openresty/") {
        return Some(PathBuf::from("/usr/local/openresty/nginx/conf/nginx.conf"));
    }
    if binary_text.contains("/usr/sbin/openresty") || binary_text.contains("/usr/bin/openresty") {
        return Some(PathBuf::from("/etc/openresty/nginx.conf"));
    }
    if binary_text.contains("/nginx") {
        return Some(PathBuf::from("/etc/nginx/nginx.conf"));
    }
    None
}

fn run_binary_capture(binary: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new(binary)
        .args(args)
        .output()
        .with_context(|| format!("Failed to execute {}", binary.display()))?;
    let text = combine_output(&output);
    if output.status.success() || !text.trim().is_empty() {
        Ok(text)
    } else {
        Err(anyhow!("Exit status: {}", output.status))
    }
}

async fn load_runtime(db: &SqlitePool) -> Result<OpenRestyRuntime> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
        .fetch_all(db)
        .await
        .context("Failed to load panel settings")?;
    let settings: HashMap<String, String> = rows.into_iter().collect();

    let enabled = setting_bool(&settings, SETTING_OPENRESTY_ENABLED)
        .or_else(|| env_bool("OPENRESTY_ENABLED"))
        .unwrap_or(cfg!(any(target_os = "linux", target_os = "windows")));

    let configured_nginx_bin = setting_or_env(
        &settings,
        SETTING_OPENRESTY_BIN,
        "OPENRESTY_BIN",
        DEFAULT_OPENRESTY_BIN,
    );
    let nginx_bin = resolve_nginx_bin(&configured_nginx_bin)
        .unwrap_or_else(|| PathBuf::from(configured_nginx_bin))
        .to_string_lossy()
        .to_string();
    let sites_dir = PathBuf::from(setting_or_env(
        &settings,
        SETTING_OPENRESTY_SITES_DIR,
        "OPENRESTY_SITES_DIR",
        DEFAULT_OPENRESTY_SITES_DIR,
    ));
    let nginx_conf = setting_or_env(
        &settings,
        SETTING_OPENRESTY_CONF_PATH,
        "OPENRESTY_CONF_PATH",
        "",
    )
    .trim()
    .to_string();
    let nginx_conf = if nginx_conf.is_empty() {
        None
    } else {
        Some(PathBuf::from(nginx_conf))
    };
    // Without an explicit setting, use the installed PHP-FPM pool's own address:
    // distros listen on a unix socket, so a fixed 127.0.0.1:9000 gave every PHP site a 502.
    let detected_fpm = crate::services::php::detect()
        .map(|pool| pool.fastcgi_addr())
        .unwrap_or_else(|| "127.0.0.1:9000".to_string());
    let php_fpm_addr = setting_or_env(
        &settings,
        SETTING_PHP_FPM_ADDR,
        "PHP_FPM_ADDR",
        &detected_fpm,
    );
    // `listen [::]:80` makes nginx refuse to start on hosts with IPv6 disabled
    // (common on VPSes and in containers), so follow what the kernel supports.
    let listen_ipv6 = setting_bool(&settings, SETTING_OPENRESTY_LISTEN_IPV6)
        .or_else(|| env_bool("OPENRESTY_LISTEN_IPV6"))
        .unwrap_or_else(ipv6_available);

    Ok(OpenRestyRuntime {
        enabled,
        nginx_bin,
        sites_dir,
        nginx_conf,
        php_fpm_addr,
        listen_ipv6,
    })
}

async fn load_runtime_for_sync(db: &SqlitePool) -> Result<OpenRestyRuntime> {
    let mut runtime = load_runtime(db).await?;

    let resolved_binary = resolve_openresty_bin()
        .or_else(|| resolve_nginx_bin(&runtime.nginx_bin))
        .ok_or_else(|| anyhow!("OpenResty binary not found"))?;
    if !is_openresty_binary_path(&resolved_binary)
        && !detect_binary_version(&resolved_binary)
            .ok()
            .flatten()
            .as_deref()
            .map(version_mentions_openresty)
            .unwrap_or(false)
        && !run_binary_capture(&resolved_binary, ["-V"].as_slice())
            .map(|text| version_mentions_openresty(&text))
            .unwrap_or(false)
    {
        return Err(anyhow!(
            "Detected nginx binary is not OpenResty: {}",
            resolved_binary.display()
        ));
    }

    runtime.nginx_bin = resolved_binary.to_string_lossy().to_string();

    let conf_path = runtime
        .nginx_conf
        .clone()
        .filter(|path| path.exists())
        .or_else(|| detect_conf_path(&resolved_binary).ok().flatten())
        .or_else(|| guess_conf_path_from_bin(&resolved_binary))
        .ok_or_else(|| anyhow!("OpenResty main config was not detected"))?;
    runtime.nginx_conf = Some(conf_path.clone());

    let mut prepare_log = String::new();
    let resolved_sites_dir = prepare_sites_dir(&conf_path, &runtime.sites_dir, &mut prepare_log)?;
    runtime.sites_dir = resolved_sites_dir.clone();

    save_setting(db, SETTING_OPENRESTY_BIN, &runtime.nginx_bin).await?;
    save_setting(
        db,
        SETTING_OPENRESTY_SITES_DIR,
        &resolved_sites_dir.to_string_lossy(),
    )
    .await?;
    save_setting(
        db,
        SETTING_OPENRESTY_CONF_PATH,
        &conf_path.to_string_lossy(),
    )
    .await?;

    Ok(runtime)
}

/// Whether the kernel has IPv6 enabled (an empty if_inet6 means it is disabled).
fn ipv6_available() -> bool {
    std::fs::read_to_string("/proc/net/if_inet6")
        .map(|text| !text.trim().is_empty())
        .unwrap_or(cfg!(not(target_os = "linux")))
}

fn setting_or_env(
    settings: &HashMap<String, String>,
    setting_key: &str,
    env_key: &str,
    default_value: &str,
) -> String {
    settings
        .get(setting_key)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::trim)
        .map(ToOwned::to_owned)
        .or_else(|| {
            std::env::var(env_key)
                .ok()
                .map(|value| value.trim().to_string())
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_value.to_string())
}

fn setting_bool(settings: &HashMap<String, String>, key: &str) -> Option<bool> {
    settings.get(key).and_then(|value| parse_bool(value))
}

fn env_bool(key: &str) -> Option<bool> {
    std::env::var(key).ok().and_then(|value| parse_bool(&value))
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn quote_nginx_value(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch == '\r' || ch == '\n' {
            continue;
        }
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            other => escaped.push(other),
        }
    }
    format!("\"{}\"", escaped)
}

fn quote_nginx_single(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch == '\r' {
            continue;
        }
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '\'' => escaped.push_str("\\'"),
            '\n' => escaped.push_str("\\n"),
            other => escaped.push(other),
        }
    }
    format!("'{}'", escaped)
}

fn default_index_files_by_type(site_type: &str) -> &'static str {
    match site_type {
        "php" => "index.php index.html index.htm",
        // Proxied types never serve files directly, so the index list is unused.
        "proxy" | "node" | "python" | "java" | "go" | "binary" => "index.html index.htm",
        _ => "index.html index.htm index.php",
    }
}

/// Validates and indents user-supplied rewrite directives.
///
/// These land inside a generated `server` block, so anything that could break out of
/// it (braces, `#` comments that swallow the rest of a line, control characters) is
/// rejected rather than escaped.
fn render_rewrite_rules(raw: &str) -> Result<String> {
    let mut out = String::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.contains('{') || line.contains('}') {
            return Err(anyhow!("Rewrite rules may not contain braces"));
        }
        if line.contains('\0') || line.contains('\r') {
            return Err(anyhow!("Rewrite rules contain invalid characters"));
        }
        let directive = line.split_whitespace().next().unwrap_or("");
        const ALLOWED: &[&str] = &["rewrite", "return", "if", "set", "break"];
        if !ALLOWED.contains(&directive) {
            return Err(anyhow!(
                "Unsupported rewrite directive: {} (allowed: rewrite, return, set)",
                directive
            ));
        }
        if !line.ends_with(';') {
            return Err(anyhow!("Rewrite rule must end with ';': {}", line));
        }
        out.push_str("    ");
        out.push_str(line);
        out.push('\n');
    }
    Ok(out)
}

fn normalize_index_files_for_nginx(raw: &str, site_type: &str) -> String {
    let mut cleaned: Vec<String> = Vec::new();
    for token in raw
        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        if token.starts_with('/')
            || token.contains("..")
            || !token.chars().all(|ch| {
                ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' || ch == '/'
            })
        {
            continue;
        }
        if !cleaned.iter().any(|item| item.eq_ignore_ascii_case(token)) {
            cleaned.push(token.to_string());
        }
    }
    if cleaned.is_empty() {
        default_index_files_by_type(site_type).to_string()
    } else {
        cleaned.join(" ")
    }
}

fn default_stopped_page_html(site_name: &str, custom_html: Option<&str>) -> String {
    if let Some(custom) = custom_html.map(str::trim).filter(|value| !value.is_empty()) {
        return custom.to_string();
    }
    let escaped_name = html_escape(site_name);
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{0} - Stopped</title><style>body{{margin:0;background:#0b1020;color:#d7e0ff;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,'Helvetica Neue',Arial,'PingFang SC','Microsoft YaHei',sans-serif}}.wrap{{min-height:100vh;display:flex;align-items:center;justify-content:center;padding:24px}}.card{{max-width:520px;width:100%;border-radius:14px;border:1px solid rgba(96,130,255,.24);background:rgba(16,24,45,.84);padding:28px 26px;box-shadow:0 10px 30px rgba(0,0,0,.26)}}h1{{margin:0 0 10px;font-size:24px;line-height:1.3}}p{{margin:0;color:#9fb4e8;line-height:1.7}}</style></head><body><div class=\"wrap\"><div class=\"card\"><h1>{0}</h1><p>This site is currently stopped by LinxPanel. Start the site to resume access.</p></div></div></body></html>",
        escaped_name
    )
}

fn html_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn has_unsafe_nginx_chars(value: &str) -> bool {
    value
        .chars()
        .any(|ch| ch.is_whitespace() || ch == ';' || ch == '{' || ch == '}')
}

#[derive(Debug, Clone)]
struct DomainBinding {
    host: String,
    port: u16,
}

fn parse_domain_binding(raw: &str) -> Option<DomainBinding> {
    let mut value = raw.trim().to_string();
    if value.is_empty() {
        return None;
    }

    if let Some((_, rest)) = value.split_once("://") {
        value = rest.to_string();
    }
    if let Some((host, _)) = value.split_once('/') {
        value = host.to_string();
    }
    let value = value.trim().trim_end_matches('.');
    if value.is_empty() {
        return None;
    }

    let (host, port) = split_host_port(value)?;
    if !is_safe_server_name(host) {
        return None;
    }

    Some(DomainBinding {
        host: host.to_ascii_lowercase(),
        port: port.unwrap_or(80),
    })
}

fn split_host_port(value: &str) -> Option<(&str, Option<u16>)> {
    if let Some((host, port_raw)) = value.rsplit_once(':') {
        if !host.is_empty() && !host.contains(':') && port_raw.chars().all(|ch| ch.is_ascii_digit())
        {
            let port = port_raw.parse::<u16>().ok()?;
            if port == 0 {
                return None;
            }
            return Some((host, Some(port)));
        }
    }
    Some((value, None))
}

fn is_safe_server_name(host: &str) -> bool {
    if host.is_empty() || host.chars().any(char::is_whitespace) {
        return false;
    }
    if host == "_" || host.eq_ignore_ascii_case("localhost") {
        return true;
    }

    let wildcard_trimmed = host.strip_prefix("*.").unwrap_or(host);
    if wildcard_trimmed.parse::<Ipv4Addr>().is_ok() {
        return true;
    }

    wildcard_trimmed.split('.').all(|label| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    })
}

#[cfg(test)]
mod rewrite_tests {
    use super::render_rewrite_rules;

    #[test]
    fn accepts_common_pseudo_static_rules() {
        let rules = "rewrite ^/(.*)$ /index.php/$1 last;\nreturn 404;";
        let rendered = render_rewrite_rules(rules).expect("valid rules");
        assert!(rendered.contains("    rewrite ^/(.*)$ /index.php/$1 last;"));
        assert!(rendered.contains("    return 404;"));
    }

    #[test]
    fn skips_blanks_and_comments() {
        let rendered = render_rewrite_rules("\n# a comment\n\nset $x 1;\n").expect("valid");
        assert_eq!(rendered, "    set $x 1;\n");
    }

    #[test]
    fn rejects_braces_that_would_escape_the_server_block() {
        for rule in ["} server { listen 8080;", "rewrite ^/x$ /y last; }"] {
            assert!(
                render_rewrite_rules(rule).is_err(),
                "should reject brace injection: {rule}"
            );
        }
    }

    #[test]
    fn rejects_unknown_directives() {
        assert!(render_rewrite_rules("root /etc;").is_err());
        assert!(render_rewrite_rules("include /etc/passwd;").is_err());
    }

    #[test]
    fn requires_a_terminating_semicolon() {
        assert!(render_rewrite_rules("rewrite ^/a$ /b last").is_err());
    }
}

#[cfg(all(test, target_os = "linux"))]
mod install_summary_tests {
    #[test]
    fn the_last_real_step_is_reported() {
        let log = "[LinxPanel] Starting OpenResty install\n\
            [LinxPanel] Installing build tools: make gcc\n\
            E: Unable to locate package libpcre3-dev\n\
            [LinxPanel] Build tool 'make' could not be installed; see the package manager output above\n\
            [LinxPanel] OpenResty source fallback failed\n";
        let summary = super::summarize_linux_install_error(log);
        assert!(
            summary.contains("Build tool 'make' could not be installed"),
            "{summary}"
        );
        assert!(!summary.contains("source fallback failed"));
        let locked = "E: Could not get lock /var/lib/dpkg/lock-frontend";
        assert!(super::summarize_linux_install_error(locked).contains("apt lock"));
    }

    #[test]
    fn the_script_is_embedded_as_written() {
        assert!(super::QUICK_INSTALL_SCRIPT.starts_with("#!/usr/bin/env bash"));
        assert!(super::QUICK_INSTALL_SCRIPT.contains("drop_openresty_repo"));
    }
}

#[cfg(test)]
mod reload_tests {
    #[test]
    fn recognises_a_stopped_server() {
        assert!(super::not_running(
            "[alert] 18667#18667: kill(7872, 1) failed (3: No such process)"
        ));
        assert!(super::not_running(
            "nginx: [error] invalid PID number \"\" in \"/run/openresty.pid\""
        ));
        assert!(super::not_running("nginx: [error] open() \"/usr/local/openresty/nginx/logs/nginx.pid\" failed (2: No such file or directory)"));
        assert!(!super::not_running(
            "nginx: [emerg] unknown directive \"foo\""
        ));
    }
}

#[cfg(test)]
mod defense_tests {
    use super::*;
    use crate::services::defense::DefenseConfig;

    #[test]
    fn global_conf_skips_directives_already_set() {
        let main = "http {\n    server_tokens off;\n    # client_header_timeout 60s;\n    keepalive_timeout 65;\n}\n";
        let config = DefenseConfig::default();
        let full = render_global_conf(2, &config, main).unwrap();
        assert!(full.contains("log_format linxpanel "));
        assert!(full.contains("zone=lp_req:20m rate=30r/s"));
        assert!(
            full.contains("client_header_timeout 10s;"),
            "commented-out lines do not count"
        );
        assert!(!full.contains("server_tokens"), "already set in nginx.conf");
        assert!(!full.contains("keepalive_timeout"));
        let minimal = render_global_conf(1, &config, main).unwrap();
        assert!(!minimal.contains("client_header_timeout"));
        assert!(render_global_conf(0, &config, main).is_none());
    }

    #[test]
    fn site_directives_follow_features_and_settings() {
        let dir = std::env::temp_dir().join(format!("lp-web-{}", uuid::Uuid::new_v4()));
        let all = GlobalFeatures {
            log_format: true,
            limit_zones: true,
        };
        let cc_on = DefenseConfig {
            cc_enabled: true,
            ..DefenseConfig::default()
        };
        let text = SiteDefense::new(all, &cc_on, &dir).server_directives(7);
        assert!(text.contains("site-7.log\" linxpanel;") || text.contains("site-7.log linxpanel;"));
        assert!(text.contains("limit_req zone=lp_req burst=200 nodelay;"));
        assert!(text.contains("limit_conn lp_conn 64;"));
        let cc_off = SiteDefense::new(all, &DefenseConfig::default(), &dir).server_directives(7);
        assert!(!cc_off.contains("limit_req"));
        // Without the global file nothing may reference its format or zones.
        assert!(SiteDefense::new(GlobalFeatures::default(), &cc_on, &dir)
            .server_directives(7)
            .is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
