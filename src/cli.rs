//! Command-line subcommands: recovery and setup tasks run on the server itself
//! (by the installer, `Lp`, or an operator locked out of the web UI).
//!
//! They edit the database directly; the running panel picks the changes up on
//! its next start, which `Lp` triggers.

use anyhow::{anyhow, bail, Result};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::db;
use crate::services::{defense, ipban, panel_access};

/// The panel's language, shared by the web UI, `Lp` and these commands.
pub const SETTING_LANGUAGE: &str = "panel_language";

static ZH: AtomicBool = AtomicBool::new(false);

pub fn is_zh() -> bool {
    ZH.load(Ordering::Relaxed)
}

/// `tr!("中文 {}", "English {}", args...)`: the message in the panel's language.
macro_rules! tr {
    ($zh:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if crate::cli::is_zh() { format!($zh $(, $arg)*) } else { format!($en $(, $arg)*) }
    };
}

/// "zh" or "en" from any spelling (zh-CN, chinese, en_US, ...).
pub fn normalize_language(value: &str) -> Option<&'static str> {
    let value = value.trim().to_ascii_lowercase();
    if value.starts_with("zh") || value == "cn" || value == "chinese" || value == "中文" {
        Some("zh")
    } else if value.starts_with("en") || value == "english" {
        Some("en")
    } else {
        None
    }
}

/// LINXPANEL_LANG (what `Lp` passes) wins; otherwise the panel's setting.
async fn load_language(command: &str) {
    if let Some(lang) = std::env::var("LINXPANEL_LANG")
        .ok()
        .as_deref()
        .and_then(normalize_language)
    {
        ZH.store(lang == "zh", Ordering::Relaxed);
        return;
    }
    // `info` and `version` are read by scripts: no database work for them.
    if matches!(command, "info" | "version" | "--version" | "-V") {
        return;
    }
    if let Ok((pool, _)) = crate::open_database().await {
        let saved = db::get_setting(&pool, SETTING_LANGUAGE)
            .await
            .ok()
            .flatten()
            .unwrap_or_default();
        pool.close().await;
        ZH.store(normalize_language(&saved) == Some("zh"), Ordering::Relaxed);
    }
}

pub fn error_prefix() -> &'static str {
    if is_zh() {
        "错误："
    } else {
        "Error: "
    }
}

fn usage() -> &'static str {
    if is_zh() {
        USAGE_ZH
    } else {
        USAGE
    }
}

pub const USAGE_ZH: &str = "\
用法：linxpanel [命令]

命令：
  （无）                       运行面板服务
  reset-password [--user 用户名] [--password-stdin]
                               设置用户密码（默认为面板账号）并让其所有会话失效；
                               带 --password-stdin 时从标准输入读取，否则随机生成并显示
  entrance [show|off|random|set 路径]
                               查看或修改安全入口
  ssl [status|on|off] [--san 名称]... [--regenerate]
                               查看或修改面板 HTTPS（未在面板中选择证书时使用自签名证书）
  port [show|端口|default]     查看或修改面板端口（default：使用 PORT 环境变量）
  allow-ip [show|off|IP,CIDR...]
                               查看、设置或清除授权访问 IP
  domain [show|off|域名]       查看、设置或清除绑定域名
  mfa [status|off] [--user 用户名]
                               查看或关闭两步验证（默认为面板账号）
  security                     查看全部面板访问设置
  lang [show|zh|en]            查看或切换面板和命令行的语言
  ban <IP> [分钟]              在内核中丢弃某地址的全部流量（0 为永久；默认按防御设置）
  unban <IP|all>               解除封禁（all：全部解除，例如误封了自己）
  bans                         列出被封禁的地址
  defense [status|on|off]      查看或开关自动封禁
  harden [status|on|off]       内核网络加固（SYN Cookie、连接队列、防伪造），写入 /etc/sysctl.d
  backup site <ID> [--keep N]  打包网站目录（计划备份使用）
  backup db <引擎> <库名> [--keep N]
                               导出数据库（mysql | postgresql）
  info                         以 key=value 格式输出访问设置
  version, --version, -V       显示版本
  help, --help, -h             显示本帮助

