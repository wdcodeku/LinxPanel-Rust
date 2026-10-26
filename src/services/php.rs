//! PHP-FPM integration: find the distro's FPM pool, tell nginx where it listens,
//! and make that socket reachable by the web server.
//!
//! Debian/Ubuntu listen on `/run/php/php8.x-fpm.sock` owned by www-data with mode
//! 0660, RHEL on `/run/php-fpm/www.sock` restricted by `listen.acl_users`, while
//! OpenResty workers run as `nobody` - so out of the box nginx gets "permission
//! denied" and every PHP site answers 502. Like BaoTa, the pool socket is opened
//! to local users (mode 0666) instead of changing nginx's worker user.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::sysservice;

#[derive(Debug, Clone, Serialize)]
pub struct FpmPool {
    /// The pool config file (`www.conf`).
    pub conf: PathBuf,
    /// e.g. "8.3"; empty when the layout does not carry one.
    pub version: String,
    /// Service name to start/restart, e.g. "php8.3-fpm" or "php-fpm".
    pub service: String,
    /// Account PHP runs as; files PHP must write (uploads, caches) belong to it.
    pub user: String,
    /// Raw `listen =` value.
    pub listen: String,
}

impl FpmPool {
    /// The `fastcgi_pass` target for this pool.
    pub fn fastcgi_addr(&self) -> String {
        fastcgi_addr(&self.listen)
    }
}

pub fn fastcgi_addr(listen: &str) -> String {
    let listen = listen.trim();
    if listen.starts_with('/') {
        format!("unix:{}", listen)
    } else if listen.chars().all(|c| c.is_ascii_digit()) {
        format!("127.0.0.1:{}", listen)
    } else {
        listen.to_string()
    }
}

fn sub_dirs(dir: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    dirs
}

/// Pool configs on this host, newest PHP version last.
fn candidate_pools() -> Vec<(PathBuf, String, String)> {
    let mut pools = Vec::new();
    // Debian / Ubuntu: /etc/php/8.3/fpm/pool.d/www.conf, service php8.3-fpm
    for dir in sub_dirs(Path::new("/etc/php")) {
        let version = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let conf = dir.join("fpm/pool.d/www.conf");
        if conf.exists() {
            pools.push((conf, version.clone(), format!("php{}-fpm", version)));
        }
    }
    // RHEL / Fedora: /etc/php-fpm.d/www.conf, service php-fpm
    let conf = PathBuf::from("/etc/php-fpm.d/www.conf");
    if conf.exists() {
        pools.push((conf, String::new(), "php-fpm".into()));
    }
    // Alpine: /etc/php83/php-fpm.d/www.conf, service php-fpm83
    for dir in sub_dirs(Path::new("/etc")) {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if let Some(digits) = name
            .strip_prefix("php")
            .filter(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit()))
        {
            let conf = dir.join("php-fpm.d/www.conf");
            if conf.exists() {
                let version = format!("{}.{}", &digits[..1], &digits[1..]);
                pools.push((conf, version, format!("php-fpm{}", digits)));
            }
        }
    }
    pools
}

/// Reads `key = value` from an ini-style pool file, ignoring comments.
fn ini_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        if line.starts_with(';') || line.starts_with('#') {
            return None;
        }
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

/// The newest installed FPM pool, if any.
pub fn detect() -> Option<FpmPool> {
    candidate_pools()
        .into_iter()
        .filter_map(|(conf, version, service)| {
            let text = std::fs::read_to_string(&conf).ok()?;
            Some(FpmPool {
                listen: ini_value(&text, "listen").unwrap_or_else(|| "127.0.0.1:9000".into()),
                user: ini_value(&text, "user").unwrap_or_else(|| "nobody".into()),
                conf,
                version,
                service,
            })
        })
        .max_by(|a, b| version_key(&a.version).cmp(&version_key(&b.version)))
}

