//! Several PHP versions side by side (Debian / Ubuntu): install and remove
//! versions, the default version, extensions, php.ini settings and PHP-FPM
//! pool tuning per version. Each website picks its version; the FPM pools are
//! found by `php::pools`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{php, shell, sysservice};

pub const SETTING_DEFAULT: &str = "php_default_version";
/// What can be installed, newest first.
pub const VERSIONS: &[&str] = &[
    "8.4", "8.3", "8.2", "8.1", "8.0", "7.4", "7.3", "7.2", "7.1", "7.0", "5.6",
];
const INSTALL_SCRIPT: &str = include_str!("../../scripts/php-install.sh");
const DROP_IN: &str = "99-linxpanel.ini";

pub fn valid_version(version: &str) -> bool {
    VERSIONS.contains(&version) || version == "8.5"
}

/// Side-by-side versions need apt (Ondřej Surý's packages).
pub fn multi_supported() -> bool {
    shell::has_binary("apt-get") && shell::has_binary("dpkg-query")
}

#[derive(Debug, Clone, Serialize)]
pub struct Installed {
    pub version: String,
    /// Full version, e.g. 8.1.34.
    pub patch: String,
    pub service: String,
    pub running: bool,
    pub listen: String,
    pub user: String,
    pub ini: String,
    pub pool_conf: String,
}

pub fn installed() -> Vec<Installed> {
    php::pools()
        .into_iter()
        .map(|pool| {
            let binary = format!("php{}", pool.version);
            let patch = shell::cached_version_output(&binary, &["-r", "echo PHP_VERSION;"])
                .map(|v| v.trim().to_string())
                .unwrap_or_default();
            Installed {
                running: sysservice::is_active(&pool.service),
                ini: ini_path(&pool.version).to_string_lossy().to_string(),
                pool_conf: pool.conf.to_string_lossy().to_string(),
                version: pool.version,
                patch,
                service: pool.service,
                listen: pool.listen,
                user: pool.user,
            }
        })
        .collect()
}

fn etc_dir(version: &str) -> PathBuf {
    Path::new("/etc/php").join(version)
}

pub fn ini_path(version: &str) -> PathBuf {
    etc_dir(version).join("fpm/php.ini")
}

/// Runs the install script; its output streams into the task log.
pub fn run_script(action: &str, version: &str) -> Result<String, String> {
    if !valid_version(version) {
        return Err(format!("Unsupported PHP version: {}", version));
    }
    shell::run_embedded_script(
        "php",
        INSTALL_SCRIPT,
        &[("PHP_ACTION", action), ("PHP_VERSION", version)],
        Duration::from_secs(45 * 60),
        "PHP script failed",
    )
}

/// Makes `version` the command-line `php` too (Composer, cron jobs, apps).
pub fn set_cli_default(version: &str) -> Result<(), String> {
    let binary = format!("/usr/bin/php{}", version);
    if !Path::new(&binary).exists() || !shell::has_binary("update-alternatives") {
        return Ok(());
    }
    let output = shell::run_privileged("update-alternatives", &["--set", "php", &binary])?;
    if output.ok() {
        Ok(())
    } else {
        Err(output.error_text())
    }
}

pub fn reload(version: &str) -> Result<(), String> {
    let pool = php::pool_for(version).ok_or("This PHP version is not installed")?;
    sysservice::enable_and_restart(&pool.service).map(|_| ())
}

// ===== Extensions =====

#[derive(Debug, Clone, Serialize)]
pub struct Extension {
    pub name: String,
    pub package: String,
    pub installed: bool,
    /// Compiled in or enabled (php -m).
    pub loaded: bool,
}

/// Packages that are PHP itself, not extensions.
fn is_core_package(name: &str) -> bool {
    matches!(
        name,
        "fpm" | "cli" | "common" | "cgi" | "dev" | "phpdbg" | "dbg" | "readline" | "json" | "embed"
    ) || name.ends_with("-dbgsym")
}

