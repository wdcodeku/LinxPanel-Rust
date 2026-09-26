//! Firewall (ufw / firewalld) and listening-socket inspection.

use serde::{Deserialize, Serialize};

use super::shell;

#[derive(Debug, Clone, Serialize)]
pub struct FirewallStatus {
    /// ufw | firewalld | none
    pub backend: String,
    pub available: bool,
    pub manageable: bool,
    pub enabled: bool,
    pub default_incoming: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FirewallRule {
    pub id: String,
    pub port: String,
    pub protocol: String,
    pub source: String,
    pub action: String,
    pub raw: String,
}

#[derive(Debug, Deserialize)]
pub struct FirewallRuleRequest {
    pub port: String,
    #[serde(default = "default_protocol")]
    pub protocol: String,
    #[serde(default)]
    pub source: String,
    #[serde(default = "default_action")]
    pub action: String,
}

fn default_protocol() -> String {
    "tcp".into()
}
fn default_action() -> String {
    "allow".into()
}

pub fn status() -> FirewallStatus {
    if !cfg!(target_os = "linux") {
        return FirewallStatus {
            backend: "none".into(),
            available: false,
            manageable: false,
            enabled: false,
            default_incoming: String::new(),
            message: "Firewall management is supported on Linux only".into(),
        };
    }

    let manageable = shell::is_root()
        || shell::run("sudo", &["-n", "true"])
            .map(|o| o.ok())
            .unwrap_or(false);

    if shell::has_binary("ufw") {
        let output = shell::run_privileged("ufw", &["status", "verbose"]).ok();
        let text = output.map(|o| o.stdout).unwrap_or_default();
        let enabled = text.contains("Status: active");
        let default_incoming = text
            .lines()
            .find(|line| line.trim_start().starts_with("Default:"))
            .and_then(|line| line.split_whitespace().nth(1))
            .unwrap_or("")
            .trim_end_matches(',')
            .to_string();
        return FirewallStatus {
            backend: "ufw".into(),
            available: true,
            manageable,
            enabled,
            default_incoming,
            message: if manageable {
                String::new()
            } else {
                "Read-only: firewall changes need root or passwordless sudo".into()
            },
        };
    }

    if shell::has_binary("firewall-cmd") {
        let enabled = shell::run_privileged("firewall-cmd", &["--state"])
            .map(|o| o.stdout.trim() == "running")
            .unwrap_or(false);
        return FirewallStatus {
            backend: "firewalld".into(),
            available: true,
            manageable,
            enabled,
            default_incoming: String::new(),
            message: if manageable {
                String::new()
            } else {
                "Read-only: firewall changes need root or passwordless sudo".into()
            },
        };
    }

    FirewallStatus {
        backend: "none".into(),
        available: false,
        manageable: false,
        enabled: false,
        default_incoming: String::new(),
        message: "No supported firewall found (ufw or firewalld)".into(),
    }
}

pub fn list_rules() -> Result<Vec<FirewallRule>, String> {
    let status = status();
    if !status.available {
        return Err(status.message);
    }
    match status.backend.as_str() {
        "ufw" => list_ufw_rules(),
        "firewalld" => list_firewalld_rules(),
        _ => Err(status.message),
    }
}

fn list_ufw_rules() -> Result<Vec<FirewallRule>, String> {
    let output = shell::run_privileged("ufw", &["status", "numbered"])?;
    if !output.ok() {
        return Err(output.error_text());
    }

    // `[ 1] 22/tcp                     ALLOW IN    Anywhere`
    let rules = output
        .stdout
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let rest = trimmed.strip_prefix('[')?;
            let (index, rest) = rest.split_once(']')?;
            let fields: Vec<&str> = rest.split_whitespace().collect();
            if fields.len() < 2 {
                return None;
            }
            let target = fields[0];
            let (port, protocol) = match target.split_once('/') {
                Some((port, proto)) => (port.to_string(), proto.to_string()),
                None => (target.to_string(), "any".to_string()),
            };
            let action = fields[1..]
                .iter()
                .take_while(|f| f.chars().all(|c| c.is_ascii_uppercase()))
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");
            let source = fields[1..]
                .iter()
                .skip_while(|f| f.chars().all(|c| c.is_ascii_uppercase()))
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");
            Some(FirewallRule {
                id: index.trim().to_string(),
                port,
                protocol,
                source: if source.is_empty() { "Anywhere".into() } else { source },
                action,
                raw: trimmed.to_string(),
            })
        })
        .collect();
    Ok(rules)
}

fn list_firewalld_rules() -> Result<Vec<FirewallRule>, String> {
    let output = shell::run_privileged("firewall-cmd", &["--list-ports"])?;
    if !output.ok() {
        return Err(output.error_text());
    }
    let rules = output
        .stdout
        .split_whitespace()
        .map(|entry| {
            let (port, protocol) = entry.split_once('/').unwrap_or((entry, "tcp"));
            FirewallRule {
                id: entry.to_string(),
                port: port.to_string(),
                protocol: protocol.to_string(),
                source: "Anywhere".into(),
                action: "ALLOW".into(),
                raw: entry.to_string(),
            }
        })
        .collect();
    Ok(rules)
}