环境变量：PORT（默认 7200）、BIND（默认 0.0.0.0）、DATABASE_URL、JWT_SECRET、RUST_LOG、LINXPANEL_LANG";

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
  lang [show|zh|en]            Show or switch the language of the panel and CLI
  ban <IP> [MINUTES]           Drop all traffic from an address in the kernel
                               (0 = permanent; default: the defense setting)
  unban <IP|all>               Lift a ban (all: every ban, e.g. after a lockout)
  bans                         List banned addresses
  defense [status|on|off]      Show or switch automatic banning
  harden [status|on|off]       Kernel network hardening (SYN cookies, backlog,
                               anti-spoofing) via /etc/sysctl.d
  backup site <ID> [--keep N]  Archive a website's directory (used by scheduled backups)
  backup db <ENGINE> <NAME> [--keep N]
                               Dump a database (mysql | postgresql)
  info                         Print access settings as key=value lines
  version, --version, -V       Print the version
  help, --help, -h             Print this help

Environment: PORT (default 7200), BIND (default 0.0.0.0), DATABASE_URL, JWT_SECRET, RUST_LOG, LINXPANEL_LANG";

/// Runs a subcommand, or returns Ok(false) when the server should start instead.
pub async fn run(args: &[String]) -> Result<bool> {
    let Some(command) = args.first() else {
        return Ok(false);
    };
    let rest = &args[1..];
    load_language(command).await;
    match command.as_str() {
        "version" | "--version" | "-V" => println!("linxpanel {}", env!("CARGO_PKG_VERSION")),
        "help" | "--help" | "-h" => println!("{}", usage()),
        "lang" | "language" => lang(rest).await?,
        "reset-password" => reset_password(rest).await?,
        "entrance" => entrance(rest).await?,
        "ssl" => ssl(rest).await?,
        "port" => port(rest).await?,
        "allow-ip" => allow_ip(rest).await?,
        "domain" => domain(rest).await?,
        "mfa" => mfa(rest).await?,
        "security" => security().await?,
        "ban" => ban(rest).await?,
        "unban" => unban(rest).await?,
        "bans" => bans().await?,
        "defense" => defense_cmd(rest).await?,
        "harden" => harden(rest)?,
        "backup" => backup(rest).await?,
        "info" => info().await?,
        other => bail!(tr!(
            "未知命令：{}\n\n{}",
            "Unknown command: {}\n\n{}",
            other,
            usage()
        )),
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
                        .ok_or_else(|| {
                            anyhow!(tr!("--user 需要一个值", "--user requires a value"))
                        })?
                        .clone(),
                );
            }
            "--password-stdin" => from_stdin = true,
            other => bail!(tr!(
                "未知参数：{}\n\n{}",
                "Unknown argument: {}\n\n{}",
                other,
                usage()
            )),
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
        bail!(tr!("用户名不能为空", "Username must not be empty"));
    }
    db::set_user_password(&pool, &username, &password).await?;
    pool.close().await;

    println!("{}", tr!("用户名：{}", "Username: {}", username));
    if !from_stdin {
        println!("{}", tr!("密码：  {}", "Password: {}", password));
    }
    println!(
        "{}",
        tr!(
            "密码已更新，该用户之前的所有登录会话已失效。",
            "Password updated. All existing sessions of this user were signed out."
        )
    );
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
            let value = args.get(1).ok_or_else(|| {
                anyhow!(tr!(
                    "entrance set 需要一个路径",
                    "entrance set requires a path"
                ))
            })?;
            Some(Some(normalize_or_fail(value)?))
        }
        // `linxpanel entrance /path` as a shorthand for `set`.
        value if value.starts_with('/') => Some(Some(normalize_or_fail(value)?)),
        other => bail!(tr!(
            "未知的入口操作：{}\n\n{}",
            "Unknown entrance action: {}\n\n{}",
            other,
            usage()
        )),
    };
    if let Some(entrance) = &changed {
        panel_access::save_entrance(&pool, entrance.as_deref()).await?;
    }
    let current = panel_access::load_entrance(&pool).await;
    pool.close().await;

    match current {
        Some(entrance) => println!("{}", tr!("安全入口：{}", "Entrance: {}", entrance)),
        None => println!("{}", tr!("安全入口：未启用", "Entrance: disabled")),
    }
    if changed.is_some() {
        println!("{}", restart_hint());
    }
    Ok(())
}

