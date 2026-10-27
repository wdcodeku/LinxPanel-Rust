//! Database server versions and network access.
//!
//! - Versions: what this server's own package sources offer (MariaDB or MySQL,
//!   every PostgreSQL major version), detected live, so a choice never points
//!   at a package that does not exist here.
//! - Remote access for MySQL / MariaDB: Debian and Ubuntu bind it to
//!   127.0.0.1, so a user allowed from '%' still cannot connect from outside.
//!   A drop-in file sets bind-address; the firewall port can be opened too.
//! - Diagnosis: listening addresses, firewall and user hosts, explained, so a
//!   failed connection (e.g. "2013 Lost connection ... handshake") has a reason.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::database::Engine;
use super::{firewall, runtime, shell};

// ===== Versions =====

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct VersionOption {
    /// Passed back to install: "mariadb" | "mysql" | "16" | "default".
    pub id: String,
    pub label: String,
    /// Full package version, for display.
    pub version: String,
    pub default: bool,
}

/// Candidate version of an apt package, or None when not available.
fn apt_candidate(package: &str) -> Option<String> {
    let output = shell::run("apt-cache", &["policy", package]).ok()?;
    output
        .stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("Candidate:"))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty() && v != "(none)")
}

/// "1:10.11.14-0ubuntu0.24.04.1" -> "10.11"; "8.0.43-0ubuntu0.24.04.1" -> "8.0".
pub fn major_minor(version: &str) -> String {
    let version = version.split_once(':').map(|(_, v)| v).unwrap_or(version);
    let mut parts = version.split(['.', '-', '+', '~']);
    match (parts.next(), parts.next()) {
        (Some(a), Some(b))
            if a.chars().all(|c| c.is_ascii_digit()) && b.chars().all(|c| c.is_ascii_digit()) =>
        {
            format!("{}.{}", a, b)
        }
        (Some(a), _) => a.to_string(),
        _ => version.to_string(),
    }
}

/// PostgreSQL major versions offered by apt (postgresql-16, postgresql-17 ...).
fn apt_postgres_majors() -> Vec<String> {
    let Ok(output) = shell::run("apt-cache", &["pkgnames", "postgresql-"]) else {
        return Vec::new();
    };
    let mut majors: Vec<u32> = output
        .stdout
        .lines()
        .filter_map(|name| name.strip_prefix("postgresql-"))
        .filter(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|rest| rest.parse().ok())
        .filter(|major| *major >= 10)
        .collect();
    majors.sort_unstable();
    majors.dedup();
    majors.into_iter().rev().map(|m| m.to_string()).collect()
}

/// The version the distro's `postgresql` metapackage points to.
fn apt_postgres_default() -> Option<String> {
    let output = shell::run("apt-cache", &["depends", "postgresql"]).ok()?;
    output.stdout.lines().find_map(|line| {
        line.trim()
            .strip_prefix("Depends: postgresql-")
            .filter(|v| v.chars().all(|c| c.is_ascii_digit()))
            .map(str::to_string)
    })
}

/// Versions this server can install. The first option is the default; an
/// empty list means "use the distro default" (install still works).
pub fn versions(engine: Engine) -> Vec<VersionOption> {
    let manager = runtime::package_manager();
    let mut options = Vec::new();
    match (engine, manager) {
        (Engine::Mysql, "apt-get") => {
            if let Some(v) = apt_candidate("mariadb-server") {
                options.push(VersionOption {
                    id: "mariadb".into(),
                    label: format!("MariaDB {}", major_minor(&v)),
                    version: v,
                    default: true,
                });
            }
            // Ubuntu ships Oracle MySQL too; Debian does not.
            if let Some(v) = apt_candidate("mysql-server") {
                if !v.contains("mariadb") {
                    options.push(VersionOption {
                        id: "mysql".into(),
                        label: format!("MySQL {}", major_minor(&v)),
                        version: v,
                        default: options.is_empty(),
                    });
                }
            }
        }
        (Engine::Mysql, "dnf" | "yum") => {
            for (id, package, name) in [
                ("mariadb", "mariadb-server", "MariaDB"),
                ("mysql", "mysql-server", "MySQL"),
            ] {
                if let Some(v) = dnf_available(manager, package) {
                    options.push(VersionOption {
                        id: id.into(),
                        label: format!("{} {}", name, major_minor(&v)),
                        version: v,
                        default: options.is_empty(),
                    });
                }
            }
        }
        (Engine::Postgresql, "apt-get") => {
            let default = apt_postgres_default();
            for major in apt_postgres_majors() {
                let version = apt_candidate(&format!("postgresql-{}", major)).unwrap_or_default();
                if version.is_empty() {
                    continue;
                }
                options.push(VersionOption {
                    default: default.as_deref() == Some(major.as_str()),
                    label: format!("PostgreSQL {}", major),
                    id: major,
                    version,
                });
            }
            // Default first.
            options.sort_by_key(|o| !o.default);
        }
        (Engine::Redis, "apt-get") => {
            if let Some(v) = apt_candidate("redis-server") {
                options.push(VersionOption {
                    id: "default".into(),
                    label: format!("Redis {}", major_minor(&v)),
                    version: v,
                    default: true,
                });
            }
        }
        _ => {}
    }
    options
}

