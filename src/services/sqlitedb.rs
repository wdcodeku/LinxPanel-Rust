//! SQLite databases: plain files that belong to an application, so the
//! panel manages them by path.
//!
//! - Files are found by scanning the usual application directories (the
//!   SQLite header, not just the extension, decides), or registered by hand.
//! - Inspection opens the file read-only; backups use `VACUUM INTO`, which
//!   produces a consistent copy even while the application writes.
//! - The panel's own database and system directories are never touched.

use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::{ConnectOptions, Connection, Row, SqliteConnection, SqlitePool};

use super::database::{self, BackupFile};

/// The registered files (a JSON list of paths).
pub const SETTING_FILES: &str = "sqlite_files";
const HEADER: &[u8; 16] = b"SQLite format 3\0";
const EXTENSIONS: &[&str] = &["db", "sqlite", "sqlite3", "db3", "s3db", "sl3"];
const SCAN_ROOTS: &[&str] = &["/www/wwwroot", "/var/www", "/opt", "/srv", "/home", "/root"];
const SCAN_SKIP: &[&str] = &["node_modules", "proc"];
const SCAN_MAX_DEPTH: usize = 7;
const SCAN_MAX_ENTRIES: usize = 60_000;
const FORBIDDEN: &[&str] = &["/proc", "/sys", "/dev", "/run", "/boot"];
const MAX_TABLES: usize = 500;

#[derive(Debug, Clone, Serialize)]
pub struct SqliteFile {
    pub path: String,
    pub exists: bool,
    pub size: u64,
    /// Including the -wal file, which holds not yet checkpointed changes.
    pub wal_size: u64,
    pub tables: Option<u64>,
    pub journal_mode: Option<String>,
    pub owner: Option<String>,
    pub modified: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Found {
    pub path: String,
    pub size: u64,
    pub modified: Option<String>,
}

// ===== Paths =====

fn has_header(path: &Path) -> bool {
    let mut buf = [0u8; 16];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut buf))
        .is_ok()
        && &buf == HEADER
}

fn modified(meta: &std::fs::Metadata) -> Option<String> {
    let time: chrono::DateTime<chrono::Local> = meta.modified().ok()?.into();
    Some(time.format("%Y-%m-%d %H:%M:%S").to_string())
}

fn owner_name(meta: &std::fs::Metadata) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let uid = meta.uid();
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    passwd
        .lines()
        .find_map(|line| {
            let mut cols = line.split(':');
            let name = cols.next()?;
            let id: u32 = cols.nth(1)?.parse().ok()?;
            (id == uid).then(|| name.to_string())
        })
        .or_else(|| Some(uid.to_string()))
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The panel's own files: its data directory, its install directory (with
/// the upgrade backups of its database) and its database file.
fn panel_paths(data_dir: &Path) -> Vec<PathBuf> {
    let data_dir = canonical(data_dir);
    let mut paths = vec![data_dir.clone()];
    if let (Some(root), Ok(exe)) = (data_dir.parent(), std::env::current_exe()) {
        if canonical(&exe).starts_with(root) {
            paths.push(root.to_path_buf());
        }
    }
    if let Ok(url) = std::env::var("DATABASE_URL") {
        let file = url
            .trim_start_matches("sqlite://")
            .trim_start_matches("sqlite:")
            .split('?')
            .next()
            .unwrap_or_default()
            .to_string();
        if !file.is_empty() {
            paths.push(canonical(Path::new(&file)));
        }
    }
    paths
}

/// A path the panel may manage: absolute, no `..`, outside system
/// directories and the panel's own files.
fn check_path(raw: &str, data_dir: &Path) -> Result<PathBuf, String> {
    let raw = raw.trim();
    let path = PathBuf::from(raw);
    if raw.is_empty() || !path.is_absolute() || raw.len() > 1024 {
        return Err("Enter the database file's absolute path".into());
    }
    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err("The path must not contain . or ..".into());
    }
    // Resolve symlinks of what exists, so a link cannot point elsewhere.
    let resolved = if path.exists() {
        std::fs::canonicalize(&path).map_err(|e| e.to_string())?
    } else {
        let parent = path.parent().ok_or("Invalid path")?;
        let parent = std::fs::canonicalize(parent)
            .map_err(|_| format!("The directory {} does not exist", parent.display()))?;
        parent.join(path.file_name().ok_or("Invalid path")?)
    };
    if panel_paths(data_dir)
        .iter()
        .any(|p| resolved.starts_with(p))
    {
        return Err("The panel's own files cannot be managed here".into());
    }
    if FORBIDDEN.iter().any(|dir| resolved.starts_with(dir)) || resolved.parent().is_none() {
        return Err("System directories cannot be used".into());
    }
    Ok(resolved)
}