pub fn add_rule(request: &FirewallRuleRequest) -> Result<String, String> {
    let status = status();
    if !status.available {
        return Err(status.message);
    }
    if !status.manageable {
        return Err("Firewall changes require root privileges or passwordless sudo".into());
    }

    let port = validate_port(&request.port)?;
    let protocol = validate_protocol(&request.protocol)?;
    let action = match request.action.as_str() {
        "allow" | "deny" => request.action.as_str(),
        other => return Err(format!("Unsupported action: {}", other)),
    };

    match status.backend.as_str() {
        "ufw" => {
            let target = format!("{}/{}", port, protocol);
            let output = if request.source.trim().is_empty() {
                shell::run_privileged("ufw", &[action, &target])?
            } else {
                let source = validate_source(&request.source)?;
                shell::run_privileged(
                    "ufw",
                    &[action, "from", &source, "to", "any", "port", &port, "proto", &protocol],
                )?
            };
            if !output.ok() {
                return Err(output.error_text());
            }
            Ok(output.stdout.trim().to_string())
        }
        "firewalld" => {
            if action != "allow" {
                return Err("firewalld backend supports allow rules only".into());
            }
            let spec = format!("--add-port={}/{}", port, protocol);
            let output = shell::run_privileged("firewall-cmd", &["--permanent", &spec])?;
            if !output.ok() {
                return Err(output.error_text());
            }
            let _ = shell::run_privileged("firewall-cmd", &["--reload"]);
            Ok(output.stdout.trim().to_string())
        }
        _ => Err(status.message),
    }
}

pub fn delete_rule(id: &str) -> Result<String, String> {
    let status = status();
    if !status.available {
        return Err(status.message);
    }
    if !status.manageable {
        return Err("Firewall changes require root privileges or passwordless sudo".into());
    }

    match status.backend.as_str() {
        "ufw" => {
            if !id.chars().all(|c| c.is_ascii_digit()) || id.is_empty() {
                return Err("Invalid rule number".into());
            }
            // ufw prompts for confirmation; `--force` answers it.
            let output = shell::run_privileged("ufw", &["--force", "delete", id])?;
            if !output.ok() {
                return Err(output.error_text());
            }
            Ok(output.stdout.trim().to_string())
        }
        "firewalld" => {
            let (port, protocol) = id.split_once('/').unwrap_or((id, "tcp"));
            let port = validate_port(port)?;
            let protocol = validate_protocol(protocol)?;
            let spec = format!("--remove-port={}/{}", port, protocol);
            let output = shell::run_privileged("firewall-cmd", &["--permanent", &spec])?;
            if !output.ok() {
                return Err(output.error_text());
            }
            let _ = shell::run_privileged("firewall-cmd", &["--reload"]);
            Ok(output.stdout.trim().to_string())
        }
        _ => Err(status.message),
    }
}

pub fn set_enabled(enabled: bool) -> Result<String, String> {
    let status = status();
    if !status.available {
        return Err(status.message);
    }
    if !status.manageable {
        return Err("Firewall changes require root privileges or passwordless sudo".into());
    }
    match status.backend.as_str() {
        "ufw" => {
            let args: &[&str] = if enabled {
                &["--force", "enable"]
            } else {
                &["disable"]
            };
            let output = shell::run_privileged("ufw", args)?;
            if !output.ok() {
                return Err(output.error_text());
            }
            Ok(output.stdout.trim().to_string())
        }
        "firewalld" => {
            let verb = if enabled { "start" } else { "stop" };
            let output = shell::run_privileged("systemctl", &[verb, "firewalld"])?;
            if !output.ok() {
                return Err(output.error_text());
            }
            Ok(format!("firewalld {}", verb))
        }
        _ => Err(status.message),
    }
}

fn validate_port(port: &str) -> Result<String, String> {
    let trimmed = port.trim();
    if trimmed.is_empty() {
        return Err("Port is required".into());
    }
    // Accept a single port or an inclusive range.
    let parts: Vec<&str> = trimmed.split(':').collect();
    if parts.len() > 2 {
        return Err("Invalid port".into());
    }
    for part in &parts {
        let value: u32 = part
            .parse()
            .map_err(|_| format!("Invalid port: {}", trimmed))?;
        if value == 0 || value > 65535 {
            return Err(format!("Port out of range: {}", trimmed));
        }
    }
    Ok(trimmed.to_string())
}

fn validate_protocol(protocol: &str) -> Result<String, String> {
    match protocol.trim().to_lowercase().as_str() {
        "tcp" => Ok("tcp".into()),
        "udp" => Ok("udp".into()),
        other => Err(format!("Unsupported protocol: {}", other)),
    }
}