fn normalize_or_fail(value: &str) -> Result<String> {
    panel_access::normalize_entrance(value)
        .map_err(|e| anyhow!(e))?
        .ok_or_else(|| {
            anyhow!(tr!(
                "安全入口不能为空；要关闭请使用 `entrance off`",
                "Entrance must not be empty; use `entrance off` to disable it"
            ))
        })
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
                    .ok_or_else(|| anyhow!(tr!("--san 需要一个值", "--san requires a value")))?
                    .clone(),
            ),
            "--regenerate" => regenerate = true,
            other => bail!(tr!(
                "未知参数：{}\n\n{}",
                "Unknown argument: {}\n\n{}",
                other,
                usage()
            )),
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
                    "{}",
                    tr!(
                        "已生成自签名证书：{}",
                        "Generated a self-signed certificate: {}",
                        cert_path.display()
                    )
                );
            }
            panel_access::load_tls_config(&cert_path, &key_path)?;
            db::set_setting(&pool, panel_access::SETTING_PANEL_SSL, "on").await?;
        }
        "off" | "disable" => {
            db::set_setting(&pool, panel_access::SETTING_PANEL_SSL, "off").await?;
        }
        other => bail!(tr!(
            "未知的 ssl 操作：{}\n\n{}",
            "Unknown ssl action: {}\n\n{}",
            other,
            usage()
        )),
    }

    let enabled = panel_access::ssl_enabled(&pool).await;
    let (cert_path, _, self_signed) = panel_access::ssl_paths(&pool, &data_dir).await;
    pool.close().await;
    println!("HTTPS: {}", on_off(enabled));
    if enabled {
        println!(
            "{}",
            tr!(
                "证书：{}{}",
                "Certificate: {}{}",
                cert_path.display(),
                if self_signed {
                    tr!("（自签名）", " (self-signed)")
                } else {
                    String::new()
                }
            )
        );
    }
    if action != "status" {
        println!("{}", restart_hint());
    }
    Ok(())
}

fn restart_hint() -> String {
    tr!(
        "重启面板后生效：Lp restart",
        "Restart the panel to apply: Lp restart"
    )
}

fn on_off(on: bool) -> String {
    match (on, is_zh()) {
        (true, true) => "开启".into(),
        (false, true) => "关闭".into(),
        (true, false) => "on".into(),
        (false, false) => "off".into(),
    }
}

/// `lang [show|zh|en]`: one language for the web UI, `Lp` and these commands.
async fn lang(args: &[String]) -> Result<()> {
    let (pool, _) = crate::open_database().await?;
    match action(args, "show") {
        "show" | "status" => {}
        value => {
            let lang = normalize_language(value).ok_or_else(|| {
                anyhow!(tr!(
                    "不支持的语言：{}（可选 zh、en）",
                    "Unsupported language: {} (use zh or en)",
                    value
                ))
            })?;
            db::set_setting(&pool, SETTING_LANGUAGE, lang).await?;
            ZH.store(lang == "zh", Ordering::Relaxed);
        }
    }
    let saved = db::get_setting(&pool, SETTING_LANGUAGE)
        .await?
        .unwrap_or_default();
    pool.close().await;
    let lang = normalize_language(&saved).unwrap_or("en");
    println!(
        "{}",
        tr!(
            "语言：{}（面板和命令行一致）",
            "Language: {} (shared by the panel and the CLI)",
            if lang == "zh" {
                "中文 (zh)"
            } else {
                "English (en)"
            }
        )
    );
    Ok(())
}

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
            println!("{}", restart_hint());
        }
        value => {
            let port = value
                .parse::<i64>()
                .map_err(|_| anyhow!(tr!("不是有效的端口号：{}", "Not a port number: {}", value)))
                .and_then(|p| panel_access::validate_port(p).map_err(|e| anyhow!(e)))?;
            if std::net::TcpListener::bind(("0.0.0.0", port)).is_err() {
                println!(
                    "{}",
                    tr!(
                        "提示：端口 {} 当前已被占用（如果是面板自己在用则没有问题）",
                        "Note: port {} is in use right now (fine if the panel itself uses it)",
                        port
                    )
                );
            }
            db::set_setting(&pool, panel_access::SETTING_LISTEN_PORT, &port.to_string()).await?;
            println!("{}", restart_hint());
        }
    }
    println!(
        "{}",
        tr!(
            "端口：{}",
            "Port: {}",
            panel_access::effective_port(&pool).await
        )
    );
    pool.close().await;
    Ok(())
}