/// An existing SQLite file.
fn check_existing(raw: &str, data_dir: &Path) -> Result<PathBuf, String> {
    let path = check_path(raw, data_dir)?;
    let meta =
        std::fs::metadata(&path).map_err(|_| format!("{} does not exist", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if meta.len() > 0 && !has_header(&path) {
        return Err(format!("{} is not an SQLite database", path.display()));
    }
    Ok(path)
}

// ===== Registry =====

pub async fn registered(pool: &SqlitePool) -> Vec<String> {
    crate::db::get_setting(pool, SETTING_FILES)
        .await
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

async fn save_registered(pool: &SqlitePool, files: &[String]) -> Result<(), String> {
    let text = serde_json::to_string(files).map_err(|e| e.to_string())?;
    crate::db::set_setting(pool, SETTING_FILES, &text)
        .await
        .map_err(|e| e.to_string())
}

/// Only registered files can be inspected, backed up or restored.
async fn registered_path(pool: &SqlitePool, raw: &str, data_dir: &Path) -> Result<PathBuf, String> {
    let path = check_path(raw, data_dir)?;
    let text = path.display().to_string();
    if registered(pool).await.contains(&text) {
        Ok(path)
    } else {
        Err("This file is not in the panel's SQLite list".into())
    }
}

async fn add_registered(pool: &SqlitePool, path: &Path) -> Result<(), String> {
    let mut files = registered(pool).await;
    let text = path.display().to_string();
    if !files.contains(&text) {
        files.push(text);
        files.sort();
        save_registered(pool, &files).await?;
    }
    Ok(())
}

// ===== Connections =====

fn options(path: &Path, read_only: bool) -> SqliteConnectOptions {
    let mut options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(read_only)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(false)
        .disable_statement_logging();
    if !read_only {
        // Never switch an application's database to another journal mode.
        options = options.journal_mode(SqliteJournalMode::Delete);
    }
    options
}

async fn open_read_only(path: &Path) -> Result<SqliteConnection, String> {
    options(path, true)
        .connect()
        .await
        .map_err(|e| format!("Cannot open {}: {}", path.display(), e))
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn quote_str(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

async fn describe(path: &Path) -> SqliteFile {
    let meta = std::fs::metadata(path).ok();
    let wal = PathBuf::from(format!("{}-wal", path.display()));
    let mut file = SqliteFile {
        path: path.display().to_string(),
        exists: meta.as_ref().is_some_and(|m| m.is_file()),
        size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
        wal_size: std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0),
        tables: None,
        journal_mode: None,
        owner: meta.as_ref().and_then(owner_name),
        modified: meta.as_ref().and_then(modified),
        error: None,
    };
    if !file.exists {
        return file;
    }
    match open_read_only(path).await {
        Ok(mut conn) => {
            let tables: Result<i64, _> = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            )
            .fetch_one(&mut conn)
            .await;
            match tables {
                Ok(count) => file.tables = Some(count.max(0) as u64),
                Err(err) => file.error = Some(err.to_string()),
            }
            file.journal_mode = sqlx::query_scalar("PRAGMA journal_mode")
                .fetch_one(&mut conn)
                .await
                .ok();
            let _ = conn.close().await;
        }
        Err(err) => file.error = Some(err),
    }
    file
}

// ===== Public API =====

pub async fn list(pool: &SqlitePool) -> Vec<SqliteFile> {
    let mut out = Vec::new();
    for path in registered(pool).await {
        out.push(describe(Path::new(&path)).await);
    }
    out
}

/// SQLite files under the usual application directories that are not in
/// the list yet (bounded, so a huge tree cannot stall the panel).
pub fn discover(data_dir: &Path, known: &[String]) -> Vec<Found> {
    let skip = panel_paths(data_dir);
    let mut found = Vec::new();
    let mut visited = 0usize;
    let mut stack: Vec<(PathBuf, usize)> = SCAN_ROOTS
        .iter()
        .map(|root| (PathBuf::from(root), 0))
        .filter(|(root, _)| root.is_dir())
        .collect();
    while let Some((dir, depth)) = stack.pop() {
        if skip.iter().any(|p| dir.starts_with(p)) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            visited += 1;
            if visited > SCAN_MAX_ENTRIES {
                return found;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if kind.is_dir() {
                // Hidden directories hold tool state (browser / NSS stores ...).
                if depth + 1 < SCAN_MAX_DEPTH
                    && !name.starts_with('.')
                    && !SCAN_SKIP.contains(&name.as_str())
                {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            if !kind.is_file() {
                continue; // symlinks are not followed
            }
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            if !EXTENSIONS.contains(&ext.as_str()) {
                continue;
            }
            let text = path.display().to_string();
            if known.contains(&text) || skip.contains(&path) || !has_header(&path) {
                continue;
            }
            let meta = entry.metadata().ok();
            found.push(Found {
                path: text,
                size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                modified: meta.as_ref().and_then(modified),
            });
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

pub async fn register(pool: &SqlitePool, data_dir: &Path, raw: &str) -> Result<SqliteFile, String> {
    let path = check_existing(raw, data_dir)?;
    add_registered(pool, &path).await?;
    Ok(describe(&path).await)
}

/// Creates an empty database owned like its directory (so the application
/// that lives there can write it).
pub async fn create(pool: &SqlitePool, data_dir: &Path, raw: &str) -> Result<SqliteFile, String> {
    let path = check_path(raw, data_dir)?;
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!(
            "Use one of the file extensions: .{}",
            EXTENSIONS.join(" .")
        ));
    }
    if path.exists() {
        return Err(format!(
            "{} already exists; add it to the list instead",
            path.display()
        ));
    }
    let parent = path.parent().ok_or("Invalid path")?.to_path_buf();
    let mut conn = options(&path, false)
        .create_if_missing(true)
        .connect()
        .await
        .map_err(|e| format!("Cannot create {}: {}", path.display(), e))?;
    // An SQLite file stays empty until the first write; give it its header.
    let init = sqlx::query("CREATE TABLE linxpanel_init (x); DROP TABLE linxpanel_init;")
        .execute(&mut conn)
        .await;
    let _ = conn.close().await;
    if let Err(err) = init {
        let _ = std::fs::remove_file(&path);
        return Err(err.to_string());
    }
    copy_owner(&parent, &path);
    add_registered(pool, &path).await?;
    Ok(describe(&path).await)
}

fn copy_owner(from: &Path, to: &Path) {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if let Ok(meta) = std::fs::metadata(from) {
        let _ = std::os::unix::fs::chown(to, Some(meta.uid()), Some(meta.gid()));
    }
    let _ = std::fs::set_permissions(to, std::fs::Permissions::from_mode(0o640));
}

/// Takes a file off the list; with `delete_file` also deletes it (and its
/// -wal / -shm companions).
pub async fn remove(
    pool: &SqlitePool,
    data_dir: &Path,
    raw: &str,
    delete_file: bool,
) -> Result<(), String> {
    let mut files = registered(pool).await;
    let before = files.len();
    files.retain(|f| f != raw.trim());
    if files.len() == before {
        return Err("This file is not in the panel's SQLite list".into());
    }
    if delete_file {
        let path = check_path(raw, data_dir)?;
        if path.exists() {
            if !has_header(&path) && std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > 0 {
                return Err(format!(
                    "{} is not an SQLite database; not deleted",
                    path.display()
                ));
            }
            std::fs::remove_file(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
        }
        for suffix in ["-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
        }
    }
    save_registered(pool, &files).await
}

pub async fn tables(
    pool: &SqlitePool,
    data_dir: &Path,
    raw: &str,
) -> Result<Vec<database::TableInfo>, String> {
    let path = registered_path(pool, raw, data_dir).await?;
    let mut conn = open_read_only(&path).await?;
    let rows = sqlx::query(
        "SELECT name, type FROM sqlite_master WHERE type IN ('table', 'view') \
         AND name NOT LIKE 'sqlite_%' ORDER BY name",
    )
    .fetch_all(&mut conn)
    .await
    .map_err(|e| e.to_string())?;
    // Page usage per table (dbstat may be compiled out; sizes are then 0).
    let sizes: Vec<(String, i64)> =
        sqlx::query_as("SELECT name, SUM(pgsize) FROM dbstat GROUP BY name")
            .fetch_all(&mut conn)
            .await
            .unwrap_or_default();
    let mut out = Vec::new();
    for row in rows.iter().take(MAX_TABLES) {
        let name: String = row.get(0);
        let kind: String = row.get(1);
        let count = if kind == "table" {
            sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {}", quote_ident(&name)))
                .fetch_one(&mut conn)
                .await
                .ok()
                .map(|c| c.max(0) as u64)
        } else {
            None
        };
        out.push(database::TableInfo {
            size: sizes
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, s)| (*s).max(0) as u64)
                .unwrap_or(0),
            name,
            rows: count,
            engine: Some(kind),
            collation: None,
            comment: None,
            updated: None,
        });
    }
    let _ = conn.close().await;
    Ok(out)
}

// ===== Backups =====

/// One directory per file, named after its path.
pub fn backup_dir(data_dir: &Path, path: &Path) -> PathBuf {
    let name: String = path
        .display()
        .to_string()
        .trim_start_matches('/')
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    data_dir
        .join("backups")
        .join("database")
        .join("sqlite")
        .join(name)
}

pub async fn list_backups(
    pool: &SqlitePool,
    data_dir: &Path,
    raw: &str,
) -> Result<Vec<BackupFile>, String> {
    let path = registered_path(pool, raw, data_dir).await?;
    Ok(database::list_backups(&backup_dir(data_dir, &path)))
}

/// A consistent copy (`VACUUM INTO`), gzip-compressed; keeps the newest `keep`.
pub async fn backup(
    pool: &SqlitePool,
    data_dir: &Path,
    raw: &str,
    keep: Option<usize>,
) -> Result<PathBuf, String> {
    let path = registered_path(pool, raw, data_dir).await?;
    if !path.is_file() {
        return Err(format!("{} does not exist", path.display()));
    }
    let dir = backup_dir(data_dir, &path);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "database".into());
    let stem: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let snapshot = dir.join(format!(".{}_{}.tmp", stem, stamp));
    let target = dir.join(format!("{}_{}.sqlite.gz", stem, stamp));
    let _ = std::fs::remove_file(&snapshot);
    let mut conn = open_read_only(&path).await?;
    let vacuum = sqlx::query(&format!(
        "VACUUM INTO {}",
        quote_str(&snapshot.display().to_string())
    ))
    .execute(&mut conn)
    .await;
    let _ = conn.close().await;
    if let Err(err) = vacuum {
        let _ = std::fs::remove_file(&snapshot);
        return Err(format!("Backup failed: {}", err));
    }
    let snapshot_path = snapshot.clone();
    let target_path = target.clone();
    let result = tokio::task::spawn_blocking(move || gzip_file(&snapshot_path, &target_path))
        .await
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&snapshot);
    if let Err(err) = result {
        let _ = std::fs::remove_file(&target);
        return Err(err);
    }
    if let Some(keep) = keep.filter(|k| *k > 0) {
        for old in database::list_backups(&dir).into_iter().skip(keep) {
            let _ = std::fs::remove_file(old.path);
        }
    }
    Ok(target)
}

fn gzip_file(source: &Path, target: &Path) -> Result<(), String> {
    let mut input = std::fs::File::open(source).map_err(|e| e.to_string())?;
    let output = std::fs::File::create(target).map_err(|e| e.to_string())?;
    let mut encoder = flate2::write::GzEncoder::new(output, flate2::Compression::default());
    std::io::copy(&mut input, &mut encoder).map_err(|e| e.to_string())?;
    encoder
        .finish()
        .map_err(|e| e.to_string())?
        .flush()
        .map_err(|e| e.to_string())
}

/// Replaces the database with a backup. The previous file is kept next to
/// it as `<name>.before-restore` until the restore has succeeded.
pub async fn restore(
    pool: &SqlitePool,
    data_dir: &Path,
    raw: &str,
    file: &str,
) -> Result<(), String> {
    database::validate_backup_file(file)?;
    let path = registered_path(pool, raw, data_dir).await?;
    let source = backup_dir(data_dir, &path).join(file);
    if !source.is_file() {
        return Err("Backup not found".into());
    }
    let parent = path.parent().ok_or("Invalid path")?.to_path_buf();
    let staged = parent.join(format!(
        ".{}.linxpanel-restore",
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    ));
    let staged_path = staged.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let input = std::fs::File::open(&source).map_err(|e| e.to_string())?;
        let mut decoder = flate2::read::GzDecoder::new(input);
        let mut output = std::fs::File::create(&staged_path).map_err(|e| e.to_string())?;
        std::io::copy(&mut decoder, &mut output).map_err(|e| format!("Corrupt backup: {}", e))?;
        output.sync_all().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    if !has_header(&staged) {
        let _ = std::fs::remove_file(&staged);
        return Err("The backup is not an SQLite database".into());
    }
    // Check the copy before it replaces anything.
    let check = match open_read_only(&staged).await {
        Ok(mut conn) => {
            let result: Result<String, _> = sqlx::query_scalar("PRAGMA quick_check")
                .fetch_one(&mut conn)
                .await;
            let _ = conn.close().await;
            result.map_err(|e| e.to_string())
        }
        Err(err) => Err(err),
    };
    if check.as_deref() != Ok("ok") {
        let _ = std::fs::remove_file(&staged);
        return Err(format!(
            "The backup failed its integrity check: {}",
            check.unwrap_or_else(|e| e)
        ));
    }
    // VACUUM INTO writes a rollback-journal file; keep a WAL database in WAL
    // mode (the mode is stored in the file, and applications rely on it).
    let was_wal = match open_read_only(&path).await {
        Ok(mut conn) => {
            let mode: Option<String> = sqlx::query_scalar("PRAGMA journal_mode")
                .fetch_one(&mut conn)
                .await
                .ok();
            let _ = conn.close().await;
            mode.is_some_and(|m| m.eq_ignore_ascii_case("wal"))
        }
        Err(_) => false,
    };
    if was_wal {
        let converted = match SqliteConnectOptions::new()
            .filename(&staged)
            .journal_mode(SqliteJournalMode::Wal)
            .disable_statement_logging()
            .connect()
            .await
        {
            Ok(conn) => conn.close().await.map_err(|e| e.to_string()),
            Err(err) => Err(err.to_string()),
        };
        if let Err(err) = converted {
            let _ = std::fs::remove_file(&staged);
            return Err(format!("Cannot prepare the restored file: {}", err));
        }
    }
    if path.exists() {
        // Same owner and mode as the file it replaces.
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if let Ok(meta) = std::fs::metadata(&path) {
            let _ = std::os::unix::fs::chown(&staged, Some(meta.uid()), Some(meta.gid()));
            let _ = std::fs::set_permissions(
                &staged,
                std::fs::Permissions::from_mode(meta.mode() & 0o7777),
            );
        }
    } else {
        copy_owner(&parent, &staged);
    }
    // Stale -wal / -shm files would be replayed onto the restored file.
    for suffix in ["-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(format!("{}{}", path.display(), suffix));
    }
    std::fs::rename(&staged, &path).map_err(|e| {
        let _ = std::fs::remove_file(&staged);
        format!("Cannot replace {}: {}", path.display(), e)
    })
}

pub async fn delete_backup(
    pool: &SqlitePool,
    data_dir: &Path,
    raw: &str,
    file: &str,
) -> Result<(), String> {
    database::validate_backup_file(file)?;
    let path = registered_path(pool, raw, data_dir).await?;
    std::fs::remove_file(backup_dir(data_dir, &path).join(file)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_checked() {
        let data = std::env::temp_dir();
        assert!(check_path("relative.db", &data).is_err());
        assert!(check_path("/www/../etc/x.db", &data).is_err());
        assert!(check_path("/proc/self/x.db", &data).is_err());
        assert!(check_path(&data.join("x.db").display().to_string(), &data).is_err());
    }

    #[test]
    fn identifiers_are_quoted() {
        assert_eq!(quote_ident("a\"b"), "\"a\"\"b\"");
        assert_eq!(quote_str("it's"), "'it''s'");
    }

    #[test]
    fn backup_dirs_are_flat() {
        let dir = backup_dir(Path::new("/d"), Path::new("/www/wwwroot/a b/app.db"));
        assert_eq!(
            dir,
            PathBuf::from("/d/backups/database/sqlite/www_wwwroot_a_b_app.db")
        );
    }
}
