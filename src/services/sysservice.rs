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

/// Starts the service (and enables it at boot).
pub fn start(name: &str) -> Result<String, String> {
    enable_and_restart(name)
}

/// Stops the service; `disable` also keeps it from starting at boot.
pub fn stop(name: &str, disable: bool) -> Result<String, String> {
    let timeout = std::time::Duration::from_secs(120);
    let run = |program: &str, args: &[&str]| -> Result<String, String> {
        let output = shell::run_privileged_with_timeout(program, args, timeout)?;
        let text = format!(
            "$ {} {}\n{}{}",
            program,
            args.join(" "),
            output.stdout,
            output.stderr
        );
        if output.ok() {
            Ok(text)
        } else {
            Err(format!("{}{}", text, output.error_text()))
        }
    };
    if systemd_running() {
        let mut log = run("systemctl", &["stop", name])?;
        if disable {
            log.push_str(&run("systemctl", &["disable", name]).unwrap_or_default());
        }
        Ok(log)
    } else if shell::has_binary("rc-service") {
        let mut log = run("rc-service", &[name, "stop"])?;
        if disable {
            log.push_str(&run("rc-update", &["del", name, "default"]).unwrap_or_default());
        }
        Ok(log)
    } else {
        run("service", &[name, "stop"])
    }
}
