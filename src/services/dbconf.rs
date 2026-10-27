//! Database server settings, the way a panel operator works with them:
//! remote access, the superuser password, the main tuning parameters, logs
//! (and MySQL's binary log, which can fill a disk), the raw configuration
//! files, start / stop and uninstall - for MySQL / MariaDB, PostgreSQL and
//! Redis alike.
//!
//! Every change that needs a restart is applied the same careful way: the
//! previous file is kept, the server restarted, and when it does not come
//! back the old file is put back and the server restarted again, so a bad
//! value never leaves the database down.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::database::{self, mysql_query, psql, Engine, MysqlAdmin};
use super::{firewall, runtime, shell, sysservice};

/// Stored superuser passwords (never sent through the generic settings API).
pub const SETTING_MYSQL_ROOT_PASSWORD: &str = "mysql_root_password";
pub const SETTING_POSTGRES_PASSWORD: &str = "postgres_password";

// ===== What the settings page shows =====

#[derive(Debug, Clone, Serialize)]
pub struct Param {
    pub key: String,
    /// Current value as the server reports it (sizes in bytes).
    pub value: String,
    /// int | size | seconds | bool | enum
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogInfo {
    /// general | slow | binlog | error | statement | server
    pub kind: &'static str,
    pub enabled: bool,
    /// Whether the panel can switch it on and off.
    pub switchable: bool,
    pub path: Option<String>,
    pub size: Option<u64>,
    /// slow: seconds (MySQL) / milliseconds (PostgreSQL); binlog: days kept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteState {
    pub enabled: bool,
    /// Redis refuses remote access without a password.
    pub needs_password: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineSettings {
    pub engine: Engine,
    /// mariadb | mysql | postgresql | redis
    pub flavor: String,
    pub version: String,
    pub service: String,
    pub running: bool,
    pub port: u16,
    pub data_dir: Option<String>,
    pub data_size: Option<u64>,
    pub remote: RemoteState,
    /// MySQL root / PostgreSQL postgres / Redis requirepass, when known.
    pub superuser: &'static str,
    pub superuser_password: Option<String>,
    pub params: Vec<Param>,
    pub logs: Vec<LogInfo>,
}

// ===== Helpers =====

fn file_size(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.len())
}

/// Total size of a directory (best effort, bounded in time).
fn dir_size(path: &str) -> Option<u64> {
    let output =
        shell::run_privileged_with_timeout("du", &["-sb", path], Duration::from_secs(15)).ok()?;
    output.stdout.split_whitespace().next()?.parse().ok()
}

fn write_config(path: &Path, content: &str) -> Result<(), String> {
    std::fs::write(path, content).map_err(|e| format!("Cannot write {}: {}", path.display(), e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Created files must be readable by the database's own account.
        let mode = std::fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o777)
            .unwrap_or(0o644);
        if mode & 0o044 == 0 && !path.starts_with("/etc/redis") {
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644));
        }
    }
    Ok(())
}

fn restore(path: &Path, previous: Option<&str>) {
    match previous {
        Some(text) => {
            let _ = std::fs::write(path, text);
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Last lines of the service's journal, to explain a failed restart.
fn journal_tail(service: &str) -> String {
    shell::run_best_effort(
        "journalctl",
        &["-u", service, "-n", "15", "--no-pager", "-q"],
    )
    .map(|o| o.stdout.trim().to_string())
    .unwrap_or_default()
}

/// Restarts the engine; when it does not come back, `rollback` undoes the
/// change and the engine is started again with what worked before.
fn restart_or_rollback(engine: Engine, rollback: impl FnOnce()) -> Result<String, String> {
    let service = database::service_name(engine);
    match sysservice::enable_and_restart(&service) {
        Ok(log) => {
            if wait_running(&service) {
                return Ok(log);
            }
            rollback();
            let _ = sysservice::enable_and_restart(&service);
            Err(format!(
                "{} did not stay up with the new settings; the previous configuration was restored.\n{}",
                engine.display_name(),
                journal_tail(&service)
            ))
        }
        Err(err) => {
            rollback();
            let _ = sysservice::enable_and_restart(&service);
            Err(format!(
                "{} failed to restart with the new settings; the previous configuration was restored.\n{}\n{}",
                engine.display_name(),
                err.trim(),
                journal_tail(&service)
            ))
        }
    }
}

fn wait_running(service: &str) -> bool {
    for _ in 0..10 {
        if sysservice::is_active(service) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}

fn open_firewall(port: u16, allow_from: Option<&str>, log: &mut String) {
    let fw = firewall::status();
    if fw.enabled && fw.manageable {
        let request = firewall::FirewallRuleRequest {
            port: port.to_string(),
            protocol: "tcp".into(),
            source: allow_from.unwrap_or_default().trim().to_string(),
            action: "allow".into(),
        };
        match firewall::add_rule(&request) {
            Ok(message) => log.push_str(&format!("Firewall: {}\n", message)),
            Err(err) => log.push_str(&format!("Firewall not changed: {}\n", err)),
        }
    }
}

/// "256M" / "1G" / "1048576" -> bytes.
pub fn parse_size(value: &str) -> Option<u64> {
    let value = value.trim();
    let (number, unit) = match value.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((at, _)) => value.split_at(at),
        None => (value, ""),
    };
    let number: u64 = number.parse().ok()?;
    let factor: u64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" => 1 << 10,
        "m" | "mb" => 1 << 20,
        "g" | "gb" => 1 << 30,
        _ => return None,
    };
    number.checked_mul(factor)
}

/// Validates a parameter value against its kind before it reaches a file.
fn check_value(kind: &str, options: &[&str], value: &str) -> Result<String, String> {
    let value = value.trim();
    let ok = match kind {
        "int" | "seconds" => value.parse::<u64>().is_ok(),
        "size" => parse_size(value).is_some(),
        "bool" => matches!(value, "on" | "off"),
        "enum" => options.contains(&value),
        "decimal" => value
            .parse::<f64>()
            .is_ok_and(|v| (0.0..=3600.0).contains(&v)),
        _ => false,
    };
    if ok {
        Ok(value.to_string())
    } else {
        Err(format!("Invalid value: {}", value))
    }
}

// ===== MySQL / MariaDB =====

/// The panel's own option file, sorted after the distro's files so it wins.
const MYSQL_DROP_IN: &str = "zz-linxpanel.cnf";

pub(crate) fn mysql_conf_dir() -> Option<PathBuf> {
    [
        "/etc/mysql/mariadb.conf.d",
        "/etc/mysql/mysql.conf.d",
        "/etc/my.cnf.d",
        "/etc/mysql/conf.d",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|dir| dir.is_dir())
}

pub(crate) fn mysql_drop_in() -> Option<PathBuf> {
    mysql_conf_dir().map(|dir| dir.join(MYSQL_DROP_IN))
}

/// The distro's main server option file.
fn mysql_main_conf() -> Option<PathBuf> {
    [
        "/etc/mysql/mariadb.conf.d/50-server.cnf",
        "/etc/mysql/mysql.conf.d/mysqld.cnf",
        "/etc/my.cnf.d/mariadb-server.cnf",
        "/etc/my.cnf.d/mysql-server.cnf",
        "/etc/my.cnf",
        "/etc/mysql/my.cnf",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
}

/// The drop-in's `[mysqld]` options, in order (key -> value; "" for flags).
fn read_drop_in() -> BTreeMap<String, String> {
    let text = mysql_drop_in()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    parse_drop_in(&text)
}

fn parse_drop_in(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('['))
        .map(|line| match line.split_once('=') {
            Some((key, value)) => (key.trim().replace('-', "_"), value.trim().to_string()),
            None => (line.replace('-', "_"), String::new()),
        })
        .collect()
}

fn render_drop_in(options: &BTreeMap<String, String>) -> String {
    let mut out = String::from(
        "# Written by LinxPanel (Databases > Settings). Changes made there replace this file.\n[mysqld]\n",
    );
    for (key, value) in options {
        if value.is_empty() {
            out.push_str(&format!("{}\n", key));
        } else {
            out.push_str(&format!("{} = {}\n", key, value));
        }
    }
    out
}

/// Writes the drop-in with `change` applied and restarts, rolling back on failure.
fn update_drop_in(change: impl FnOnce(&mut BTreeMap<String, String>)) -> Result<String, String> {
    let path = mysql_drop_in().ok_or("MySQL / MariaDB configuration directory not found")?;
    let previous = std::fs::read_to_string(&path).ok();
    let mut options = parse_drop_in(previous.as_deref().unwrap_or_default());
    change(&mut options);
    let content = render_drop_in(&options);
    if previous.as_deref() == Some(content.as_str()) {
        return Ok(String::new());
    }
    write_config(&path, &content)?;
    let restore_path = path.clone();
    restart_or_rollback(Engine::Mysql, move || {
        restore(&restore_path, previous.as_deref())
    })
}

fn mysql_vars(admin: &MysqlAdmin) -> BTreeMap<String, String> {
    mysql_query(admin, "SHOW GLOBAL VARIABLES;")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('\t')?;
            Some((key.to_string(), value.to_string()))
        })
        .collect()
}