/// Extension packages this version can install, plus what is loaded.
pub fn extensions(version: &str) -> Result<Vec<Extension>, String> {
    if !valid_version(version) {
        return Err("Unsupported PHP version".into());
    }
    let prefix = format!("php{}-", version);
    let available = shell::run("apt-cache", &["pkgnames", &prefix])?;
    let installed_out = shell::run(
        "dpkg-query",
        &["-W", "-f=${Package} ${Status}\\n", &format!("{}*", prefix)],
    )?;
    let installed: std::collections::HashSet<String> = installed_out
        .stdout
        .lines()
        .filter(|l| l.ends_with(" installed"))
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_string)
        .collect();
    let loaded_out = shell::run(&format!("php{}", version), &["-m"]).ok();
    let loaded: std::collections::HashSet<String> = loaded_out
        .map(|o| o.stdout)
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_ascii_lowercase().replace(' ', "_"))
        .filter(|l| !l.is_empty() && !l.starts_with('['))
        .collect();
    let mut list: Vec<Extension> = available
        .stdout
        .lines()
        .map(str::trim)
        .filter_map(|pkg| pkg.strip_prefix(&prefix).map(|n| (pkg, n)))
        .filter(|(_, name)| !is_core_package(name))
        .map(|(pkg, name)| {
            let module = match name {
                "mysql" => "mysqli",
                "sqlite3" => "sqlite3",
                "opcache" => "zend_opcache",
                other => other,
            };
            Extension {
                name: name.to_string(),
                package: pkg.to_string(),
                installed: installed.contains(pkg),
                loaded: loaded.contains(module),
            }
        })
        .collect();
    list.sort_by(|a, b| b.installed.cmp(&a.installed).then(a.name.cmp(&b.name)));
    list.dedup_by(|a, b| a.name == b.name);
    Ok(list)
}

pub fn set_extension(version: &str, name: &str, install: bool) -> Result<String, String> {
    if !valid_version(version)
        || name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err("Invalid extension".into());
    }
    let package = format!("php{}-{}", version, name);
    let args: Vec<&str> = if install {
        vec!["install", "-y", "--no-install-recommends", &package]
    } else {
        vec!["remove", "-y", &package]
    };
    let output = super::runtime::run_package_manager("apt-get", &args, Duration::from_secs(900))?;
    let text = format!("{}{}", output.stdout, output.stderr);
    if !output.ok() {
        return Err(text
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("apt-get failed")
            .to_string());
    }
    let _ = reload(version);
    Ok(text)
}

// ===== php.ini settings and FPM pool =====

/// php.ini keys the panel manages (in a drop-in that wins over php.ini).
pub const INI_KEYS: &[&str] = &[
    "upload_max_filesize",
    "post_max_size",
    "memory_limit",
    "max_execution_time",
    "max_input_time",
    "max_input_vars",
    "display_errors",
    "date.timezone",
    "short_open_tag",
    "disable_functions",
];
pub const POOL_KEYS: &[&str] = &[
    "pm",
    "pm.max_children",
    "pm.start_servers",
    "pm.min_spare_servers",
    "pm.max_spare_servers",
    "pm.max_requests",
];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    pub ini: BTreeMap<String, String>,
    pub pool: BTreeMap<String, String>,
}

fn strip_ini_value(value: &str) -> String {
    let v = value.trim();
    // A trailing ; comment (outside quotes) is not part of the value.
    let v = if v.starts_with('"') {
        v
    } else {
        v.split(';').next().unwrap_or(v).trim()
    };
    v.trim_matches('"').to_string()
}

/// `key = value` pairs of an ini text (later ones win).
fn parse_ini(text: &str, into: &mut BTreeMap<String, String>, keys: &[&str]) {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with(';')
            || line.starts_with('#')
            || line.starts_with('[')
        {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim();
            if keys.contains(&k) {
                into.insert(k.to_string(), strip_ini_value(v));
            }
        }
    }
}

/// Effective values: php.ini, then conf.d/*.ini in order (as PHP reads them).
pub fn settings(version: &str) -> Result<Settings, String> {
    let pool = php::pool_for(version).ok_or("This PHP version is not installed")?;
    let mut ini = BTreeMap::new();
    let base = etc_dir(version).join("fpm");
    parse_ini(
        &std::fs::read_to_string(base.join("php.ini")).unwrap_or_default(),
        &mut ini,
        INI_KEYS,
    );
    let mut extra: Vec<PathBuf> = std::fs::read_dir(base.join("conf.d"))
        .map(|d| {
            d.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "ini"))
                .collect()
        })
        .unwrap_or_default();
    extra.sort();
    for file in extra {
        parse_ini(
            &std::fs::read_to_string(file).unwrap_or_default(),
            &mut ini,
            INI_KEYS,
        );
    }
    let mut pool_values = BTreeMap::new();
    parse_ini(
        &std::fs::read_to_string(&pool.conf).unwrap_or_default(),
        &mut pool_values,
        POOL_KEYS,
    );
    Ok(Settings {
        ini,
        pool: pool_values,
    })
}

