use anyhow::{anyhow, Context, Result};
use sqlx::SqlitePool;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::models::{Domain, Site};
use crate::AppState;

const SETTING_OPENRESTY_ENABLED: &str = "openresty_enabled";
const SETTING_OPENRESTY_BIN: &str = "openresty_bin";
const SETTING_OPENRESTY_SITES_DIR: &str = "openresty_sites_dir";
const SETTING_OPENRESTY_CONF_PATH: &str = "openresty_conf_path";
const SETTING_PHP_FPM_ADDR: &str = "php_fpm_addr";
const SETTING_OPENRESTY_LISTEN_IPV6: &str = "openresty_listen_ipv6";

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
        self.sites_dir.join(format!("linxpanel-site-{}.conf", site_id))
    }
}

pub async fn sync_site(state: &AppState, site_id: i64) -> Result<()> {
    let _guard = state.openresty_lock.lock().await;
    let runtime = load_runtime(&state.db).await?;
    if !runtime.enabled {
        return Ok(());
    }
    if !cfg!(target_os = "linux") {
        return Err(anyhow!("OpenResty sync is supported on Linux only"));
    }

    let site = sqlx::query_as::<_, Site>("SELECT * FROM sites WHERE id = ?")
        .bind(site_id)
        .fetch_optional(&state.db)
        .await
        .context("Failed to load site data")?;

    let desired_config = match site {
        Some(site) if site.status == "running" => Some(render_site_config(&state.db, &site, &runtime).await?),
        _ => None,
    };

    apply_site_config(&runtime, site_id, desired_config)?;
    Ok(())
}

pub async fn remove_site(state: &AppState, site_id: i64) -> Result<()> {
    let _guard = state.openresty_lock.lock().await;
    let runtime = load_runtime(&state.db).await?;
    if !runtime.enabled {
        return Ok(());
    }
    if !cfg!(target_os = "linux") {
        return Err(anyhow!("OpenResty sync is supported on Linux only"));
    }

    apply_site_config(&runtime, site_id, None)?;
    Ok(())
}

fn apply_site_config(runtime: &OpenRestyRuntime, site_id: i64, desired_config: Option<String>) -> Result<()> {
    fs::create_dir_all(&runtime.sites_dir)
        .with_context(|| format!("Failed to create OpenResty sites dir: {}", runtime.sites_dir.display()))?;

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
                fs::remove_file(path)
                    .with_context(|| format!("Failed to remove invalid config: {}", path.display()))?;
            }
        }
    }
    Ok(())
}

async fn render_site_config(db: &SqlitePool, site: &Site, runtime: &OpenRestyRuntime) -> Result<String> {
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

    let mut blocks = String::new();
    blocks.push_str("# Generated by LinxPanel\n");
    blocks.push_str(&format!("# site_id: {}\n", site.id));

    for (port, hosts) in grouped {
        blocks.push('\n');
        blocks.push_str("server {\n");

        if site.ssl_enabled && site.ssl_cert_path.is_some() && site.ssl_key_path.is_some() {
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
        } else {
            blocks.push_str(&format!("    listen {};\n", port));
            if runtime.listen_ipv6 {
                blocks.push_str(&format!("    listen [::]:{};\n", port));
            }
        }

        blocks.push_str(&format!("    server_name {};\n", hosts.join(" ")));

        match site.site_type.as_str() {
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
                blocks.push_str("        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;\n");
                blocks.push_str("        proxy_set_header X-Forwarded-Proto $scheme;\n");
                blocks.push_str("    }\n");
            }
            "php" => {
                blocks.push_str(&format!("    root {};\n", quote_nginx_value(&site.doc_root)));
                blocks.push_str("    index index.php index.html index.htm;\n");
                blocks.push_str("    location / {\n");
                blocks.push_str("        try_files $uri $uri/ /index.php?$query_string;\n");
                blocks.push_str("    }\n");
                blocks.push_str("    location ~ \\.php$ {\n");
                blocks.push_str("        include fastcgi_params;\n");
                blocks.push_str("        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;\n");
                blocks.push_str("        fastcgi_index index.php;\n");
                blocks.push_str(&format!("        fastcgi_pass {};\n", runtime.php_fpm_addr));
                blocks.push_str("    }\n");
            }
            _ => {
                blocks.push_str(&format!("    root {};\n", quote_nginx_value(&site.doc_root)));
                blocks.push_str("    index index.html index.htm index.php;\n");
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
    let rows = sqlx::query_as::<_, Domain>("SELECT * FROM domains WHERE site_id = ? ORDER BY sort_order ASC, id ASC")
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

fn reload_openresty(runtime: &OpenRestyRuntime) -> Result<()> {
    run_openresty(runtime, ["-s", "reload"].as_slice())
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

async fn load_runtime(db: &SqlitePool) -> Result<OpenRestyRuntime> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
        .fetch_all(db)
        .await
        .context("Failed to load panel settings")?;
    let settings: HashMap<String, String> = rows.into_iter().collect();

    let enabled = setting_bool(&settings, SETTING_OPENRESTY_ENABLED)
        .or_else(|| env_bool("OPENRESTY_ENABLED"))
        .unwrap_or(cfg!(target_os = "linux"));

    let nginx_bin = setting_or_env(
        &settings,
        SETTING_OPENRESTY_BIN,
        "OPENRESTY_BIN",
        "/usr/local/openresty/nginx/sbin/nginx",
    );
    let sites_dir = PathBuf::from(setting_or_env(
        &settings,
        SETTING_OPENRESTY_SITES_DIR,
        "OPENRESTY_SITES_DIR",
        "/usr/local/openresty/nginx/conf/sites-enabled",
    ));
    let nginx_conf = setting_or_env(&settings, SETTING_OPENRESTY_CONF_PATH, "OPENRESTY_CONF_PATH", "")
        .trim()
        .to_string();
    let nginx_conf = if nginx_conf.is_empty() {
        None
    } else {
        Some(PathBuf::from(nginx_conf))
    };
    let php_fpm_addr = setting_or_env(
        &settings,
        SETTING_PHP_FPM_ADDR,
        "PHP_FPM_ADDR",
        "127.0.0.1:9000",
    );
    let listen_ipv6 = setting_bool(&settings, SETTING_OPENRESTY_LISTEN_IPV6)
        .or_else(|| env_bool("OPENRESTY_LISTEN_IPV6"))
        .unwrap_or(true);

    Ok(OpenRestyRuntime {
        enabled,
        nginx_bin,
        sites_dir,
        nginx_conf,
        php_fpm_addr,
        listen_ipv6,
    })
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
        .or_else(|| std::env::var(env_key).ok().map(|value| value.trim().to_string()))
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
        if !host.is_empty() && !host.contains(':') && port_raw.chars().all(|ch| ch.is_ascii_digit()) {
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