fn is_mariadb(vars: &BTreeMap<String, String>) -> bool {
    vars.get("version")
        .is_some_and(|v| v.to_ascii_lowercase().contains("mariadb"))
}

const MYSQL_PARAMS: &[(&str, &str)] = &[
    ("max_connections", "int"),
    ("innodb_buffer_pool_size", "size"),
    ("max_allowed_packet", "size"),
    ("wait_timeout", "seconds"),
    ("table_open_cache", "int"),
    ("tmp_table_size", "size"),
];

fn mysql_log_path(vars: &BTreeMap<String, String>, key: &str) -> Option<PathBuf> {
    let value = vars.get(key)?.trim();
    if value.is_empty() || value == "stderr" {
        return None;
    }
    let path = PathBuf::from(value);
    Some(if path.is_absolute() {
        path
    } else {
        PathBuf::from(vars.get("datadir").cloned().unwrap_or_default()).join(path)
    })
}

fn binlog_total(admin: &MysqlAdmin) -> u64 {
    mysql_query(admin, "SHOW BINARY LOGS;")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split('\t').nth(1)?.parse::<u64>().ok())
        .sum()
}

fn on(value: Option<&String>) -> bool {
    value.is_some_and(|v| matches!(v.to_ascii_uppercase().as_str(), "ON" | "1" | "YES"))
}

fn mysql_settings(admin: Option<&MysqlAdmin>, mut base: EngineSettings) -> EngineSettings {
    let Some(admin) = admin else {
        return base;
    };
    let vars = mysql_vars(admin);
    if vars.is_empty() {
        return base;
    }
    let mariadb = is_mariadb(&vars);
    base.flavor = if mariadb { "mariadb" } else { "mysql" }.into();
    base.version = vars.get("version").cloned().unwrap_or_default();
    if let Some(port) = vars.get("port").and_then(|p| p.parse().ok()) {
        base.port = port;
    }
    let datadir = vars.get("datadir").cloned();
    base.data_size = datadir.as_deref().and_then(dir_size);
    base.data_dir = datadir;
    base.params = MYSQL_PARAMS
        .iter()
        .map(|(key, kind)| Param {
            key: key.to_string(),
            value: vars.get(*key).cloned().unwrap_or_default(),
            kind,
            options: Vec::new(),
        })
        .collect();
    let binlog_on = on(vars.get("log_bin"));
    let expire_days = if mariadb {
        vars.get("expire_logs_days").cloned()
    } else {
        vars.get("binlog_expire_logs_seconds")
            .and_then(|s| s.parse::<u64>().ok())
            .map(|s| (s / 86_400).to_string())
    };
    let general = mysql_log_path(&vars, "general_log_file");
    let slow = mysql_log_path(&vars, "slow_query_log_file");
    let error = mysql_log_path(&vars, "log_error");
    base.logs = vec![
        LogInfo {
            kind: "binlog",
            enabled: binlog_on,
            switchable: true,
            path: None,
            size: Some(if binlog_on { binlog_total(admin) } else { 0 }),
            threshold: expire_days,
        },
        LogInfo {
            kind: "slow",
            enabled: on(vars.get("slow_query_log")),
            switchable: true,
            size: slow.as_deref().and_then(file_size),
            path: slow.map(|p| p.display().to_string()),
            threshold: vars.get("long_query_time").cloned(),
        },
        LogInfo {
            kind: "general",
            enabled: on(vars.get("general_log")),
            switchable: true,
            size: general.as_deref().and_then(file_size),
            path: general.map(|p| p.display().to_string()),
            threshold: None,
        },
        LogInfo {
            kind: "error",
            enabled: true,
            switchable: false,
            size: error.as_deref().and_then(file_size),
            path: error.map(|p| p.display().to_string()),
            threshold: None,
        },
    ];
    base.remote.enabled = read_drop_in()
        .get("bind_address")
        .is_some_and(|v| v != "127.0.0.1" && v != "localhost");
    base
}