fn validate_source(source: &str) -> Result<String, String> {
    let trimmed = source.trim();
    let valid = !trimmed.is_empty()
        && trimmed.len() <= 64
        && trimmed
            .chars()
            .all(|c| c.is_ascii_hexdigit() || matches!(c, '.' | ':' | '/'));
    if valid {
        Ok(trimmed.to_string())
    } else {
        Err("Invalid source address".into())
    }
}

// ===== Listening sockets =====

#[derive(Debug, Clone, Serialize)]
pub struct ListeningPort {
    pub protocol: String,
    pub address: String,
    pub port: u32,
    pub pid: Option<u32>,
    pub process: String,
}

pub fn listening_ports() -> Result<Vec<ListeningPort>, String> {
    if shell::has_binary("ss") {
        return parse_ss();
    }
    if shell::has_binary("lsof") {
        return parse_lsof();
    }
    if shell::has_binary("netstat") {
        return parse_netstat();
    }
    Err("No supported tool found (ss, lsof or netstat)".into())
}

fn parse_ss() -> Result<Vec<ListeningPort>, String> {
    let output = shell::run_best_effort("ss", &["-tulnp"])?;
    if !output.ok() && output.stdout.trim().is_empty() {
        return Err(output.error_text());
    }

    // `tcp LISTEN 0 4096 0.0.0.0:22 0.0.0.0:* users:(("sshd",pid=812,fd=3))`
    let rows = output
        .stdout
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 5 {
                return None;
            }
            let protocol = fields[0].to_string();
            let local = fields[4];
            let (address, port) = split_socket_addr(local)?;
            let (pid, process) = fields
                .iter()
                .find(|f| f.starts_with("users:("))
                .map(|f| parse_ss_users(f))
                .unwrap_or((None, String::new()));
            Some(ListeningPort {
                protocol,
                address,
                port,
                pid,
                process,
            })
        })
        .collect();
    Ok(dedupe_ports(rows))
}

fn parse_ss_users(field: &str) -> (Option<u32>, String) {
    // users:(("sshd",pid=812,fd=3))
    let name = field
        .split('"')
        .nth(1)
        .unwrap_or_default()
        .to_string();
    let pid = field
        .split("pid=")
        .nth(1)
        .and_then(|rest| {
            rest.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .ok()
        });
    (pid, name)
}

fn parse_lsof() -> Result<Vec<ListeningPort>, String> {
    let output = shell::run_best_effort("lsof", &["-nP", "-iTCP", "-sTCP:LISTEN"])?;
    if !output.ok() && output.stdout.trim().is_empty() {
        return Err(output.error_text());
    }

    // `sshd 812 root 3u IPv4 0x... 0t0 TCP *:22 (LISTEN)`
    let rows = output
        .stdout
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 9 {
                return None;
            }
            let process = fields[0].to_string();
            let pid = fields[1].parse().ok();
            let (address, port) = split_socket_addr(fields[8])?;
            Some(ListeningPort {
                protocol: "tcp".into(),
                address,
                port,
                pid,
                process,
            })
        })
        .collect();
    Ok(dedupe_ports(rows))
}

fn parse_netstat() -> Result<Vec<ListeningPort>, String> {
    let args: &[&str] = if cfg!(windows) {
        &["-ano"]
    } else {
        &["-tulnp"]
    };
    let output = shell::run_best_effort("netstat", args)?;
    if !output.ok() && output.stdout.trim().is_empty() {
        return Err(output.error_text());
    }

    let rows = output
        .stdout
        .lines()
        .filter(|line| line.to_uppercase().contains("LISTEN") || line.contains("udp"))
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 4 {
                return None;
            }
            let protocol = fields[0].to_lowercase();
            let (address, port) = split_socket_addr(fields[3])?;
            let pid = fields
                .last()
                .and_then(|f| f.split('/').next())
                .and_then(|f| f.parse().ok());
            let process = fields
                .last()
                .and_then(|f| f.split('/').nth(1))
                .unwrap_or_default()
                .to_string();
            Some(ListeningPort {
                protocol,
                address,
                port,
                pid,
                process,
            })
        })
        .collect();
    Ok(dedupe_ports(rows))
}

/// Splits `0.0.0.0:22`, `[::]:80`, `*:443` into (address, port).
fn split_socket_addr(value: &str) -> Option<(String, u32)> {
    let (address, port) = value.rsplit_once(':')?;
    let port: u32 = port.parse().ok()?;
    let address = address.trim_matches(|c| c == '[' || c == ']');
    Some((
        if address.is_empty() || address == "*" {
            "0.0.0.0".to_string()
        } else {
            address.to_string()
        },
        port,
    ))
}

fn dedupe_ports(mut rows: Vec<ListeningPort>) -> Vec<ListeningPort> {
    rows.sort_by(|a, b| {
        a.port
            .cmp(&b.port)
            .then_with(|| a.protocol.cmp(&b.protocol))
            .then_with(|| a.address.cmp(&b.address))
    });
    rows.dedup_by(|a, b| a.port == b.port && a.protocol == b.protocol && a.address == b.address);
    rows
}