async fn allow_ip(args: &[String]) -> Result<()> {
    let (pool, _) = crate::open_database().await?;
    match action(args, "show") {
        "show" | "status" => {}
        "off" | "clear" => {
            db::set_setting(&pool, panel_access::SETTING_ALLOWED_IPS, "").await?;
            println!("{}", restart_hint());
        }
        _ => {
            let list = panel_access::parse_ip_list(&args.join(",")).map_err(|e| anyhow!(e))?;
            let value = panel_access::format_ip_list(&list);
            db::set_setting(&pool, panel_access::SETTING_ALLOWED_IPS, &value).await?;
            println!("{}", restart_hint());
        }
    }
    let policy = panel_access::AccessPolicy::load(&pool).await;
    pool.close().await;
    if policy.allowed_ips.is_empty() {
        println!("{}", tr!("授权 IP：不限", "Allowed IPs: any"));
    } else {
        println!(
            "{}",
            tr!(
                "授权 IP：{}",
                "Allowed IPs: {}",
                panel_access::format_ip_list(&policy.allowed_ips)
            )
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
            println!("{}", restart_hint());
        }
        value => {
            let domain = panel_access::normalize_domain(value)
                .map_err(|e| anyhow!(e))?
                .unwrap_or_default();
            db::set_setting(&pool, panel_access::SETTING_BIND_DOMAIN, &domain).await?;
            println!("{}", restart_hint());
        }
    }
    let policy = panel_access::AccessPolicy::load(&pool).await;
    pool.close().await;
    match policy.domain.as_deref() {
        Some(domain) => println!("{}", tr!("绑定域名：{}", "Bound domain: {}", domain)),
        None => println!("{}", tr!("绑定域名：未绑定", "Bound domain: none")),
    }
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
                        .ok_or_else(|| {
                            anyhow!(tr!("--user 需要一个值", "--user requires a value"))
                        })?
                        .clone(),
                );
            }
            other => bail!(tr!(
                "未知参数：{}\n\n{}",
                "Unknown argument: {}\n\n{}",
                other,
                usage()
            )),
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
                bail!(tr!("没有这个用户：{}", "No such user: {}", username));
            }
            println!(
                "{}",
                tr!(
                    "已关闭 {} 的两步验证",
                    "Two-factor login turned off for {}",
                    username
                )
            );
        }
        other => bail!(tr!(
            "未知的 mfa 操作：{}\n\n{}",
            "Unknown mfa action: {}\n\n{}",
            other,
            usage()
        )),
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
        "{}",
        tr!(
            "{} 的两步验证：{}",
            "Two-factor login for {}: {}",
            username,
            on_off(on)
        )
    );
    Ok(())
}

/// `backup site|db ...`: what scheduled backup jobs in cron run.
async fn backup(args: &[String]) -> Result<()> {
    let mut positional = Vec::new();
    let mut keep = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--keep" {
            let value = iter
                .next()
                .ok_or_else(|| anyhow!(tr!("--keep 需要一个数字", "--keep requires a number")))?;
            keep = Some(
                value
                    .parse::<usize>()
                    .map_err(|_| anyhow!(tr!("无效的 --keep：{}", "Invalid --keep: {}", value)))?,
            );
        } else {
            positional.push(arg.as_str());
        }
    }
    let (pool, data_dir) = crate::open_database().await?;
    let path = match positional.as_slice() {
        ["site", id] => {
            let id: i64 = id
                .parse()
                .map_err(|_| anyhow!(tr!("网站 ID 必须是数字", "Site id must be a number")))?;
            let site = sqlx::query_as::<_, crate::models::Site>("SELECT * FROM sites WHERE id = ?")
                .bind(id)
                .fetch_optional(&pool)
                .await?
                .ok_or_else(|| anyhow!(tr!("没有 ID 为 {} 的网站", "No site with id {}", id)))?;
            crate::services::backup::backup_site(
                &data_dir,
                site.id,
                &site.name,
                std::path::Path::new(&site.doc_root),
                keep,
            )
            .map_err(|e| anyhow!(e))?
        }
        ["db", engine, name] => {
            use crate::services::database;
            let engine = database::Engine::parse(engine).map_err(|e| anyhow!(e))?;
            let admin = database::load_admin(&pool, engine).await;
            database::backup(&data_dir, engine, name, admin.as_ref(), keep)
                .map_err(|e| anyhow!(e))?
        }
        _ => bail!(tr!(
            "用法：backup site <ID> | backup db <引擎> <库名> [--keep N]",
            "Usage: backup site <ID> | backup db <ENGINE> <NAME> [--keep N]"
        )),
    };
    pool.close().await;
    println!("{}", path.display());
    Ok(())
}