/// Applies MySQL parameter and log changes through the drop-in.
fn mysql_apply(
    admin: Option<&MysqlAdmin>,
    params: &BTreeMap<String, String>,
    logs: &BTreeMap<String, LogChange>,
) -> Result<String, String> {
    let admin = admin.ok_or("MySQL is not set up for the panel")?;
    let vars = mysql_vars(admin);
    let mariadb = is_mariadb(&vars);
    let mut checked = BTreeMap::new();
    for (key, value) in params {
        let kind = MYSQL_PARAMS
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, kind)| *kind)
            .ok_or_else(|| format!("Unknown parameter: {}", key))?;
        checked.insert(key.clone(), check_value(kind, &[], value)?);
    }
    let mut threshold = BTreeMap::new();
    for (kind, change) in logs {
        if let Some(value) = &change.threshold {
            let check = if kind == "slow" { "decimal" } else { "int" };
            threshold.insert(kind.clone(), check_value(check, &[], value)?);
        }
    }
    update_drop_in(|options| {
        for (key, value) in checked {
            options.insert(key, value);
        }
        for (kind, change) in logs {
            match kind.as_str() {
                "general" => {
                    if let Some(enabled) = change.enabled {
                        options.insert("general_log".into(), (enabled as u8).to_string());
                    }
                }
                "slow" => {
                    if let Some(enabled) = change.enabled {
                        options.insert("slow_query_log".into(), (enabled as u8).to_string());
                    }
                    if let Some(value) = threshold.get("slow") {
                        options.insert("long_query_time".into(), value.clone());
                    }
                }
                "binlog" => {
                    if let Some(enabled) = change.enabled {
                        options.remove("skip_log_bin");
                        options.remove("disable_log_bin");
                        options.remove("log_bin");
                        if mariadb {
                            // Off by default in MariaDB; on needs a base name.
                            if enabled {
                                options.insert("log_bin".into(), "mysql-bin".into());
                            }
                        } else if !enabled {
                            // On by default in MySQL 8, where it fills disks.
                            options.insert("skip_log_bin".into(), String::new());
                        }
                    }
                    if let Some(days) = threshold.get("binlog") {
                        if mariadb {
                            options.insert("expire_logs_days".into(), days.clone());
                        } else {
                            let seconds = days.parse::<u64>().unwrap_or(7) * 86_400;
                            options
                                .insert("binlog_expire_logs_seconds".into(), seconds.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
    })
}

pub(crate) fn mysql_set_remote(enabled: bool) -> Result<String, String> {
    update_drop_in(|options| {
        if enabled {
            options.insert("bind_address".into(), "0.0.0.0".into());
        } else {
            options.remove("bind_address");
        }
    })
}

/// Deletes all binary logs except the one in use.
pub fn mysql_purge_binlogs(admin: Option<&MysqlAdmin>) -> Result<u64, String> {
    let admin = admin.ok_or("MySQL is not set up for the panel")?;
    let before = binlog_total(admin);
    mysql_query(admin, "PURGE BINARY LOGS BEFORE NOW();")?;
    Ok(before.saturating_sub(binlog_total(admin)))
}

fn mysql_set_root_password(admin: Option<&MysqlAdmin>, password: &str) -> Result<(), String> {
    let admin = admin.ok_or("MySQL is not set up for the panel")?;
    let vars = mysql_vars(admin);
    let sql = if is_mariadb(&vars) {
        // Keeps root's socket login (what recovery tools use) next to the password.
        format!(
            "ALTER USER 'root'@'localhost' IDENTIFIED VIA unix_socket OR mysql_native_password USING PASSWORD('{}');\nFLUSH PRIVILEGES;\n",
            password
        )
    } else {
        format!(
            "ALTER USER 'root'@'localhost' IDENTIFIED WITH caching_sha2_password BY '{}';\nFLUSH PRIVILEGES;\n",
            password
        )
    };
    match mysql_query(admin, &sql) {
        Ok(_) => Ok(()),
        // Older servers without IDENTIFIED VIA ... OR.
        Err(_) => mysql_query(
            admin,
            &format!(
                "ALTER USER 'root'@'localhost' IDENTIFIED BY '{}';\nFLUSH PRIVILEGES;\n",
                password
            ),
        )
        .map(|_| ()),
    }
}

// ===== PostgreSQL =====

fn pg_show(name: &str) -> Option<String> {
    psql(&format!("SHOW {};", name), None)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

const PG_PARAMS: &[(&str, &str)] = &[
    ("max_connections", "int"),
    ("shared_buffers", "size"),
    ("work_mem", "size"),
    ("effective_cache_size", "size"),
];

/// PostgreSQL reports sizes as "128MB" / "4GB" / "64kB".
fn pg_size_bytes(value: &str) -> String {
    parse_size(
        &value
            .replace("kB", "K")
            .replace("MB", "M")
            .replace("GB", "G"),
    )
    .map(|b| b.to_string())
    .unwrap_or_else(|| value.to_string())
}

const PG_HBA_BEGIN: &str = "# >>> LinxPanel remote access";
const PG_HBA_END: &str = "# <<< LinxPanel remote access";

fn pg_hba_has_block(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| text.contains(PG_HBA_BEGIN))
}

fn pg_log_dir() -> Option<PathBuf> {
    // Debian / Ubuntu clusters log here (pg_ctlcluster); others use log_directory.
    let debian = PathBuf::from("/var/log/postgresql");
    if debian.is_dir() {
        return Some(debian);
    }
    let dir = pg_show("log_directory")?;
    let path = PathBuf::from(&dir);
    Some(if path.is_absolute() {
        path
    } else {
        PathBuf::from(pg_show("data_directory")?).join(path)
    })
}

/// The newest log file of the server.
fn pg_server_log() -> Option<PathBuf> {
    let dir = pg_log_dir()?;
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().is_file())
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
        .map(|e| e.path())
}

fn pg_settings(mut base: EngineSettings) -> EngineSettings {
    base.flavor = "postgresql".into();
    if let Some(version) = pg_show("server_version") {
        base.version = version;
    }
    if let Some(port) = pg_show("port").and_then(|p| p.parse().ok()) {
        base.port = port;
    }
    base.data_dir = pg_show("data_directory");
    base.data_size = base.data_dir.as_deref().and_then(dir_size);
    base.params = PG_PARAMS
        .iter()
        .map(|(key, kind)| {
            let raw = pg_show(key).unwrap_or_default();
            Param {
                key: key.to_string(),
                value: if *kind == "size" {
                    pg_size_bytes(&raw)
                } else {
                    raw
                },
                kind,
                options: Vec::new(),
            }
        })
        .collect();
    let slow_ms = pg_show("log_min_duration_statement").unwrap_or_else(|| "-1".into());
    let log_dir_size = pg_log_dir().map(|dir| {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|e| e.metadata().ok())
                    .filter(|m| m.is_file())
                    .map(|m| m.len())
                    .sum()
            })
            .unwrap_or(0)
    });
    let server_log = pg_server_log();
    base.logs = vec![
        LogInfo {
            kind: "slow",
            enabled: slow_ms != "-1",
            switchable: true,
            path: None,
            size: None,
            threshold: Some(if slow_ms == "-1" {
                "1000".into()
            } else {
                slow_ms.trim_end_matches("ms").to_string()
            }),
        },
        LogInfo {
            kind: "statement",
            enabled: pg_show("log_statement").is_some_and(|v| v == "all"),
            switchable: true,
            path: None,
            size: None,
            threshold: None,
        },
        LogInfo {
            kind: "server",
            enabled: true,
            switchable: false,
            path: server_log.map(|p| p.display().to_string()),
            size: log_dir_size,
            threshold: None,
        },
    ];
    let listen = pg_show("listen_addresses").unwrap_or_default();
    base.remote.enabled = listen.split(',').any(|a| {
        let a = a.trim();
        a == "*" || a == "0.0.0.0" || a == "::"
    }) && pg_show("hba_file")
        .is_some_and(|p| pg_hba_has_block(Path::new(&p)));
    base
}

fn pg_alter(settings: &[(String, Option<String>)]) -> Result<String, String> {
    let mut sql = String::new();
    for (key, value) in settings {
        match value {
            Some(value) => sql.push_str(&format!(
                "ALTER SYSTEM SET {} = {};\n",
                key,
                database::pg_str(value)
            )),
            None => sql.push_str(&format!("ALTER SYSTEM RESET {};\n", key)),
        }
    }
    let auto_conf =
        pg_show("data_directory").map(|d| PathBuf::from(d).join("postgresql.auto.conf"));
    let previous = auto_conf
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok());
    psql(&sql, None)?;
    restart_or_rollback(Engine::Postgresql, move || {
        if let (Some(path), Some(text)) = (auto_conf, previous) {
            restore(&path, Some(&text));
        }
    })
}