fn dnf_available(manager: &str, package: &str) -> Option<String> {
    let output = shell::run(manager, &["-q", "info", package]).ok()?;
    if !output.ok() {
        return None;
    }
    let field = |name: &str| {
        output.stdout.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == name).then(|| value.trim().to_string())
        })
    };
    Some(format!(
        "{}-{}",
        field("Version")?,
        field("Release").unwrap_or_default()
    ))
}

/// Packages to install for a chosen version id (None: the old defaults).
pub fn packages_for(engine: Engine, manager: &str, version: Option<&str>) -> Option<Vec<String>> {
    let owned = |list: &[&str]| Some(list.iter().map(|s| s.to_string()).collect());
    match (engine, manager, version) {
        (_, _, None | Some("") | Some("default")) => None,
        (Engine::Mysql, "apt-get", Some("mariadb")) => owned(&["mariadb-server", "mariadb-client"]),
        (Engine::Mysql, "apt-get", Some("mysql")) => owned(&["mysql-server", "mysql-client"]),
        (Engine::Mysql, "dnf" | "yum", Some("mariadb")) => owned(&["mariadb-server", "mariadb"]),
        (Engine::Mysql, "dnf" | "yum", Some("mysql")) => owned(&["mysql-server", "mysql"]),
        (Engine::Postgresql, "apt-get", Some(major))
            if major.len() <= 3 && major.chars().all(|c| c.is_ascii_digit()) =>
        {
            Some(vec![
                format!("postgresql-{}", major),
                format!("postgresql-client-{}", major),
            ])
        }
        _ => None,
    }
}

// ===== MySQL / MariaDB network access =====

/// Sorts after the distro's own files (50-server.cnf, mysqld.cnf, ...), so
/// its settings win.
const DROP_IN: &str = "zz-linxpanel.cnf";

