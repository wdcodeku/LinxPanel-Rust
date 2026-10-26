//! Command-line subcommands: recovery and setup tasks run on the server itself
//! (by the installer, `Lp`, or an operator locked out of the web UI).
//!
//! They edit the database directly; the running panel picks the changes up on
//! its next start, which `Lp` triggers.

use anyhow::{anyhow, bail, Result};

use crate::db;
use crate::services::panel_access;

pub const USAGE: &str = "\
Usage: linxpanel [COMMAND]

Commands:
  (none)                       Run the panel server
  reset-password [--user NAME] [--password-stdin]
                               Set a user's password (default: the panel's
                               account) and sign out its sessions.
                               Reads it from stdin with --password-stdin,
                               otherwise generates and prints one.
  entrance [show|off|random|set PATH]
                               Show or change the security entrance
  ssl [status|on|off] [--san NAME]... [--regenerate]
                               Show or change panel HTTPS (self-signed unless a
                               certificate was chosen in the panel)
  port [show|PORT|default]     Show or change the panel port (default: use PORT)
  allow-ip [show|off|IP,CIDR...]
                               Show, set or clear the allowed client IPs
  domain [show|off|DOMAIN]     Show, set or clear the bound domain
  mfa [status|off] [--user NAME]
                               Show or turn off two-factor login (default:
                               the panel's account)
  security                     Show all panel access settings
  info                         Print access settings as key=value lines
  version, --version, -V       Print the version
  help, --help, -h             Print this help

Environment: PORT (default 7200), BIND (default 0.0.0.0), DATABASE_URL, JWT_SECRET, RUST_LOG";

/// Runs a subcommand, or returns Ok(false) when the server should start instead.
pub async fn run(args: &[String]) -> Result<bool> {
    let Some(command) = args.first() else {
        return Ok(false);
    };
    let rest = &args[1..];
    match command.as_str() {
        "version" | "--version" | "-V" => println!("linxpanel {}", env!("CARGO_PKG_VERSION")),
        "help" | "--help" | "-h" => println!("{}", USAGE),
        "reset-password" => reset_password(rest).await?,
        "entrance" => entrance(rest).await?,
        "ssl" => ssl(rest).await?,
        "port" => port(rest).await?,
        "allow-ip" => allow_ip(rest).await?,
        "domain" => domain(rest).await?,
        "mfa" => mfa(rest).await?,
        "security" => security().await?,
        "info" => info().await?,
        other => bail!("Unknown command: {}\n\n{}", other, USAGE),
    }
    Ok(true)
}

/// Recovery path for a lost password, also used by the installer to replace the
/// well-known default with a random one.
async fn reset_password(args: &[String]) -> Result<()> {
    let mut username = None;
    let mut from_stdin = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--user" | "-u" => {
                username = Some(
                    iter.next()
                        .ok_or_else(|| anyhow!("--user requires a value"))?
                        .clone(),
                );
            }
            "--password-stdin" => from_stdin = true,
            other => bail!("Unknown argument: {}\n\n{}", other, USAGE),
        }
    }

    let password = if from_stdin {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line)?;
        line.trim_end_matches(['\r', '\n']).to_string()
    } else {
        db::generate_password(16)
    };
    db::validate_password(&password).map_err(|e| anyhow!(e))?;

    let (pool, _) = crate::open_database().await?;
    // Default to the panel's account, which may no longer be called "admin".
    let username = match username {
        Some(name) => name,
        None => db::primary_username(&pool).await?,
    };
    if username.trim().is_empty() {
        bail!("Username must not be empty");
    }
    db::set_user_password(&pool, &username, &password).await?;
    pool.close().await;

    println!("Username: {}", username);
    if !from_stdin {
        println!("Password: {}", password);
    }
    println!("Password updated. All existing sessions of this user were signed out.");
    Ok(())
}

async fn entrance(args: &[String]) -> Result<()> {
    let (pool, _) = crate::open_database().await?;
    let action = args.first().map(String::as_str).unwrap_or("show");
    let changed = match action {
        "show" | "status" => None,
        "off" | "disable" => Some(None),
        "random" | "new" => Some(Some(panel_access::random_entrance())),
        "set" => {
            let value = args
                .get(1)
                .ok_or_else(|| anyhow!("entrance set requires a path"))?;
            Some(Some(normalize_or_fail(value)?))
        }
        // `linxpanel entrance /path` as a shorthand for `set`.
        value if value.starts_with('/') => Some(Some(normalize_or_fail(value)?)),
        other => bail!("Unknown entrance action: {}\n\n{}", other, USAGE),
    };
    if let Some(entrance) = &changed {
        panel_access::save_entrance(&pool, entrance.as_deref()).await?;
    }
    let current = panel_access::load_entrance(&pool).await;
    pool.close().await;

    match current {
        Some(entrance) => println!("Entrance: {}", entrance),
        None => println!("Entrance: disabled"),
    }
    if changed.is_some() {
        println!("Restart the panel to apply: Lp restart");
    }
    Ok(())
}