fn pg_apply(
    params: &BTreeMap<String, String>,
    logs: &BTreeMap<String, LogChange>,
) -> Result<String, String> {
    let mut changes = Vec::new();
    for (key, value) in params {
        let kind = PG_PARAMS
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, kind)| *kind)
            .ok_or_else(|| format!("Unknown parameter: {}", key))?;
        let value = check_value(kind, &[], value)?;
        // PostgreSQL wants units it knows: bytes become kB.
        let value = if kind == "size" {
            format!("{}kB", parse_size(&value).unwrap_or(0) / 1024)
        } else {
            value
        };
        changes.push((key.clone(), Some(value)));
    }
    for (kind, change) in logs {
        match kind.as_str() {
            "slow" => {
                let ms = match &change.threshold {
                    Some(value) => check_value("int", &[], value)?,
                    None => pg_show("log_min_duration_statement")
                        .map(|v| v.trim_end_matches("ms").to_string())
                        .filter(|v| v != "-1")
                        .unwrap_or_else(|| "1000".into()),
                };
                let enabled = change.enabled.unwrap_or_else(|| {
                    pg_show("log_min_duration_statement").is_some_and(|v| v != "-1")
                });
                changes.push((
                    "log_min_duration_statement".into(),
                    Some(if enabled { ms } else { "-1".into() }),
                ));
            }
            "statement" => {
                if let Some(enabled) = change.enabled {
                    changes.push((
                        "log_statement".into(),
                        Some(if enabled { "all" } else { "none" }.into()),
                    ));
                }
            }
            _ => {}
        }
    }
    if changes.is_empty() {
        return Ok(String::new());
    }
    pg_alter(&changes)
}

fn pg_set_remote(enabled: bool, allow_from: Option<&str>) -> Result<String, String> {
    let hba = PathBuf::from(pg_show("hba_file").ok_or("pg_hba.conf not found")?);
    let text = std::fs::read_to_string(&hba).map_err(|e| e.to_string())?;
    let mut kept = Vec::new();
    let mut skipping = false;
    for line in text.lines() {
        if line.trim() == PG_HBA_BEGIN {
            skipping = true;
        } else if line.trim() == PG_HBA_END {
            skipping = false;
        } else if !skipping {
            kept.push(line.to_string());
        }
    }
    if enabled {
        let method = match pg_show("password_encryption").as_deref() {
            Some("md5") => "md5",
            _ => "scram-sha-256",
        };
        let sources: Vec<String> = match allow_from.map(str::trim).filter(|s| !s.is_empty()) {
            Some(source) => super::panel_access::parse_ip_list(source)?
                .iter()
                .map(|net| {
                    // pg_hba.conf wants a CIDR, also for a single address.
                    let text = net.to_string();
                    if text.contains('/') {
                        text
                    } else if text.contains(':') {
                        format!("{}/128", text)
                    } else {
                        format!("{}/32", text)
                    }
                })
                .collect(),
            None => vec!["0.0.0.0/0".into(), "::/0".into()],
        };
        kept.push(PG_HBA_BEGIN.to_string());
        for source in sources {
            kept.push(format!(
                "host    all             all             {:<23} {}",
                source, method
            ));
        }
        kept.push(PG_HBA_END.to_string());
    }
    let previous = text.clone();
    std::fs::write(&hba, kept.join("\n") + "\n").map_err(|e| e.to_string())?;
    let setting = vec![(
        "listen_addresses".to_string(),
        enabled.then(|| "*".to_string()),
    )];
    let result = pg_alter(&setting);
    if result.is_err() {
        restore(&hba, Some(&previous));
    }
    result
}

