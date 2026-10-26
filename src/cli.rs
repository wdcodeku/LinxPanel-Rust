//! Command-line subcommands: recovery and setup tasks run on the server itself
//! (by the installer, `Lp`, or an operator locked out of the web UI).
//!
//! They edit the database directly; the running panel picks up entrance and
//! HTTPS changes on its next start, which `Lp` triggers.

use anyhow::{anyhow, bail, Result};

use crate::db;
use crate::services::panel_access;

pub const USAGE: &str = "\
Usage: linxpanel [COMMAND]

Commands:
  (none)                       Run the panel server
  reset-password [--user NAME] [--password-stdin]
                               Set a user's password and sign out its sessions.
                               Reads it from stdin with --password-stdin,
                               otherwise generates and prints one.
  entrance [show|off|random|set PATH]
                               Show or change the security entrance
  ssl [status|on|off] [--san NAME]... [--regenerate]
                               Show or change panel HTTPS (self-signed unless a
                               certificate was chosen in the panel)
  info                         Print scheme and entrance as key=value lines
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
        "info" => info().await?,
        other => bail!("Unknown command: {}\n\n{}", other, USAGE),
    }
    Ok(true)
}

/// Recovery path for a lost password, also used by the installer to replace the
/// well-known default with a random one.
async fn reset_password(args: &[String]) -> Result<()> {
    let mut username = db::DEFAULT_ADMIN_USER.to_string();
    let mut from_stdin = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--user" | "-u" => {
                username = iter
                    .next()
                    .ok_or_else(|| anyhow!("--user requires a value"))?
                    .clone();
            }
            "--password-stdin" => from_stdin = true,
            other => bail!("Unknown argument: {}\n\n{}", other, USAGE),
        }
    }
    if username.trim().is_empty() {
        bail!("Username must not be empty");
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

/// Machine-readable summary for the installer and `Lp`.
async fn info() -> Result<()> {
    let (pool, data_dir) = crate::open_database().await?;
    // Mirrors startup: a configured certificate that fails to load means plain HTTP.
    let (cert_path, key_path, _) = panel_access::ssl_paths(&pool, &data_dir).await;
    let tls = panel_access::ssl_enabled(&pool).await
        && panel_access::load_tls_config(&cert_path, &key_path).is_ok();
    let scheme = if tls { "https" } else { "http" };
    let entrance = panel_access::load_entrance(&pool).await.unwrap_or_default();
    pool.close().await;
    println!("version={}", env!("CARGO_PKG_VERSION"));
    println!("scheme={}", scheme);
    println!("entrance={}", entrance);
    Ok(())
}