fn validate(settings: &Settings) -> Result<(), String> {
    for (k, v) in &settings.ini {
        if !INI_KEYS.contains(&k.as_str()) {
            return Err(format!("Unknown setting: {}", k));
        }
        if v.contains(['\n', '\r', '"']) || v.len() > 2000 {
            return Err(format!("Invalid value for {}", k));
        }
        let size_like = |v: &str| {
            let v = v.trim();
            v == "-1"
                || (!v.is_empty()
                    && v.trim_end_matches(['K', 'M', 'G', 'k', 'm', 'g'])
                        .chars()
                        .all(|c| c.is_ascii_digit()))
        };
        match k.as_str() {
            "upload_max_filesize" | "post_max_size" | "memory_limit" if !size_like(v) => {
                return Err(format!("{} must look like 64M, 1G or -1", k))
            }
            "max_execution_time" | "max_input_time" | "max_input_vars"
                if v.trim().parse::<i64>().is_err() =>
            {
                return Err(format!("{} must be a number", k))
            }
            "display_errors" | "short_open_tag" if !matches!(v.as_str(), "On" | "Off") => {
                return Err(format!("{} must be On or Off", k))
            }
            "date.timezone"
                if !v.is_empty()
                    && !v
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "/_-+".contains(c)) =>
            {
                return Err("Invalid time zone".into())
            }
            "disable_functions"
                if !v
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == ',') =>
            {
                return Err("disable_functions is a comma-separated list of function names".into())
            }
            _ => {}
        }
    }
    let num = |key: &str| {
        settings
            .pool
            .get(key)
            .and_then(|v| v.trim().parse::<u32>().ok())
    };
    for (k, v) in &settings.pool {
        if !POOL_KEYS.contains(&k.as_str()) {
            return Err(format!("Unknown pool setting: {}", k));
        }
        if k == "pm" {
            if !matches!(v.as_str(), "dynamic" | "static" | "ondemand") {
                return Err("pm must be dynamic, static or ondemand".into());
            }
        } else if v.trim().parse::<u32>().is_err() {
            return Err(format!("{} must be a whole number", k));
        }
    }
    if let Some(children) = num("pm.max_children") {
        if children == 0 || children > 5000 {
            return Err("pm.max_children must be 1-5000".into());
        }
        if settings.pool.get("pm").map(String::as_str) == Some("dynamic") {
            let (start, min, max) = (
                num("pm.start_servers").unwrap_or(1),
                num("pm.min_spare_servers").unwrap_or(1),
                num("pm.max_spare_servers").unwrap_or(1),
            );
            if !(min <= start && start <= max && max <= children) {
                return Err("Need min_spare ≤ start_servers ≤ max_spare ≤ max_children".into());
            }
        }
    }
    Ok(())
}

/// Replaces (or appends) `key = value` lines in a pool file.
fn set_pool_values(text: &str, values: &BTreeMap<String, String>) -> String {
    let mut done = std::collections::HashSet::new();
    let mut out: Vec<String> = text
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if let Some((k, _)) = trimmed.split_once('=') {
                let k = k.trim();
                if let Some(v) = values.get(k) {
                    done.insert(k.to_string());
                    return format!("{} = {}", k, v);
                }
            }
            line.to_string()
        })
        .collect();
    for (k, v) in values {
        if !done.contains(k) {
            out.push(format!("{} = {}", k, v));
        }
    }
    out.join("\n") + "\n"
}

