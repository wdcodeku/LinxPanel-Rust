//! System service control.
//!
//! Linux drives systemd. macOS exposes launchd read-only (enough to inspect a dev box
//! without pretending the panel can manage launchd units properly). Windows is unsupported.

use serde::{Deserialize, Serialize};

use super::shell;

#[derive(Debug, Clone, Serialize)]
pub struct ServiceRow {
    pub name: String,
    pub description: String,
    /// loaded / not-found / masked
    pub load_state: String,
    /// active / inactive / failed / activating
    pub active_state: String,
    /// running / exited / dead
    pub sub_state: String,
    /// enabled / disabled / static / unknown
    pub enabled: String,
    pub can_manage: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceBackend {
    /// systemd | launchd | none
    pub kind: String,
    pub available: bool,
    pub manageable: bool,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct ServiceActionRequest {
    pub name: String,
    /// start | stop | restart | reload | enable | disable
    pub action: String,
}

pub fn backend() -> ServiceBackend {
    if cfg!(target_os = "linux") && shell::has_binary("systemctl") {
        let manageable = shell::is_root() || sudo_available();
        return ServiceBackend {
            kind: "systemd".into(),
            available: true,
            manageable,
            message: if manageable {
                String::new()
            } else {
                "Read-only: service control needs root or passwordless sudo".into()
            },
        };
    }
    if cfg!(target_os = "macos") && shell::has_binary("launchctl") {
        return ServiceBackend {
            kind: "launchd".into(),
            available: true,
            manageable: false,
            message: "launchd is listed read-only; service control is Linux/systemd only".into(),
        };
    }
    ServiceBackend {
        kind: "none".into(),
        available: false,
        manageable: false,
        message: "No supported service manager found (systemd required)".into(),
    }
}

fn sudo_available() -> bool {
    shell::run("sudo", &["-n", "true"])
        .map(|output| output.ok())
        .unwrap_or(false)
}

pub fn list() -> Result<Vec<ServiceRow>, String> {
    let backend = backend();
    match backend.kind.as_str() {
        "systemd" => list_systemd(backend.manageable),
        "launchd" => list_launchd(),
        _ => Err(backend.message),
    }
}

fn list_systemd(manageable: bool) -> Result<Vec<ServiceRow>, String> {
    let output = shell::run(
        "systemctl",
        &[
            "list-units",
            "--type=service",
            "--all",
            "--no-pager",
            "--no-legend",
            "--plain",
        ],
    )?;
    if !output.ok() {
        return Err(output.error_text());
    }

    // `unit load active sub description...`
    let mut rows: Vec<ServiceRow> = output
        .stdout
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let unit = parts.next()?;
            let load_state = parts.next()?.to_string();
            let active_state = parts.next()?.to_string();
            let sub_state = parts.next()?.to_string();
            let description = parts.collect::<Vec<_>>().join(" ");
            Some(ServiceRow {
                name: unit.trim_end_matches(".service").to_string(),
                description,
                load_state,
                active_state,
                sub_state,
                enabled: "unknown".into(),
                can_manage: manageable,
            })
        })
        .collect();

    // Second pass fills in enablement; one call for all units keeps it cheap.
    if let Ok(enabled_output) = shell::run(
        "systemctl",
        &[
            "list-unit-files",
            "--type=service",
            "--no-pager",
            "--no-legend",
            "--plain",
        ],
    ) {
        if enabled_output.ok() {
            let states: std::collections::HashMap<String, String> = enabled_output
                .stdout
                .lines()
                .filter_map(|line| {
                    let mut parts = line.split_whitespace();
                    let unit = parts.next()?.trim_end_matches(".service").to_string();
                    let state = parts.next()?.to_string();
                    Some((unit, state))
                })
                .collect();
            for row in &mut rows {
                if let Some(state) = states.get(&row.name) {
                    row.enabled = state.clone();
                }
            }
        }
    }

    rows.sort_by(|a, b| {
        active_rank(&a.active_state)
            .cmp(&active_rank(&b.active_state))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(rows)
}

fn active_rank(state: &str) -> u8 {
    match state {
        "failed" => 0,
        "activating" | "deactivating" => 1,
        "active" => 2,
        _ => 3,
    }
}

fn list_launchd() -> Result<Vec<ServiceRow>, String> {
    let output = shell::run("launchctl", &["list"])?;
    if !output.ok() {
        return Err(output.error_text());
    }

    // `PID Status Label`
    let rows = output
        .stdout
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?;
            let status = parts.next()?;
            let label = parts.next()?.to_string();
            let running = pid != "-";
            Some(ServiceRow {
                name: label,
                description: if status == "0" {
                    String::new()
                } else {
                    format!("last exit status {}", status)
                },
                load_state: "loaded".into(),
                active_state: if running { "active" } else { "inactive" }.into(),
                sub_state: if running { "running" } else { "dead" }.into(),
                enabled: "unknown".into(),
                can_manage: false,
            })
        })
        .collect();
    Ok(rows)
}

pub fn action(request: &ServiceActionRequest) -> Result<String, String> {
    let backend = backend();
    if backend.kind != "systemd" {
        return Err(backend.message);
    }
    if !backend.manageable {
        return Err("Service control requires root privileges or passwordless sudo".into());
    }

    let unit = sanitize_unit(&request.name)?;
    let verb = match request.action.as_str() {
        "start" | "stop" | "restart" | "reload" | "enable" | "disable" => request.action.as_str(),
        other => return Err(format!("Unsupported action: {}", other)),
    };

    let output = shell::run_privileged("systemctl", &[verb, &unit])?;
    if !output.ok() {
        return Err(output.error_text());
    }
    Ok(format!("{} {}", verb, unit))
}

pub fn status_text(name: &str) -> Result<String, String> {
    let backend = backend();
    if backend.kind != "systemd" {
        return Err(backend.message);
    }
    let unit = sanitize_unit(name)?;
    let output = shell::run(
        "systemctl",
        &["status", &unit, "--no-pager", "--lines=50"],
    )?;
    // `systemctl status` exits non-zero for inactive units, but its output is still what we want.
    let text = if output.stdout.trim().is_empty() {
        output.stderr
    } else {
        output.stdout
    };
    Ok(text)
}

pub fn logs(name: &str, lines: usize) -> Result<String, String> {
    if !shell::has_binary("journalctl") {
        return Err("journalctl is not available on this system".into());
    }
    let unit = sanitize_unit(name)?;
    let lines = lines.clamp(10, 1000).to_string();
    let output = shell::run(
        "journalctl",
        &["-u", &unit, "-n", &lines, "--no-pager", "--output=short-iso"],
    )?;
    if !output.ok() {
        return Err(output.error_text());
    }
    Ok(output.stdout)
}

/// Unit names reach systemctl as arguments, so reject anything that is not a plain unit name.
fn sanitize_unit(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Service name is required".into());
    }
    let valid = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '@' | ':' | '\\'));
    if !valid || trimmed.len() > 128 {
        return Err("Invalid service name".into());
    }
    if trimmed.contains("..") {
        return Err("Invalid service name".into());
    }
    if trimmed.ends_with(".service") {
        Ok(trimmed.to_string())
    } else {
        Ok(format!("{}.service", trimmed))
    }
}