fn pg_set_password(password: &str) -> Result<(), String> {
    psql(
        &format!("ALTER ROLE postgres WITH PASSWORD '{}';\n", password),
        None,
    )
    .map(|_| ())
}

// ===== Redis =====

/// The value of a redis.conf directive (the last uncommented occurrence wins).
fn redis_directive(text: &str, key: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let (name, rest) = line.split_once(char::is_whitespace)?;
            name.eq_ignore_ascii_case(key)
                .then(|| rest.trim().to_string())
        })
        .last()
}

/// Sets (or with None removes) every occurrence of a directive, keeping the
/// first one's place in the file.
fn set_redis_directive(text: &str, key: &str, value: Option<&str>) -> String {
    let mut out = Vec::new();
    let mut placed = false;
    for line in text.lines() {
        let trimmed = line.trim();
        let matches = !trimmed.starts_with('#')
            && trimmed
                .split_whitespace()
                .next()
                .is_some_and(|name| name.eq_ignore_ascii_case(key));
        if matches {
            if !placed {
                if let Some(value) = value {
                    out.push(format!("{} {}", key, value));
                }
                placed = true;
            }
            continue;
        }
        out.push(line.to_string());
    }
    if !placed {
        if let Some(value) = value {
            out.push(format!("{} {}", key, value));
        }
    }
    out.join("\n") + "\n"
}

const REDIS_POLICIES: &[&str] = &[
    "noeviction",
    "allkeys-lru",
    "allkeys-lfu",
    "allkeys-random",
    "volatile-lru",
    "volatile-lfu",
    "volatile-random",
    "volatile-ttl",
];

const REDIS_BIND_KEEP: &str = "# linxpanel-local-bind:";

fn redis_settings(mut base: EngineSettings) -> EngineSettings {
    base.flavor = "redis".into();
    let Some(conf) = database::redis_conf() else {
        return base;
    };
    let text = std::fs::read_to_string(&conf).unwrap_or_default();
    if let Some(port) = redis_directive(&text, "port").and_then(|p| p.parse().ok()) {
        base.port = port;
    }
    base.data_dir = redis_directive(&text, "dir");
    base.data_size = base.data_dir.as_deref().and_then(dir_size);
    let maxmemory = redis_directive(&text, "maxmemory").unwrap_or_else(|| "0".into());
    let save = redis_directive(&text, "save");
    base.params = vec![
        Param {
            key: "maxmemory".into(),
            value: parse_size(&maxmemory)
                .map(|b| b.to_string())
                .unwrap_or(maxmemory),
            kind: "size",
            options: Vec::new(),
        },
        Param {
            key: "maxmemory-policy".into(),
            value: redis_directive(&text, "maxmemory-policy")
                .unwrap_or_else(|| "noeviction".into()),
            kind: "enum",
            options: REDIS_POLICIES.to_vec(),
        },
        Param {
            key: "appendonly".into(),
            value: if redis_directive(&text, "appendonly").as_deref() == Some("yes") {
                "on".into()
            } else {
                "off".into()
            },
            kind: "bool",
            options: Vec::new(),
        },
        Param {
            key: "save".into(),
            // `save ""` turns RDB snapshots off; no directive means the defaults.
            value: if save.as_deref() == Some("\"\"") || save.as_deref() == Some("''") {
                "off".into()
            } else {
                "on".into()
            },
            kind: "bool",
            options: Vec::new(),
        },
    ];
    let logfile = redis_directive(&text, "logfile")
        .map(|v| v.trim_matches('"').to_string())
        .filter(|v| !v.is_empty());
    base.logs = vec![LogInfo {
        kind: "server",
        enabled: logfile.is_some(),
        switchable: false,
        size: logfile.as_deref().map(Path::new).and_then(file_size),
        path: logfile,
        threshold: None,
    }];
    base.superuser_password = database::redis_password();
    base.remote.needs_password = true;
    base.remote.enabled = redis_directive(&text, "bind").is_some_and(|bind| {
        bind.split_whitespace()
            .any(|a| a == "0.0.0.0" || a == "*" || a == "::" || a == "-::*")
    });
    base
}

fn update_redis_conf(
    change: impl FnOnce(String) -> Result<String, String>,
) -> Result<String, String> {
    let conf = database::redis_conf().ok_or("redis.conf not found")?;
    let previous = std::fs::read_to_string(&conf).map_err(|e| e.to_string())?;
    let next = change(previous.clone())?;
    if next == previous {
        return Ok(String::new());
    }
    write_config(&conf, &next)?;
    restart_or_rollback(Engine::Redis, move || restore(&conf, Some(&previous)))
}

fn redis_apply(params: &BTreeMap<String, String>) -> Result<String, String> {
    let mut checked = Vec::new();
    for (key, value) in params {
        let value = match key.as_str() {
            "maxmemory" => {
                let bytes = parse_size(&check_value("size", &[], value)?).unwrap_or(0);
                format!("{}mb", bytes / (1 << 20))
            }
            "maxmemory-policy" => check_value("enum", REDIS_POLICIES, value)?,
            "appendonly" => if check_value("bool", &[], value)? == "on" {
                "yes"
            } else {
                "no"
            }
            .to_string(),
            "save" => check_value("bool", &[], value)?,
            other => return Err(format!("Unknown parameter: {}", other)),
        };
        checked.push((key.clone(), value));
    }
    update_redis_conf(|mut text| {
        for (key, value) in checked {
            text = match key.as_str() {
                // Off: an empty save list; on: back to Redis's own defaults.
                "save" if value == "off" => set_redis_directive(&text, "save", Some("\"\"")),
                "save" => set_redis_directive(&text, "save", None),
                "maxmemory" if value == "0mb" => set_redis_directive(&text, "maxmemory", None),
                _ => set_redis_directive(&text, &key, Some(&value)),
            };
        }
        Ok(text)
    })
}