fn parse_ip(raw: Option<&String>) -> Result<std::net::IpAddr> {
    let raw = raw.ok_or_else(|| anyhow!(tr!("需要提供 IP 地址", "An IP address is required")))?;
    raw.trim()
        .parse::<std::net::IpAddr>()
        .map(|ip| ip.to_canonical())
        .map_err(|_| anyhow!(tr!("不是有效的 IP 地址：{}", "Not an IP address: {}", raw)))
}

async fn ban(args: &[String]) -> Result<()> {
    let ip = parse_ip(args.first())?;
    if let Some(why) = defense::never_ban(&ip) {
        let why = match why {
            "loopback or special address" => {
                tr!("本机回环或特殊地址", "loopback or special address")
            }
            "this server's own address" => tr!("这是服务器自己的地址", "this server's own address"),
            other => other.to_string(),
        };
        bail!(tr!("不能封禁 {}：{}", "Cannot ban {}: {}", ip, why));
    }
    let (pool, _) = crate::open_database().await?;
    let minutes = match args.get(1) {
        Some(raw) => raw
            .parse::<u64>()
            .ok()
            .filter(|m| *m <= 525_600)
            .ok_or_else(|| {
                anyhow!(tr!(
                    "分钟数必须在 0-525600 之间",
                    "MINUTES must be 0-525600"
                ))
            })?,
        None => defense::load_config(&pool).await.ban_minutes,
    };
    let result = defense::ban(
        &pool,
        ip,
        (minutes > 0).then_some(minutes * 60),
        "manual",
        "Banned from the command line",
    )
    .await;
    pool.close().await;
    result.map_err(|e| anyhow!(e))?;
    match minutes {
        0 => println!("{}", tr!("已永久封禁 {}", "Banned {} permanently", ip)),
        m => println!(
            "{}",
            tr!("已封禁 {}，{} 分钟", "Banned {} for {} minutes", ip, m)
        ),
    }
    Ok(())
}

async fn unban(args: &[String]) -> Result<()> {
    let (pool, _) = crate::open_database().await?;
    let targets: Vec<std::net::IpAddr> = if args.first().map(String::as_str) == Some("all") {
        // Kernel elements too: a ban whose record is gone must not linger.
        let mut ips: Vec<std::net::IpAddr> = defense::list_bans(&pool)
            .await
            .into_iter()
            .filter_map(|b| b.ip.parse().ok())
            .collect();
        if let Ok(kernel) = ipban::list() {
            ips.extend(
                kernel
                    .into_iter()
                    .filter_map(|b| b.ip.parse::<std::net::IpAddr>().ok()),
            );
        }
        ips.sort();
        ips.dedup();
        ips
    } else {
        vec![parse_ip(args.first())?]
    };
    let mut failed = None;
    for ip in &targets {
        if let Err(err) = defense::unban(&pool, *ip).await {
            failed = Some(format!("{}: {}", ip, err));
        }
    }
    pool.close().await;
    if let Some(err) = failed {
        bail!(err);
    }
    println!(
        "{}",
        tr!(
            "已解除 {} 个地址的封禁",
            "Unbanned {} address(es)",
            targets.len()
        )
    );
    Ok(())
}

async fn bans() -> Result<()> {
    let (pool, _) = crate::open_database().await?;
    let records = defense::list_bans(&pool).await;
    pool.close().await;
    if records.is_empty() {
        println!("{}", tr!("没有被封禁的地址", "No banned addresses"));
    }
    let now = chrono::Utc::now().timestamp();
    for record in records {
        let left = match record.expires_at {
            0 => tr!("永久", "permanent"),
            at => {
                let minutes = ((at - now).max(0) as u64).div_ceil(60);
                tr!("剩余 {} 分钟", "{}m left", minutes)
            }
        };
        println!(
            "{:<40} {:<10} {:<8} {}",
            record.ip, left, record.source, record.reason
        );
    }
    Ok(())
}

async fn defense_cmd(args: &[String]) -> Result<()> {
    let (pool, _) = crate::open_database().await?;
    let mut config = defense::load_config(&pool).await;
    match action(args, "status") {
        "status" | "show" => {}
        "on" | "enable" => config.enabled = true,
        "off" | "disable" => config.enabled = false,
        other => bail!(tr!(
            "未知的 defense 操作：{}",
            "Unknown defense action: {}",
            other
        )),
    }
    defense::save_config(&pool, &config).await?;
    pool.close().await;
    println!(
        "{}",
        tr!(
            "自动封禁：{}（封禁 {} 分钟；网站 {} 次请求/{} 秒，SSH {} 次失败/{} 秒，面板 {} 次拒绝/{} 秒）",
            "Automatic banning: {} (ban {} min; web {} req/{}s, ssh {} failures/{}s, panel {} denials/{}s)",
            on_off(config.enabled),
            config.ban_minutes,
            config.web_max_requests,
            config.web_window,
            config.ssh_max_failures,
            config.ssh_window,
            config.panel_max_denied,
            config.panel_window
        )
    );
    println!("nftables: {}", installed_text(ipban::available()));
    Ok(())
}

