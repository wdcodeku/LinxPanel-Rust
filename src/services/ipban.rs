//! Kernel-level IP blocking with nftables.
//!
//! Banned addresses live in two nft sets with per-element timeouts, matched by
//! an input hook that runs before other filters. Their packets are dropped in
//! the kernel: no TCP handshake, no TLS, no nginx/PHP/panel work - the cheapest
//! possible answer to a flood. The table is separate from ufw/firewalld, which
//! keep working unchanged.

use std::net::IpAddr;
use std::time::Duration;

use serde::Serialize;

use super::shell;

pub const TABLE: &str = "linxpanel";
const TIMEOUT: Duration = Duration::from_secs(20);

const TABLE_DEFINITION: &str = "\
table inet linxpanel {
    set ban_v4 {
        type ipv4_addr
        flags timeout
    }
    set ban_v6 {
        type ipv6_addr
        flags timeout
    }
    chain input {
        type filter hook input priority -150; policy accept;
        ip saddr @ban_v4 drop
        ip6 saddr @ban_v6 drop
    }
}
";

pub fn available() -> bool {
    shell::has_binary("nft")
}

fn nft(args: &[&str], input: Option<&str>) -> Result<String, String> {
    let output = if shell::is_root() {
        shell::run_with_input("nft", args, input, TIMEOUT)?
    } else {
        let mut sudo_args = vec!["-n", "nft"];
        sudo_args.extend_from_slice(args);
        shell::run_with_input("sudo", &sudo_args, input, TIMEOUT)?
    };
    if output.ok() {
        Ok(output.stdout)
    } else {
        Err(output.error_text())
    }
}

/// Creates the table, sets and drop rules if they are missing (a reboot or a
/// `nft flush ruleset` removes them). Existing bans are left in place.
pub fn ensure_table() -> Result<(), String> {
    if !available() {
        return Err("nftables (nft) is not installed".into());
    }
    let present = nft(&["list", "chain", "inet", TABLE, "input"], None)
        .map(|text| text.contains("@ban_v4") && text.contains("@ban_v6"))
        .unwrap_or(false);
    if present {
        return Ok(());
    }
    // Recreate atomically: a half-built table could drop nothing.
    let _ = nft(&["delete", "table", "inet", TABLE], None);
    nft(&["-f", "-"], Some(TABLE_DEFINITION)).map(|_| ())
}

fn set_for(ip: &IpAddr) -> &'static str {
    if ip.is_ipv4() {
        "ban_v4"
    } else {
        "ban_v6"
    }
}

/// Drops all traffic from `ip`, for `seconds` or (None) until unbanned.
pub fn ban(ip: IpAddr, seconds: Option<u64>) -> Result<(), String> {
    let ip = ip.to_canonical();
    ensure_table()?;
    // Re-banning replaces the element so a new timeout takes effect.
    let _ = unban(ip);
    let element = match seconds {
        Some(secs) => format!("{{ {} timeout {}s }}", ip, secs.max(1)),
        None => format!("{{ {} }}", ip),
    };
    nft(
        &["add", "element", "inet", TABLE, set_for(&ip), &element],
        None,
    )
    .map(|_| ())
}

pub fn unban(ip: IpAddr) -> Result<(), String> {
    let ip = ip.to_canonical();
    let element = format!("{{ {} }}", ip);
    match nft(
        &["delete", "element", "inet", TABLE, set_for(&ip), &element],
        None,
    ) {
        Ok(_) => Ok(()),
        // Not present (expired, or never added): nothing to do.
        Err(err) if err.contains("No such file") || err.contains("does not exist") => Ok(()),
        Err(err) => Err(err),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct KernelBan {
    pub ip: String,
    /// Seconds left; None for permanent.
    pub expires_in: Option<u64>,
}

/// What the kernel is currently dropping.
pub fn list() -> Result<Vec<KernelBan>, String> {
    let mut bans = Vec::new();
    for set in ["ban_v4", "ban_v6"] {
        let text = nft(&["-j", "list", "set", "inet", TABLE, set], None)?;
        let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let elements = json["nftables"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|item| item["set"]["elem"].as_array())
            .flatten();
        for element in elements {
            let (ip, expires_in) = match element {
                serde_json::Value::String(ip) => (ip.clone(), None),
                other => (
                    other["elem"]["val"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    other["elem"]["expires"].as_u64(),
                ),
            };
            if !ip.is_empty() {
                bans.push(KernelBan { ip, expires_in });
            }
        }
    }
    Ok(bans)
}
