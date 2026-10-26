//! One-click website programs (WordPress, Typecho, Adminer...): the catalog and
//! the file-level steps of a deployment - download, unpack, configure, permissions.
//! The orchestration (database, site, rollback) lives in `api::appstore_api`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use super::{runtime, shell};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Archive {
    TarGz,
    Zip,
    /// A single PHP file, saved as `index.php`.
    PhpFile,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppDef {
    pub key: &'static str,
    pub name: &'static str,
    pub needs_mysql: bool,
    pub homepage: &'static str,
    #[serde(skip)]
    pub archive: Archive,
    #[serde(skip)]
    url_en: &'static str,
    #[serde(skip)]
    url_zh: &'static str,
}

impl AppDef {
    pub fn default_url(&self, locale: &str) -> &'static str {
        if locale.starts_with("zh") {
            self.url_zh
        } else {
            self.url_en
        }
    }
}

pub const CATALOG: &[AppDef] = &[
    AppDef {
        key: "wordpress",
        name: "WordPress",
        needs_mysql: true,
        homepage: "https://wordpress.org",
        archive: Archive::TarGz,
        url_en: "https://wordpress.org/latest.tar.gz",
        url_zh: "https://cn.wordpress.org/latest-zh_CN.tar.gz",
    },
    AppDef {
        key: "typecho",
        name: "Typecho",
        needs_mysql: true,
        homepage: "https://typecho.org",
        archive: Archive::Zip,
        url_en: "https://github.com/typecho/typecho/releases/latest/download/typecho.zip",
        url_zh: "https://github.com/typecho/typecho/releases/latest/download/typecho.zip",
    },
    AppDef {
        key: "adminer",
        name: "Adminer",
        needs_mysql: false,
        homepage: "https://www.adminer.org",
        archive: Archive::PhpFile,
        url_en: "https://www.adminer.org/latest.php",
        url_zh: "https://www.adminer.org/latest.php",
    },
];

pub fn find(key: &str) -> Option<&'static AppDef> {
    CATALOG.iter().find(|app| app.key == key)
}

/// Only http(s) URLs of printable characters, passed to curl as one argument.
pub fn validate_url(url: &str) -> Result<(), String> {
    let ok = (url.starts_with("https://") || url.starts_with("http://"))
        && url.len() <= 1000
        && url.bytes().all(|b| b.is_ascii_graphic());
    if ok {
        Ok(())
    } else {
        Err("Download URL must be an http(s) URL".into())
    }
}

/// A database / user name derived from a domain: `blog.example.com` -> `blog_example`.
pub fn db_name_for(domain: &str) -> String {
    let mut name: String = domain
        .split('.')
        .take(2)
        .collect::<Vec<_>>()
        .join("_")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if !name.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
        name.insert_str(0, "db_");
    }
    name.truncate(16);
    name.trim_end_matches('_').to_string()
}

pub fn dir_is_empty(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(true)
}