fn redis_set_remote(enabled: bool) -> Result<String, String> {
    if enabled && database::redis_password().is_none() {
        return Err("Set a Redis password before opening it to other machines".into());
    }
    update_redis_conf(|text| {
        let current = redis_directive(&text, "bind");
        let kept = text
            .lines()
            .find_map(|line| line.trim().strip_prefix(REDIS_BIND_KEEP))
            .map(|v| v.trim().to_string());
        let without_note: String = text
            .lines()
            .filter(|line| !line.trim().starts_with(REDIS_BIND_KEEP))
            .map(|line| format!("{}\n", line))
            .collect();
        Ok(if enabled {
            // Remember the local bind, to put it back when switched off.
            let local = kept.or(current).unwrap_or_else(|| "127.0.0.1".into());
            let text = set_redis_directive(&without_note, "bind", Some("0.0.0.0"));
            format!("{}{} {}\n", text, REDIS_BIND_KEEP, local)
        } else {
            let local = kept.unwrap_or_else(|| "127.0.0.1".into());
            set_redis_directive(&without_note, "bind", Some(&local))
        })
    })
}

// ===== Public API =====

#[derive(Debug, Default, Deserialize)]
pub struct LogChange {
    pub enabled: Option<bool>,
    pub threshold: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SettingsChange {
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    #[serde(default)]
    pub logs: BTreeMap<String, LogChange>,
}

pub async fn settings(pool: &SqlitePool, engine: Engine) -> EngineSettings {
    let admin = database::load_mysql_admin(pool).await;
    let stored = match engine {
        Engine::Mysql => crate::db::get_setting(pool, SETTING_MYSQL_ROOT_PASSWORD).await,
        Engine::Postgresql => crate::db::get_setting(pool, SETTING_POSTGRES_PASSWORD).await,
        Engine::Redis => Ok(None),
    }
    .ok()
    .flatten()
    .filter(|v| !v.is_empty());
    tokio::task::spawn_blocking(move || {
        let status = database::status(engine, admin.as_ref());
        let base = EngineSettings {
            engine,
            flavor: engine.as_str().into(),
            version: status.version.clone(),
            service: status.service.clone(),
            running: status.running,
            port: status.port,
            data_dir: None,
            data_size: None,
            remote: RemoteState {
                enabled: false,
                needs_password: false,
            },
            superuser: match engine {
                Engine::Mysql => "root",
                Engine::Postgresql => "postgres",
                Engine::Redis => "requirepass",
            },
            superuser_password: stored,
            params: Vec::new(),
            logs: Vec::new(),
        };
        if !status.running {
            return base;
        }
        match engine {
            Engine::Mysql => mysql_settings(admin.as_ref(), base),
            Engine::Postgresql => pg_settings(base),
            Engine::Redis => redis_settings(base),
        }
    })
    .await
    .unwrap_or_else(|_| unreachable_settings(engine))
}

fn unreachable_settings(engine: Engine) -> EngineSettings {
    EngineSettings {
        engine,
        flavor: engine.as_str().into(),
        version: String::new(),
        service: String::new(),
        running: false,
        port: 0,
        data_dir: None,
        data_size: None,
        remote: RemoteState {
            enabled: false,
            needs_password: false,
        },
        superuser: "",
        superuser_password: None,
        params: Vec::new(),
        logs: Vec::new(),
    }
}

pub async fn apply(
    pool: &SqlitePool,
    engine: Engine,
    change: SettingsChange,
) -> Result<String, String> {
    let admin = database::load_mysql_admin(pool).await;
    tokio::task::spawn_blocking(move || match engine {
        Engine::Mysql => mysql_apply(admin.as_ref(), &change.params, &change.logs),
        Engine::Postgresql => pg_apply(&change.params, &change.logs),
        Engine::Redis => redis_apply(&change.params),
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Opens or closes the engine to other machines (and its firewall port).
pub fn set_remote(
    engine: Engine,
    enabled: bool,
    allow_from: Option<&str>,
) -> Result<String, String> {
    let mut log = match engine {
        Engine::Mysql => mysql_set_remote(enabled)?,
        Engine::Postgresql => pg_set_remote(enabled, allow_from)?,
        Engine::Redis => redis_set_remote(enabled)?,
    };
    if enabled {
        let port = match engine {
            Engine::Mysql => 3306,
            Engine::Postgresql => 5432,
            Engine::Redis => 6379,
        };
        open_firewall(port, allow_from, &mut log);
    }
    Ok(log)
}

/// Sets the superuser password (MySQL root, PostgreSQL postgres, Redis
/// requirepass) and remembers it for the settings page.
pub async fn set_superuser_password(
    pool: &SqlitePool,
    engine: Engine,
    password: &str,
) -> Result<(), String> {
    let password = password.trim().to_string();
    if engine == Engine::Redis && password.is_empty() {
        if settings(pool, engine).await.remote.enabled {
            return Err("Close remote access before removing the Redis password".into());
        }
    } else {
        database::validate_db_password(&password)?;
    }
    let admin = database::load_mysql_admin(pool).await;
    let value = password.clone();
    tokio::task::spawn_blocking(move || match engine {
        Engine::Mysql => mysql_set_root_password(admin.as_ref(), &value),
        Engine::Postgresql => pg_set_password(&value),
        Engine::Redis => database::set_redis_password(&value),
    })
    .await
    .map_err(|e| e.to_string())??;
    let key = match engine {
        Engine::Mysql => SETTING_MYSQL_ROOT_PASSWORD,
        Engine::Postgresql => SETTING_POSTGRES_PASSWORD,
        Engine::Redis => return Ok(()),
    };
    crate::db::set_setting(pool, key, &password)
        .await
        .map_err(|e| e.to_string())
}

pub fn start(engine: Engine) -> Result<String, String> {
    sysservice::start(&database::service_name(engine))
}

pub fn stop(engine: Engine) -> Result<String, String> {
    sysservice::stop(&database::service_name(engine), false)
}

// ----- Configuration files -----

#[derive(Debug, Clone, Serialize)]
pub struct ConfigFile {
    /// What the page passes back: main | panel | hba
    pub id: &'static str,
    pub path: String,
}

/// The files the editor may open: a fixed list, never a path from the browser.
pub fn config_files(engine: Engine) -> Vec<ConfigFile> {
    let mut files = Vec::new();
    match engine {
        Engine::Mysql => {
            if let Some(path) = mysql_main_conf() {
                files.push(ConfigFile {
                    id: "main",
                    path: path.display().to_string(),
                });
            }
            if let Some(path) = mysql_drop_in() {
                files.push(ConfigFile {
                    id: "panel",
                    path: path.display().to_string(),
                });
            }
        }
        Engine::Postgresql => {
            if let Some(path) = pg_show("config_file") {
                files.push(ConfigFile { id: "main", path });
            }
            if let Some(path) = pg_show("hba_file") {
                files.push(ConfigFile { id: "hba", path });
            }
        }
        Engine::Redis => {
            if let Some(path) = database::redis_conf() {
                files.push(ConfigFile {
                    id: "main",
                    path: path.display().to_string(),
                });
            }
        }
    }
    files
}

pub fn read_config(engine: Engine, id: &str) -> Result<(ConfigFile, String), String> {
    let file = config_files(engine)
        .into_iter()
        .find(|f| f.id == id)
        .ok_or("No such configuration file")?;
    let content = std::fs::read_to_string(&file.path).unwrap_or_default();
    Ok((file, content))
}

/// Saves an edited configuration file and restarts; restores it on failure.
pub fn write_config_file(engine: Engine, id: &str, content: &str) -> Result<String, String> {
    if content.len() > 512 * 1024 {
        return Err("The file is too large".into());
    }
    let (file, previous) = read_config(engine, id)?;
    let path = PathBuf::from(&file.path);
    let existed = path.exists();
    let content = if content.ends_with('\n') {
        content.to_string()
    } else {
        format!("{}\n", content)
    };
    write_config(&path, &content)?;
    restart_or_rollback(engine, move || {
        restore(&path, existed.then_some(previous.as_str()))
    })
}

// ----- Logs -----

fn log_path(engine: Engine, kind: &str, admin: Option<&MysqlAdmin>) -> Option<PathBuf> {
    match engine {
        Engine::Mysql => {
            let vars = mysql_vars(admin?);
            let key = match kind {
                "general" => "general_log_file",
                "slow" => "slow_query_log_file",
                "error" => "log_error",
                _ => return None,
            };
            mysql_log_path(&vars, key)
        }
        Engine::Postgresql => (kind == "server").then(pg_server_log).flatten(),
        Engine::Redis => {
            let text = std::fs::read_to_string(database::redis_conf()?).ok()?;
            (kind == "server")
                .then(|| redis_directive(&text, "logfile"))
                .flatten()
                .map(|v| PathBuf::from(v.trim_matches('"')))
                .filter(|p| p.is_absolute())
        }
    }
}

/// The last `lines` lines of a log (reads at most the last 2 MB).
pub async fn tail_log(
    pool: &SqlitePool,
    engine: Engine,
    kind: &str,
    lines: usize,
) -> Result<String, String> {
    let admin = database::load_mysql_admin(pool).await;
    let kind = kind.to_string();
    tokio::task::spawn_blocking(move || {
        let path = log_path(engine, &kind, admin.as_ref()).ok_or("This log is not available")?;
        use std::io::{Read, Seek, SeekFrom};
        let mut file =
            std::fs::File::open(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let start = len.saturating_sub(2 * 1024 * 1024);
        file.seek(SeekFrom::Start(start))
            .map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&bytes);
        let all: Vec<&str> = text.lines().collect();
        Ok(all[all.len().saturating_sub(lines.clamp(1, 5000))..].join("\n"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Empties a log file (frees its disk space at once).
pub async fn clear_log(pool: &SqlitePool, engine: Engine, kind: &str) -> Result<(), String> {
    let admin = database::load_mysql_admin(pool).await;
    let kind = kind.to_string();
    tokio::task::spawn_blocking(move || {
        let path = log_path(engine, &kind, admin.as_ref()).ok_or("This log is not available")?;
        std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .map(|_| ())
            .map_err(|e| format!("{}: {}", path.display(), e))
    })
    .await
    .map_err(|e| e.to_string())?
}

// ----- Uninstall -----

/// Installed packages of an engine (exact names, for the package manager).
fn installed_packages(engine: Engine) -> Vec<String> {
    let wanted = |name: &str| -> bool {
        match engine {
            Engine::Mysql => {
                [
                    "mariadb-server",
                    "mariadb-client",
                    "mysql-server",
                    "mysql-client",
                    "mariadb-plugin-",
                    "mariadb-backup",
                ]
                .iter()
                .any(|p| name.starts_with(p))
                    || name == "mariadb"
                    || name == "mysql"
            }
            Engine::Postgresql => {
                name == "postgresql"
                    || name == "postgresql-server"
                    || name == "postgresql-contrib"
                    || name
                        .strip_prefix("postgresql-")
                        .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit()))
                    || name
                        .strip_prefix("postgresql-client-")
                        .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit()))
            }
            Engine::Redis => name == "redis-server" || name == "redis" || name == "redis-tools",
        }
    };
    let listing = match runtime::package_manager() {
        "apt-get" => shell::run("dpkg-query", &["-W", "-f=${Package} ${db:Status-Abbrev}\n"])
            .map(|o| {
                o.stdout
                    .lines()
                    .filter(|l| {
                        l.split_whitespace()
                            .nth(1)
                            .is_some_and(|s| s.starts_with("ii"))
                    })
                    .filter_map(|l| l.split_whitespace().next().map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        "dnf" | "yum" => shell::run("rpm", &["-qa", "--qf", "%{NAME}\n"])
            .map(|o| o.stdout.lines().map(String::from).collect())
            .unwrap_or_default(),
        "apk" => shell::run("apk", &["info"])
            .map(|o| o.stdout.lines().map(String::from).collect())
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let mut packages: Vec<String> = listing.into_iter().filter(|n| wanted(n)).collect();
    packages.sort();
    packages.dedup();
    packages
}

fn data_dirs(engine: Engine) -> &'static [&'static str] {
    match engine {
        Engine::Mysql => &["/var/lib/mysql"],
        Engine::Postgresql => &["/var/lib/postgresql", "/var/lib/pgsql"],
        Engine::Redis => &["/var/lib/redis"],
    }
}

/// Removes the engine. Data stays unless `purge_data`; the log goes to the
/// task window as it happens.
pub fn uninstall(engine: Engine, purge_data: bool) -> Result<String, String> {
    let mut log = String::new();
    let mut note = |line: String| {
        super::tasks::note(&line);
        log.push_str(&line);
    };
    note(format!(
        "[LinxPanel] Uninstalling {}\n",
        engine.display_name()
    ));
    let service = database::service_name(engine);
    if let Ok(text) = sysservice::stop(&service, true) {
        note(text);
    }
    let packages = installed_packages(engine);
    if packages.is_empty() {
        note("[LinxPanel] No installed packages found\n".into());
    } else {
        note(format!("[LinxPanel] Removing: {}\n", packages.join(" ")));
        let refs: Vec<&str> = packages.iter().map(String::as_str).collect();
        let manager = runtime::package_manager();
        let verb = match (manager, purge_data) {
            ("apt-get", true) => "purge",
            ("apk", _) => "del",
            _ => "remove",
        };
        let mut args = vec![verb, "-y"];
        if manager == "apk" {
            args.pop();
        }
        args.extend(refs.iter().copied());
        let output = runtime::run_package_manager(manager, &args, Duration::from_secs(1800))?;
        note(format!("{}{}", output.stdout, output.stderr));
        if !output.ok() {
            return Err(format!("{}\n[LinxPanel] Package removal failed", log));
        }
    }
    if engine == Engine::Mysql {
        if let Some(path) = mysql_drop_in() {
            let _ = std::fs::remove_file(path);
        }
    }
    if purge_data {
        for dir in data_dirs(engine) {
            if Path::new(dir).exists() {
                let _ = shell::run_privileged("rm", &["-rf", "--one-file-system", dir]);
                note(format!("[LinxPanel] Deleted {}\n", dir));
            }
        }
    } else {
        note("[LinxPanel] Data directories were kept\n".into());
    }
    note(format!("[LinxPanel] {} removed\n", engine.display_name()));
    Ok(log)
}

/// Forgets what the panel recorded about a removed engine.
pub async fn forget(pool: &SqlitePool, engine: Engine, purge_data: bool) {
    if purge_data {
        let _ = sqlx::query("DELETE FROM databases WHERE engine = ?")
            .bind(engine.as_str())
            .execute(pool)
            .await;
    }
    let keys: &[&str] = match engine {
        Engine::Mysql => &[
            database::SETTING_MYSQL_ADMIN_USER,
            database::SETTING_MYSQL_ADMIN_PASSWORD,
            SETTING_MYSQL_ROOT_PASSWORD,
        ],
        Engine::Postgresql => &[SETTING_POSTGRES_PASSWORD],
        Engine::Redis => &[],
    };
    for key in keys {
        let _ = sqlx::query("DELETE FROM settings WHERE key = ?")
            .bind(key)
            .execute(pool)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_parse() {
        assert_eq!(parse_size("128M"), Some(128 << 20));
        assert_eq!(parse_size("1G"), Some(1 << 30));
        assert_eq!(parse_size("4096"), Some(4096));
        assert_eq!(parse_size("64kb"), Some(64 << 10));
        assert_eq!(parse_size("12x"), None);
        assert_eq!(pg_size_bytes("128MB"), (128u64 << 20).to_string());
    }

    #[test]
    fn values_are_checked_by_kind() {
        assert!(check_value("int", &[], "151").is_ok());
        assert!(check_value("int", &[], "15; DROP").is_err());
        assert!(check_value("size", &[], "256M").is_ok());
        assert!(check_value("size", &[], "256M\nbind-address=0.0.0.0").is_err());
        assert!(check_value("enum", REDIS_POLICIES, "allkeys-lru").is_ok());
        assert!(check_value("enum", REDIS_POLICIES, "anything").is_err());
        assert!(check_value("decimal", &[], "0.5").is_ok());
    }

    #[test]
    fn drop_in_round_trips() {
        let text = "# x\n[mysqld]\nbind-address = 0.0.0.0\nskip-log-bin\nmax_connections = 300\n";
        let options = parse_drop_in(text);
        assert_eq!(
            options.get("bind_address").map(String::as_str),
            Some("0.0.0.0")
        );
        assert_eq!(options.get("skip_log_bin").map(String::as_str), Some(""));
        let rendered = render_drop_in(&options);
        assert!(rendered.contains("[mysqld]\n"));
        assert!(rendered.contains("skip_log_bin\n"));
        assert!(rendered.contains("max_connections = 300\n"));
        assert_eq!(parse_drop_in(&rendered), options);
    }

    #[test]
    fn redis_directives() {
        let text = "# bind 1.2.3.4\nbind 127.0.0.1 -::1\nport 6379\nsave 3600 1\nsave 300 100\n";
        assert_eq!(
            redis_directive(text, "bind").as_deref(),
            Some("127.0.0.1 -::1")
        );
        let off = set_redis_directive(text, "save", Some("\"\""));
        assert_eq!(off.matches("save").count(), 1);
        assert!(off.contains("save \"\"\n"));
        assert!(off.contains("# bind 1.2.3.4"));
        let added = set_redis_directive(text, "maxmemory", Some("256mb"));
        assert!(added.ends_with("maxmemory 256mb\n"));
        let removed = set_redis_directive(text, "save", None);
        assert!(!removed.contains("\nsave "));
    }
}