fn mysql_conf_dir() -> Option<PathBuf> {
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

fn drop_in_path() -> Option<PathBuf> {
    mysql_conf_dir().map(|dir| dir.join(DROP_IN))
}

#[derive(Debug, Clone, Serialize)]
pub struct Listener {
    pub address: String,
    pub process: String,
}

/// Who listens on `port` (TCP), from `ss`.
pub fn listeners(port: u16) -> Vec<Listener> {
    let filter = format!("sport = :{}", port);
    let Ok(output) = shell::run_best_effort("ss", &["-ltnpH", &filter]) else {
        return Vec::new();
    };
    output
        .stdout
        .lines()
        .filter_map(|line| {
            let cols: Vec<&str> = line.split_whitespace().collect();
            let address = cols.get(3)?.to_string();
            let process = line
                .split("((\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .unwrap_or_default()
                .to_string();
            Some(Listener { address, process })
        })
        .collect()
}

fn is_loopback_listen(address: &str) -> bool {
    let host = address.rsplit_once(':').map(|(h, _)| h).unwrap_or(address);
    let host = host.trim_matches(['[', ']']);
    host.starts_with("127.") || host == "::1" || host == "localhost"
}

#[derive(Debug, Clone, Serialize)]
pub struct FirewallCheck {
    pub backend: String,
    pub enabled: bool,
    /// Rules that allow the port (sources), when the firewall is active.
    pub allowed_from: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetworkInfo {
    pub port: u16,
    pub listeners: Vec<Listener>,
    /// Some listener is reachable from other machines.
    pub public: bool,
    /// The panel's drop-in is in place.
    pub remote_enabled: bool,
    /// Another program holds the port (e.g. a leftover docker-proxy).
    pub foreign_listener: Option<String>,
    pub firewall: FirewallCheck,
}

pub fn network_info(engine: Engine) -> NetworkInfo {
    let port = engine_port(engine);
    let listeners = listeners(port);
    let public = listeners.iter().any(|l| !is_loopback_listen(&l.address));
    let foreign_listener = listeners.iter().map(|l| l.process.clone()).find(|p| {
        !p.is_empty() && !["mariadbd", "mysqld", "postgres", "redis-server"].contains(&p.as_str())
    });
    let fw = firewall::status();
    let allowed_from = if fw.enabled {
        firewall::list_rules()
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.port.split(['/', ':']).next() == Some(port.to_string().as_str()))
            .filter(|r| !r.action.to_ascii_uppercase().contains("DENY"))
            .map(|r| {
                if r.source.is_empty() {
                    "any".to_string()
                } else {
                    r.source
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    NetworkInfo {
        port,
        listeners,
        public,
        remote_enabled: drop_in_path().is_some_and(|p| p.exists()),
        foreign_listener,
        firewall: FirewallCheck {
            backend: fw.backend,
            enabled: fw.enabled,
            allowed_from,
        },
    }
}

fn engine_port(engine: Engine) -> u16 {
    match engine {
        Engine::Mysql => 3306,
        Engine::Postgresql => 5432,
        Engine::Redis => 6379,
    }
}

pub const DROP_IN_CONTENT: &str = "\
# Written by LinxPanel (Databases > Remote access). Removing remote access deletes this file.
[mysqld]
bind-address = 0.0.0.0
";

/// Turns remote access on or off: the bind address, a service restart, and
/// (on) a firewall rule for the port, limited to `allow_from` when given.
pub fn set_mysql_remote(enabled: bool, allow_from: Option<&str>) -> Result<String, String> {
    let path = drop_in_path().ok_or("MySQL / MariaDB configuration directory not found")?;
    let mut log = String::new();
    if enabled {
        write_root_file(&path, DROP_IN_CONTENT)?;
        log.push_str(&format!("Wrote {}\n", path.display()));
    } else if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("Cannot remove {}: {}", path.display(), e))?;
        log.push_str(&format!("Removed {}\n", path.display()));
    }
    log.push_str(&super::database::restart(Engine::Mysql)?);

    if enabled {
        let fw = firewall::status();
        if fw.enabled && fw.manageable {
            let request = firewall::FirewallRuleRequest {
                port: "3306".into(),
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
    // Give the server a moment, then confirm what it listens on.
    std::thread::sleep(std::time::Duration::from_millis(500));
    let public = listeners(3306)
        .iter()
        .any(|l| !is_loopback_listen(&l.address));
    if enabled && !public {
        return Err(format!(
            "{}MySQL still listens on localhost only; check {} for another bind-address",
            log,
            mysql_conf_dir()
                .map(|d| d.display().to_string())
                .unwrap_or_default()
        ));
    }
    Ok(log)
}

fn write_root_file(path: &Path, content: &str) -> Result<(), String> {
    std::fs::write(path, content).map_err(|e| format!("Cannot write {}: {}", path.display(), e))?;
    // Readable by mysqld (which does not run as root); world-writable it must not be.
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_shortened_for_labels() {
        assert_eq!(major_minor("1:10.11.14-0ubuntu0.24.04.1"), "10.11");
        assert_eq!(major_minor("8.0.43-0ubuntu0.24.04.1"), "8.0");
        assert_eq!(major_minor("5:7.0.15-1ubuntu0.24.04.5"), "7.0");
        assert_eq!(major_minor("16+257build1.1"), "16");
    }

    #[test]
    fn version_ids_map_to_packages() {
        assert_eq!(packages_for(Engine::Mysql, "apt-get", None), None);
        assert_eq!(
            packages_for(Engine::Mysql, "apt-get", Some("default")),
            None
        );
        assert_eq!(
            packages_for(Engine::Mysql, "apt-get", Some("mysql")).unwrap(),
            vec!["mysql-server", "mysql-client"]
        );
        assert_eq!(
            packages_for(Engine::Postgresql, "apt-get", Some("16")).unwrap(),
            vec!["postgresql-16", "postgresql-client-16"]
        );
        // Nothing that is not a plain version number reaches apt.
        assert_eq!(
            packages_for(Engine::Postgresql, "apt-get", Some("16; rm -rf /")),
            None
        );
        assert_eq!(
            packages_for(Engine::Mysql, "apt-get", Some("percona")),
            None
        );
    }

    #[test]
    fn loopback_listeners_are_recognised() {
        for local in ["127.0.0.1:3306", "[::1]:3306", "127.0.0.53:53"] {
            assert!(is_loopback_listen(local), "{local}");
        }
        for public in ["0.0.0.0:3306", "*:3306", "[::]:3306", "203.0.113.4:3306"] {
            assert!(!is_loopback_listen(public), "{public}");
        }
    }
}