fn harden(args: &[String]) -> Result<()> {
    match action(args, "status") {
        "status" | "show" => {}
        "on" | "enable" => {
            let output = defense::apply_sysctl().map_err(|e| anyhow!(e))?;
            print!("{}", output);
        }
        "off" | "disable" => {
            defense::remove_sysctl().map_err(|e| anyhow!(e))?;
            println!(
                "{}",
                tr!(
                    "加固配置已删除；重启后恢复内核默认值",
                    "Profile removed; kernel defaults return after a reboot"
                )
            );
        }
        other => bail!(tr!(
            "未知的 harden 操作：{}",
            "Unknown harden action: {}",
            other
        )),
    }
    let status = defense::sysctl_status();
    println!(
        "{}",
        tr!(
            "加固配置 {}：{}；SYN Cookie {}；somaxconn {}",
            "Profile {}: {}; SYN cookies {}; somaxconn {}",
            defense::SYSCTL_PATH,
            installed_text(status.applied),
            on_off(status.syncookies),
            status.somaxconn
        )
    );
    if status.applied && !status.syncookies {
        bail!(tr!(
            "加固配置已安装，但内核没有接受",
            "The profile is installed but the kernel did not accept it"
        ));
    }
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
    let port = panel_access::effective_port(&pool).await;
    pool.close().await;

    let label = |zh: &str, en: &str| {
        if is_zh() {
            pad_display(zh, 12)
        } else {
            format!("{:<17}", en)
        }
    };
    println!("{}{}", label("端口：", "Port:"), port);
    println!(
        "{}{}",
        label("安全入口：", "Entrance:"),
        policy
            .entrance
            .clone()
            .unwrap_or_else(|| tr!("未启用", "disabled"))
    );
    println!(
        "{}{}{}",
        label("HTTPS：", "HTTPS:"),
        on_off(ssl),
        match (ssl, ssl_ok, self_signed) {
            (true, false, _) => tr!(
                "（证书无法加载，当前使用 HTTP）",
                " (certificate cannot be loaded; serving HTTP)"
            ),
            (true, true, true) => tr!("（自签名）", " (self-signed)"),
            _ => String::new(),
        }
    );
    println!(
        "{}{}",
        label("授权 IP：", "Allowed IPs:"),
        if policy.allowed_ips.is_empty() {
            tr!("不限", "any")
        } else {
            panel_access::format_ip_list(&policy.allowed_ips)
        }
    );
    println!(
        "{}{}",
        label("绑定域名：", "Bound domain:"),
        policy
            .domain
            .clone()
            .unwrap_or_else(|| tr!("未绑定", "none"))
    );
    let deny = match policy.deny.as_str() {
        "hint" => tr!("提示页（hint）", "hint"),
        code => code.to_string(),
    };
    println!("{}{}", label("拒绝访问：", "Denied visitors:"), deny);
    for (username, secret) in users {
        let on = secret.is_some_and(|s| !s.is_empty());
        println!(
            "{}{}",
            label("用户：", "User:"),
            tr!(
                "{}（两步验证 {}）",
                "{} (two-factor {})",
                username,
                on_off(on)
            )
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
    let language = db::get_setting(&pool, SETTING_LANGUAGE)
        .await
        .ok()
        .flatten()
        .and_then(|value| normalize_language(&value))
        .unwrap_or_default();
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
    println!("language={}", language);
    Ok(())
}

fn installed_text(installed: bool) -> String {
    if installed {
        tr!("已安装", "installed")
    } else {
        tr!("未安装", "not installed")
    }
}

/// Pads to a terminal width, counting CJK characters as two columns.
fn pad_display(text: &str, width: usize) -> String {
    let used: usize = text
        .chars()
        .map(|c| if (c as u32) >= 0x1100 { 2 } else { 1 })
        .sum();
    format!("{}{}", text, " ".repeat(width.saturating_sub(used)))
}