fn run_step(
    log: &mut String,
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<(), String> {
    log.push_str(&format!("$ {} {}\n", program, args.join(" ")));
    let output = shell::run_with_timeout(program, args, timeout)?;
    let text = format!("{}{}", output.stdout, output.stderr);
    if !text.trim().is_empty() {
        // Download progress is noisy; the tail is what matters.
        let tail: Vec<&str> = text.trim_end().lines().rev().take(15).collect();
        for line in tail.into_iter().rev() {
            log.push_str(line);
            log.push('\n');
        }
    }
    if output.ok() {
        Ok(())
    } else {
        Err(format!("{} failed: {}", program, output.error_text()))
    }
}

pub fn download(url: &str, target: &Path, log: &mut String) -> Result<(), String> {
    validate_url(url)?;
    let target_str = target.to_string_lossy().to_string();
    if shell::has_binary("curl") {
        run_step(
            log,
            "curl",
            &[
                "-fSL",
                "--retry",
                "3",
                "--connect-timeout",
                "15",
                "--max-time",
                "900",
                "-sS",
                "-o",
                &target_str,
                url,
            ],
            Duration::from_secs(960),
        )
    } else {
        run_step(
            log,
            "wget",
            &["-q", "--tries=3", "--timeout=30", "-O", &target_str, url],
            Duration::from_secs(960),
        )
    }
}

/// Unpacks `archive` into `doc_root`, flattening a single top-level directory
/// (`wordpress/...` -> doc root).
pub fn unpack(
    app: &AppDef,
    archive: &Path,
    doc_root: &Path,
    work: &Path,
    log: &mut String,
) -> Result<(), String> {
    std::fs::create_dir_all(doc_root).map_err(|e| e.to_string())?;
    if app.archive == Archive::PhpFile {
        std::fs::copy(archive, doc_root.join("index.php")).map_err(|e| e.to_string())?;
        return Ok(());
    }

    let extract = work.join("extract");
    std::fs::create_dir_all(&extract).map_err(|e| e.to_string())?;
    let archive_str = archive.to_string_lossy().to_string();
    let extract_str = extract.to_string_lossy().to_string();
    match app.archive {
        Archive::TarGz => run_step(
            log,
            "tar",
            &["-xzf", &archive_str, "-C", &extract_str],
            Duration::from_secs(300),
        )?,
        Archive::Zip => {
            if !shell::has_binary("unzip") {
                log.push_str("[LinxPanel] Installing unzip\n");
                runtime::install_packages(&["unzip"], log);
            }
            run_step(
                log,
                "unzip",
                &["-q", "-o", &archive_str, "-d", &extract_str],
                Duration::from_secs(300),
            )?
        }
        Archive::PhpFile => unreachable!(),
    }

    let entries: Vec<PathBuf> = std::fs::read_dir(&extract)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    let source = match entries.as_slice() {
        [only] if only.is_dir() => only.clone(),
        _ => extract.clone(),
    };
    let source_contents = format!("{}/.", source.to_string_lossy());
    let doc_root_str = doc_root.to_string_lossy().to_string();
    // `cp -a` rather than rename: /www may be on another filesystem than the work dir.
    run_step(
        log,
        "cp",
        &["-a", &source_contents, &doc_root_str],
        Duration::from_secs(300),
    )
}

fn random_phrase() -> String {
    // No quotes or backslashes: the value lands inside a PHP string literal.
    crate::db::generate_password(64)
}

/// Writes wp-config.php from the bundled sample.
pub fn configure_wordpress(
    doc_root: &Path,
    db: &str,
    user: &str,
    password: &str,
) -> Result<(), String> {
    let sample = std::fs::read_to_string(doc_root.join("wp-config-sample.php"))
        .map_err(|e| format!("wp-config-sample.php: {}", e))?;
    let mut config = sample
        .replace("database_name_here", db)
        .replace("username_here", user)
        .replace("password_here", password);
    while config.contains("put your unique phrase here") {
        config = config.replacen("put your unique phrase here", &random_phrase(), 1);
    }
    // Let WordPress install plugins and updates directly instead of asking for FTP.
    if let Some(index) = config.find("$table_prefix") {
        config.insert_str(index, "define( 'FS_METHOD', 'direct' );\n\n");
    }
    std::fs::write(doc_root.join("wp-config.php"), config).map_err(|e| e.to_string())
}

/// Gives the PHP-FPM user ownership, so uploads, plugins and caches can be written.
pub fn set_owner(doc_root: &Path, user: &str, log: &mut String) -> Result<(), String> {
    let owner = format!("{}:{}", user, user);
    let path = doc_root.to_string_lossy().to_string();
    run_step(
        log,
        "chown",
        &["-R", &owner, &path],
        Duration::from_secs(300),
    )
    .or_else(|_| run_step(log, "chown", &["-R", user, &path], Duration::from_secs(300)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_names_from_domains() {
        assert_eq!(db_name_for("blog.example.com"), "blog_example");
        assert_eq!(db_name_for("123.example.com"), "db_123_example");
        assert_eq!(db_name_for("my-site.io"), "my_site_io");
        assert_eq!(db_name_for("averyveryverylongname.example.com").len(), 16);
        for name in ["blog.example.com", "123.example.com", "a-b.c"] {
            assert!(crate::services::database::validate_name(&db_name_for(name), "x").is_ok());
        }
    }

    #[test]
    fn urls_are_validated() {
        assert!(validate_url("https://wordpress.org/latest.tar.gz").is_ok());
        assert!(validate_url("file:///etc/passwd").is_err());
        assert!(validate_url("https://x/a b").is_err());
        assert!(validate_url("-o/etc/x").is_err());
    }

    #[test]
    fn wordpress_config_is_filled() {
        let dir = std::env::temp_dir().join(format!("lp-wp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("wp-config-sample.php"),
            "define( 'DB_NAME', 'database_name_here' );\ndefine( 'DB_USER', 'username_here' );\n\
             define( 'DB_PASSWORD', 'password_here' );\ndefine( 'AUTH_KEY', 'put your unique phrase here' );\n\
             define( 'SECURE_AUTH_KEY', 'put your unique phrase here' );\n$table_prefix = 'wp_';\n",
        )
        .unwrap();
        configure_wordpress(&dir, "blog", "bloguser", "Secret123").unwrap();
        let config = std::fs::read_to_string(dir.join("wp-config.php")).unwrap();
        assert!(config.contains("'DB_NAME', 'blog'"));
        assert!(config.contains("'DB_USER', 'bloguser'"));
        assert!(config.contains("'DB_PASSWORD', 'Secret123'"));
        assert!(!config.contains("put your unique phrase here"));
        assert!(config.contains("FS_METHOD"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