fn normalize_or_fail(value: &str) -> Result<String> {
    panel_access::normalize_entrance(value)
        .map_err(|e| anyhow!(e))?
        .ok_or_else(|| anyhow!("Entrance must not be empty; use `entrance off` to disable it"))
}

async fn ssl(args: &[String]) -> Result<()> {
    let (pool, data_dir) = crate::open_database().await?;
    let action = args.first().map(String::as_str).unwrap_or("status");
    let mut names = Vec::new();
    let mut regenerate = false;
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--san" => names.push(
                iter.next()
                    .ok_or_else(|| anyhow!("--san requires a value"))?
                    .clone(),
            ),
            "--regenerate" => regenerate = true,
            other => bail!("Unknown argument: {}\n\n{}", other, USAGE),
        }
    }

    match action {
        "status" => {}
        "on" | "enable" => {
            let (cert_path, key_path, self_signed) =
                panel_access::ssl_paths(&pool, &data_dir).await;
            let missing = !cert_path.exists() || !key_path.exists();
            if self_signed && (missing || regenerate || !names.is_empty()) {
                panel_access::generate_self_signed(&data_dir, &names)?;
                println!(
                    "Generated a self-signed certificate: {}",
                    cert_path.display()
                );
            }
            panel_access::load_tls_config(&cert_path, &key_path)?;
            db::set_setting(&pool, panel_access::SETTING_PANEL_SSL, "on").await?;
        }
        "off" | "disable" => {
            db::set_setting(&pool, panel_access::SETTING_PANEL_SSL, "off").await?;
        }
        other => bail!("Unknown ssl action: {}\n\n{}", other, USAGE),
    }

    let enabled = panel_access::ssl_enabled(&pool).await;
    let (cert_path, _, self_signed) = panel_access::ssl_paths(&pool, &data_dir).await;
    pool.close().await;
    println!("HTTPS: {}", if enabled { "on" } else { "off" });
    if enabled {
        println!(
            "Certificate: {}{}",
            cert_path.display(),
            if self_signed { " (self-signed)" } else { "" }
        );
    }
    if action != "status" {
        println!("Restart the panel to apply: Lp restart");
    }
    Ok(())
}

const RESTART_HINT: &str = "Restart the panel to apply: Lp restart";

/// The first argument as the action, with `default` when absent.
fn action<'a>(args: &'a [String], default: &'a str) -> &'a str {
    args.first().map(String::as_str).unwrap_or(default)
}

async fn port(args: &[String]) -> Result<()> {
    let (pool, _) = crate::open_database().await?;
    match action(args, "show") {
        "show" | "status" => {}
        "default" | "reset" => {
            db::set_setting(&pool, panel_access::SETTING_LISTEN_PORT, "").await?;
            println!("{}", RESTART_HINT);
        }
        value => {
            let port = value
                .parse::<i64>()
                .map_err(|_| anyhow!("Not a port number: {}", value))
                .and_then(|p| panel_access::validate_port(p).map_err(|e| anyhow!(e)))?;
            if std::net::TcpListener::bind(("0.0.0.0", port)).is_err() {
                println!(
                    "Note: port {} is in use right now (fine if the panel itself uses it)",
                    port
                );
            }
            db::set_setting(&pool, panel_access::SETTING_LISTEN_PORT, &port.to_string()).await?;
            println!("{}", RESTART_HINT);
        }
    }
    println!("Port: {}", panel_access::effective_port(&pool).await);
    pool.close().await;
    Ok(())
}

async fn allow_ip(args: &[String]) -> Result<()> {
    let (pool, _) = crate::open_database().await?;
    match action(args, "show") {
        "show" | "status" => {}
        "off" | "clear" => {
            db::set_setting(&pool, panel_access::SETTING_ALLOWED_IPS, "").await?;
            println!("{}", RESTART_HINT);
        }
        _ => {
            let list = panel_access::parse_ip_list(&args.join(",")).map_err(|e| anyhow!(e))?;
            let value = panel_access::format_ip_list(&list);
            db::set_setting(&pool, panel_access::SETTING_ALLOWED_IPS, &value).await?;
            println!("{}", RESTART_HINT);
        }
    }
    let policy = panel_access::AccessPolicy::load(&pool).await;
    pool.close().await;
    if policy.allowed_ips.is_empty() {
        println!("Allowed IPs: any");
    } else {
        println!(
            "Allowed IPs: {}",
            panel_access::format_ip_list(&policy.allowed_ips)
        );
    }
    Ok(())
}