/// Writes the drop-in (FPM and CLI) and the pool values, tests the FPM
/// configuration and restarts it; on a failed test everything is put back.
pub fn save_settings(version: &str, settings: &Settings) -> Result<(), String> {
    validate(settings)?;
    let pool = php::pool_for(version).ok_or("This PHP version is not installed")?;
    let mut drop_in = String::from("; Written by LinxPanel (PHP settings). Edit in the panel.\n");
    for (k, v) in &settings.ini {
        if v.trim().is_empty() && k != "disable_functions" && k != "date.timezone" {
            continue;
        }
        let quoted = if k == "date.timezone" || k == "disable_functions" {
            format!("\"{}\"", v.trim())
        } else {
            v.trim().to_string()
        };
        drop_in.push_str(&format!("{} = {}\n", k, quoted));
    }
    let targets: Vec<PathBuf> = ["fpm", "cli"]
        .iter()
        .map(|sapi| etc_dir(version).join(sapi).join("conf.d"))
        .filter(|d| d.is_dir())
        .map(|d| d.join(DROP_IN))
        .collect();
    let backups: Vec<(PathBuf, Option<String>)> = targets
        .iter()
        .map(|p| (p.clone(), std::fs::read_to_string(p).ok()))
        .chain(std::iter::once((
            pool.conf.clone(),
            std::fs::read_to_string(&pool.conf).ok(),
        )))
        .collect();
    let restore = || {
        for (path, old) in &backups {
            match old {
                Some(text) => {
                    let _ = std::fs::write(path, text);
                }
                None => {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    };
    for path in &targets {
        if let Err(e) = std::fs::write(path, &drop_in) {
            restore();
            return Err(format!("{}: {}", path.display(), e));
        }
    }
    if !settings.pool.is_empty() {
        let text = std::fs::read_to_string(&pool.conf).map_err(|e| e.to_string())?;
        if let Err(e) = std::fs::write(&pool.conf, set_pool_values(&text, &settings.pool)) {
            restore();
            return Err(e.to_string());
        }
    }
    // php-fpmX.Y -t checks the configuration without touching the running pool.
    let fpm = format!("php-fpm{}", version);
    if shell::has_binary(&fpm) {
        let test = shell::run(&fpm, &["-t"])?;
        if !test.ok() {
            restore();
            return Err(format!(
                "PHP-FPM rejected the settings: {}",
                test.error_text()
            ));
        }
    }
    if let Err(err) = reload(version) {
        restore();
        let _ = reload(version);
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_parsing() {
        let mut map = BTreeMap::new();
        parse_ini(
            "[PHP]\n;memory_limit = 1G\nmemory_limit = 128M ; comment\nupload_max_filesize=2M\ndate.timezone = \"Asia/Shanghai\"\n",
            &mut map,
            INI_KEYS,
        );
        assert_eq!(map.get("memory_limit").unwrap(), "128M");
        assert_eq!(map.get("upload_max_filesize").unwrap(), "2M");
        assert_eq!(map.get("date.timezone").unwrap(), "Asia/Shanghai");
    }

    #[test]
    fn pool_values_are_replaced_or_added() {
        let mut values = BTreeMap::new();
        values.insert("pm".to_string(), "static".to_string());
        values.insert("pm.max_requests".to_string(), "500".to_string());
        let out = set_pool_values("[www]\npm = dynamic\n;pm.max_requests = 500\n", &values);
        assert!(out.contains("\npm = static\n"));
        assert!(out.contains(";pm.max_requests = 500") && out.ends_with("pm.max_requests = 500\n"));
    }

    #[test]
    fn validation() {
        let mut s = Settings::default();
        s.ini.insert("memory_limit".into(), "256M".into());
        s.ini.insert("display_errors".into(), "Off".into());
        s.pool.insert("pm".into(), "dynamic".into());
        s.pool.insert("pm.max_children".into(), "20".into());
        s.pool.insert("pm.start_servers".into(), "4".into());
        s.pool.insert("pm.min_spare_servers".into(), "2".into());
        s.pool.insert("pm.max_spare_servers".into(), "6".into());
        assert!(validate(&s).is_ok());
        s.pool.insert("pm.start_servers".into(), "9".into());
        assert!(validate(&s).is_err());
        let mut bad = Settings::default();
        bad.ini.insert("memory_limit".into(), "lots".into());
        assert!(validate(&bad).is_err());
        let mut inj = Settings::default();
        inj.ini
            .insert("disable_functions".into(), "exec\nextension=evil".into());
        assert!(validate(&inj).is_err());
        assert!(valid_version("7.4") && !valid_version("9.9") && !valid_version("../x"));
    }
}