fn version_key(version: &str) -> Vec<u32> {
    version
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// Rewrites a pool config so the socket is usable by the web server: mode 0666
/// and no ACL (an ACL overrides the mode). Returns None when nothing changes.
fn patch_pool_config(text: &str) -> Option<String> {
    let mut changed = false;
    let mut has_mode = false;
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let key = trimmed
            .split_once('=')
            .map(|(k, _)| k.trim())
            .unwrap_or_default();
        // Commented-out lines keep their ';' in `key`, so they never match here.
        if key == "listen.acl_users" || key == "listen.acl_groups" {
            lines.push(format!(
                ";{} ; disabled by LinxPanel so nginx can connect",
                line
            ));
            changed = true;
            continue;
        }
        if key == "listen.mode" || key == ";listen.mode" {
            has_mode = true;
            if trimmed != "listen.mode = 0666" {
                lines.push("listen.mode = 0666".into());
                changed = true;
                continue;
            }
        }
        lines.push(line.to_string());
    }
    if !has_mode {
        lines.push("listen.mode = 0666".into());
        changed = true;
    }
    changed.then(|| lines.join("\n") + "\n")
}

/// Makes the detected pool usable by OpenResty and (re)starts PHP-FPM.
/// Returns a log of what was done.
pub fn prepare() -> Result<(FpmPool, String), String> {
    let pool = detect().ok_or("PHP-FPM is not installed")?;
    let mut log = format!(
        "[LinxPanel] PHP-FPM {} pool: {} (listen {}, user {})\n",
        pool.version,
        pool.conf.display(),
        pool.listen,
        pool.user
    );
    let mut changed = false;
    if pool.listen.starts_with('/') {
        let text = std::fs::read_to_string(&pool.conf).map_err(|e| e.to_string())?;
        if let Some(patched) = patch_pool_config(&text) {
            std::fs::write(&pool.conf, patched)
                .map_err(|e| format!("Cannot update {}: {}", pool.conf.display(), e))?;
            log.push_str("[LinxPanel] Set listen.mode = 0666 so nginx can reach the socket\n");
            changed = true;
        }
    }
    // Restarting interrupts requests of every PHP site, so only when needed.
    if !changed && sysservice::is_active(&pool.service) {
        log.push_str(&format!("[LinxPanel] {} is running\n", pool.service));
        return Ok((pool, log));
    }
    match sysservice::enable_and_restart(&pool.service) {
        Ok(text) => log.push_str(&text),
        Err(err) => return Err(format!("{}Cannot start {}: {}", log, pool.service, err)),
    }
    Ok((pool, log))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fastcgi_addresses() {
        assert_eq!(
            fastcgi_addr("/run/php/php8.3-fpm.sock"),
            "unix:/run/php/php8.3-fpm.sock"
        );
        assert_eq!(fastcgi_addr("9000"), "127.0.0.1:9000");
        assert_eq!(fastcgi_addr("127.0.0.1:9001"), "127.0.0.1:9001");
    }

    #[test]
    fn pool_config_is_opened_for_nginx() {
        let debian = "[www]\nuser = www-data\nlisten = /run/php/php8.3-fpm.sock\nlisten.owner = www-data\n;listen.mode = 0660\n";
        let patched = patch_pool_config(debian).unwrap();
        assert!(patched.contains("\nlisten.mode = 0666\n"));
        assert!(!patched.contains(";listen.mode"));
        assert!(
            patch_pool_config(&patched).is_none(),
            "patching is idempotent"
        );

        let rhel = "listen = /run/php-fpm/www.sock\nlisten.acl_users = apache,nginx\n";
        let patched = patch_pool_config(rhel).unwrap();
        assert!(patched.contains(";listen.acl_users = apache,nginx"));
        assert!(patched.contains("listen.mode = 0666"));
    }

    #[test]
    fn ini_values_skip_comments() {
        let text = "; listen = /nope\nlisten = /run/php.sock\nuser = \"www-data\"\n";
        assert_eq!(ini_value(text, "listen").as_deref(), Some("/run/php.sock"));
        assert_eq!(ini_value(text, "user").as_deref(), Some("www-data"));
    }
}
