//! Starting and enabling system services (php-fpm, MariaDB, PostgreSQL, Redis...)
//! under whatever init system the host has: systemd, OpenRC, or SysV `service`
//! (the latter two cover containers and WSL, where systemd is not running).

use std::path::Path;

use super::shell;

pub fn systemd_running() -> bool {
    Path::new("/run/systemd/system").exists()
}

/// Whether a unit file / init script by this name exists.
pub fn exists(name: &str) -> bool {
    if systemd_running() {
        return shell::run("systemctl", &["cat", &format!("{}.service", name)])
            .map(|o| o.ok())
            .unwrap_or(false);
    }
    Path::new("/etc/init.d").join(name).exists()
}

/// The first of `candidates` that is installed as a service.
pub fn first_existing(candidates: &[String]) -> Option<String> {
    candidates.iter().find(|name| exists(name)).cloned()
}

pub fn is_active(name: &str) -> bool {
    let output = if systemd_running() {
        shell::run("systemctl", &["is-active", "--quiet", name])
    } else if shell::has_binary("rc-service") {
        shell::run_best_effort("rc-service", &[name, "status"])
    } else {
        shell::run_best_effort("service", &[name, "status"])
    };
    output.map(|o| o.ok()).unwrap_or(false)
}

/// Enables the service at boot (where the init system supports it) and starts
/// or restarts it. Returns a short log.
pub fn enable_and_restart(name: &str) -> Result<String, String> {
    let mut log = String::new();
    let mut step = |program: &str, args: &[&str]| -> Result<(), String> {
        log.push_str(&format!("$ {} {}\n", program, args.join(" ")));
        let output =
            shell::run_privileged_with_timeout(program, args, std::time::Duration::from_secs(120))?;
        let text = format!("{}{}", output.stdout, output.stderr);
        if !text.trim().is_empty() {
            log.push_str(text.trim_end());
            log.push('\n');
        }
        if output.ok() {
            Ok(())
        } else {
            Err(output.error_text())
        }
    };

    let result = if systemd_running() {
        step("systemctl", &["enable", name]).ok();
        step("systemctl", &["restart", name])
    } else if shell::has_binary("rc-service") {
        step("rc-update", &["add", name, "default"]).ok();
        step("rc-service", &[name, "restart"])
    } else {
        step("service", &[name, "restart"])
    };
    match result {
        Ok(()) => Ok(log),
        Err(err) => Err(format!("{}\n{}", log, err)),
    }
}