async fn domain(args: &[String]) -> Result<()> {
    let (pool, _) = crate::open_database().await?;
    match action(args, "show") {
        "show" | "status" => {}
        "off" | "clear" => {
            db::set_setting(&pool, panel_access::SETTING_BIND_DOMAIN, "").await?;
            println!("{}", RESTART_HINT);
        }
        value => {
            let domain = panel_access::normalize_domain(value)
                .map_err(|e| anyhow!(e))?
                .unwrap_or_default();
            db::set_setting(&pool, panel_access::SETTING_BIND_DOMAIN, &domain).await?;
            println!("{}", RESTART_HINT);
        }
    }
    let policy = panel_access::AccessPolicy::load(&pool).await;
    pool.close().await;
    println!(
        "Bound domain: {}",
        policy.domain.as_deref().unwrap_or("none")
    );
    Ok(())
}

async fn mfa(args: &[String]) -> Result<()> {
    let mut username = None;
    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--user" | "-u" => {
                username = Some(
                    iter.next()
                        .ok_or_else(|| anyhow!("--user requires a value"))?
                        .clone(),
                );
            }
            other => bail!("Unknown argument: {}\n\n{}", other, USAGE),
        }
    }
    let (pool, _) = crate::open_database().await?;
    let username = match username {
        Some(name) => name,
        None => db::primary_username(&pool).await?,
    };
    match action(args, "status") {
        "status" | "show" => {}
        "off" | "disable" => {
            if !db::disable_totp(&pool, &username).await? {
                bail!("No such user: {}", username);
            }
            println!("Two-factor login turned off for {}", username);
        }
        other => bail!("Unknown mfa action: {}\n\n{}", other, USAGE),
    }
    let enabled: Option<(Option<String>,)> =
        sqlx::query_as("SELECT totp_secret FROM users WHERE username = ?")
            .bind(&username)
            .fetch_optional(&pool)
            .await?;
    pool.close().await;
    let on = enabled
        .and_then(|(secret,)| secret)
        .is_some_and(|s| !s.is_empty());
    println!(
        "Two-factor login for {}: {}",
        username,
        if on { "on" } else { "off" }
    );
    Ok(())
}

/// Human-readable overview, like BaoTa's `bt default`.
async fn security() -> Result<()> {
    let (pool, data_dir) = crate::open_database().await?;
    let policy = panel_access::AccessPolicy::load(&pool).await;
    let (cert_path, key_path, self_signed) = panel_access::ssl_paths(&pool, &data_dir).await;
    let ssl = panel_access::ssl_enabled(&pool).await;
    let ssl_ok = !ssl || panel_access::load_tls_config(&cert_path, &key_path).is_ok();
    let users: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT username, totp_secret FROM users ORDER BY id")
            .fetch_all(&pool)
            .await?;
    println!(
        "Port:            {}",
        panel_access::effective_port(&pool).await
    );
    pool.close().await;

    println!(
        "Entrance:        {}",
        policy.entrance.as_deref().unwrap_or("disabled")
    );
    println!(
        "HTTPS:           {}{}",
        if ssl { "on" } else { "off" },
        match (ssl, ssl_ok, self_signed) {
            (true, false, _) => " (certificate cannot be loaded; serving HTTP)",
            (true, true, true) => " (self-signed)",
            _ => "",
        }
    );
    println!(
        "Allowed IPs:     {}",
        if policy.allowed_ips.is_empty() {
            "any".to_string()
        } else {
            panel_access::format_ip_list(&policy.allowed_ips)
        }
    );
    println!(
        "Bound domain:    {}",
        policy.domain.as_deref().unwrap_or("none")
    );
    println!("Denied visitors: {}", policy.deny.as_str());
    for (username, secret) in users {
        let on = secret.is_some_and(|s| !s.is_empty());
        println!(
            "User:            {} (two-factor {})",
            username,
            if on { "on" } else { "off" }
        );
    }
    Ok(())
}

/// Machine-readable summary for the installer and `Lp`.
async fn info() -> Result<()> {
    let (pool, data_dir) = crate::open_database().await?;
    // Mirrors startup: a configured certificate that fails to load means plain HTTP.
    let (cert_path, key_path, _) = panel_access::ssl_paths(&pool, &data_dir).await;
    let tls = panel_access::ssl_enabled(&pool).await
        && panel_access::load_tls_config(&cert_path, &key_path).is_ok();
    let scheme = if tls { "https" } else { "http" };
    let policy = panel_access::AccessPolicy::load(&pool).await;
    // Only the saved value: empty means the PORT environment variable decides.
    let listen_port = panel_access::load_listen_port(&pool).await;
    let admin_user = db::primary_username(&pool).await?;
    pool.close().await;
    println!("version={}", env!("CARGO_PKG_VERSION"));
    println!("scheme={}", scheme);
    println!("entrance={}", policy.entrance.unwrap_or_default());
    println!(
        "listen_port={}",
        listen_port.map(|p| p.to_string()).unwrap_or_default()
    );
    println!("domain={}", policy.domain.unwrap_or_default());
    println!("admin_user={}", admin_user);
    Ok(())
}
