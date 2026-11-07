//! Recycle bin for the file manager. A deleted item is renamed into a bin on
//! its own filesystem - instant and atomic whatever its size - and recorded in
//! the panel database, from where it can be restored to its original place or
//! removed for good. Items older than the retention period are purged.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::fileops;

pub const SETTING_KEY: &str = "recycle_bin";
const BIN_NAME: &str = ".Recycle_bin";
/// The usual bin, used for everything on the same filesystem as /www.
const DEFAULT_BIN: &str = "/www/.Recycle_bin";
/// Stop sizing a deleted folder after this many entries (the size shows as unknown).
const SIZE_SCAN_LIMIT: usize = 100_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    /// Items older than this are deleted for good; 0 keeps them until emptied.
    pub keep_days: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            keep_days: 30,
        }
    }
}

pub async fn load_settings(pool: &SqlitePool) -> Settings {
    crate::db::get_setting(pool, SETTING_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub async fn save_settings(pool: &SqlitePool, settings: &Settings) -> Result<(), String> {
    if settings.keep_days > 3650 {
        return Err("Keep at most 3650 days".into());
    }
    crate::db::set_setting(
        pool,
        SETTING_KEY,
        &serde_json::to_string(settings).unwrap_or_default(),
    )
    .await
    .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Item {
    pub id: i64,
    pub name: String,
    pub original: String,
    pub stored: String,
    pub is_dir: bool,
    /// Bytes; None when unknown (very large folders).
    pub size: Option<i64>,
    pub deleted_at: String,
}

/// Paths that are never deleted, with or without the bin.
pub fn is_protected(path: &Path) -> bool {
    const SYSTEM: &[&str] = &[
        "/",
        "/bin",
        "/boot",
        "/dev",
        "/etc",
        "/home",
        "/lib",
        "/lib32",
        "/lib64",
        "/libx32",
        "/media",
        "/mnt",
        "/opt",
        "/proc",
        "/root",
        "/run",
        "/sbin",
        "/snap",
        "/srv",
        "/sys",
        "/tmp",
        "/usr",
        "/var",
        "/www",
        "/www/server",
        "/www/wwwroot",
        "/www/backup",
        DEFAULT_BIN,
    ];
    let text = path.to_string_lossy();
    let trimmed = text.trim_end_matches('/');
    let normalized = if trimmed.is_empty() { "/" } else { trimmed };
    SYSTEM.contains(&normalized) || path.file_name().is_some_and(|n| n == BIN_NAME)
}

#[cfg(unix)]
fn device_of(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::symlink_metadata(path).ok().map(|m| m.dev())
}

#[cfg(not(unix))]
fn device_of(_path: &Path) -> Option<u64> {
    None
}

/// The bin on the same filesystem as `path`: /www/.Recycle_bin, or
/// .Recycle_bin at the root of the filesystem `path` lives on.
pub fn bin_for(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    let device = device_of(parent)?;
    let default_parent = Path::new(DEFAULT_BIN).parent()?;
    if device_of(default_parent) == Some(device) {
        return Some(PathBuf::from(DEFAULT_BIN));
    }
    // Walk up to the filesystem's root (the last ancestor on the same device).
    let mut root = parent.to_path_buf();
    while let Some(up) = root.parent() {
        if device_of(up) != Some(device) {
            break;
        }
        root = up.to_path_buf();
    }
    Some(root.join(BIN_NAME))
}

/// Whether `path` is inside a recycle bin (deleting there is permanent).
pub fn in_bin(path: &Path) -> bool {
    path.ancestors()
        .any(|a| a.file_name().is_some_and(|n| n == BIN_NAME))
}

fn size_of(path: &Path) -> Option<i64> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_dir() {
        return Some(meta.len() as i64);
    }
    let mut total: u64 = 0;
    let mut seen = 0usize;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).ok()?.flatten() {
            seen += 1;
            if seen > SIZE_SCAN_LIMIT {
                return None;
            }
            if let Ok(meta) = entry.metadata() {
                if meta.is_dir() && !meta.file_type().is_symlink() {
                    stack.push(entry.path());
                } else {
                    total += meta.len();
                }
            }
        }
    }
    Some(total as i64)
}

/// Deletes for good (a file, a link or a whole folder).
pub fn remove_now(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Moves `path` into its bin and records it. Blocking filesystem work runs on
/// a blocking thread.
pub async fn move_to_bin(pool: &SqlitePool, path: PathBuf) -> Result<(), String> {
    let bin = bin_for(&path).ok_or("Cannot find a recycle bin for this path")?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or("Invalid path")?;
    let stored = bin.join(format!("{}_{}", uuid::Uuid::new_v4().simple(), name));
    let (is_dir, size) = {
        let (path, stored, bin) = (path.clone(), stored.clone(), bin.clone());
        tokio::task::spawn_blocking(move || -> Result<(bool, Option<i64>), String> {
            if !bin.exists() {
                std::fs::create_dir_all(&bin).map_err(|e| format!("{}: {}", bin.display(), e))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700));
                }
            }
            let is_dir = std::fs::symlink_metadata(&path)
                .map(|m| m.is_dir())
                .map_err(|e| e.to_string())?;
            std::fs::rename(&path, &stored).map_err(|e| e.to_string())?;
            Ok((is_dir, size_of(&stored)))
        })
        .await
        .map_err(|e| e.to_string())??
    };
    let recorded = sqlx::query(
        "INSERT INTO recycle_bin (name, original, stored, is_dir, size) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&name)
    .bind(path.to_string_lossy().to_string())
    .bind(stored.to_string_lossy().to_string())
    .bind(is_dir)
    .bind(size)
    .execute(pool)
    .await;
    if let Err(err) = recorded {
        // Never leave an item in the bin that the list cannot show: put it back.
        let _ = std::fs::rename(&stored, &path);
        return Err(err.to_string());
    }
    Ok(())
}

/// Items in the bin, newest first. Records whose file has gone are dropped.
pub async fn list(pool: &SqlitePool) -> Result<Vec<Item>, String> {
    let items = sqlx::query_as::<_, Item>("SELECT * FROM recycle_bin ORDER BY id DESC")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    let (present, gone): (Vec<Item>, Vec<Item>) = items
        .into_iter()
        .partition(|item| std::fs::symlink_metadata(&item.stored).is_ok());
    for item in gone {
        let _ = sqlx::query("DELETE FROM recycle_bin WHERE id = ?")
            .bind(item.id)
            .execute(pool)
            .await;
    }
    Ok(present)
}

async fn get(pool: &SqlitePool, id: i64) -> Result<Item, String> {
    sqlx::query_as::<_, Item>("SELECT * FROM recycle_bin WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Item not found in the recycle bin".to_string())
}

/// Puts items back where they were. A name now taken gets " (2)" etc.
/// Returns the paths they were restored to.
pub async fn restore(pool: &SqlitePool, ids: &[i64]) -> Result<Vec<String>, String> {
    let mut restored = Vec::new();
    for id in ids {
        let item = get(pool, *id).await?;
        let original = PathBuf::from(&item.original);
        let stored = PathBuf::from(&item.stored);
        let target = tokio::task::spawn_blocking(move || -> Result<PathBuf, String> {
            let dir = original
                .parent()
                .ok_or("Invalid original path")?
                .to_path_buf();
            if !dir.exists() {
                fileops::create_dirs_like_parent(&dir)
                    .map_err(|e| format!("{}: {}", dir.display(), e))?;
            }
            let name = original
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let target = if std::fs::symlink_metadata(&original).is_ok() {
                fileops::free_name(&dir, &name)
            } else {
                original
            };
            std::fs::rename(&stored, &target).map_err(|e| e.to_string())?;
            Ok(target)
        })
        .await
        .map_err(|e| e.to_string())??;
        let _ = sqlx::query("DELETE FROM recycle_bin WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await;
        restored.push(target.to_string_lossy().to_string());
    }
    Ok(restored)
}

/// Deletes items from the bin for good.
pub async fn purge(pool: &SqlitePool, ids: &[i64]) -> Result<usize, String> {
    let mut count = 0;
    for id in ids {
        let item = match get(pool, *id).await {
            Ok(item) => item,
            Err(_) => continue,
        };
        let stored = PathBuf::from(&item.stored);
        tokio::task::spawn_blocking(move || remove_now(&stored))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("{}: {}", item.name, e))?;
        let _ = sqlx::query("DELETE FROM recycle_bin WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await;
        count += 1;
    }
    Ok(count)
}

pub async fn empty(pool: &SqlitePool) -> Result<usize, String> {
    let ids: Vec<i64> = sqlx::query_as::<_, (i64,)>("SELECT id FROM recycle_bin")
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(id,)| id)
        .collect();
    purge(pool, &ids).await
}

/// Purges items past the retention period.
pub async fn expire(pool: &SqlitePool) -> usize {
    let settings = load_settings(pool).await;
    if settings.keep_days == 0 {
        return 0;
    }
    let ids: Vec<i64> = sqlx::query_as::<_, (i64,)>(
        "SELECT id FROM recycle_bin WHERE deleted_at < datetime('now', ?)",
    )
    .bind(format!("-{} days", settings.keep_days))
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|(id,)| id)
    .collect();
    purge(pool, &ids).await.unwrap_or(0)
}

/// Background: purge expired items every few hours.
pub async fn run(pool: SqlitePool, mut shutdown: tokio::sync::watch::Receiver<bool>) {
    loop {
        let removed = expire(&pool).await;
        if removed > 0 {
            tracing::info!("Recycle bin: removed {} expired item(s)", removed);
        }
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)) => {}
            _ = shutdown.changed() => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_paths() {
        for p in [
            "/",
            "/etc",
            "/etc/",
            "/usr",
            "/www",
            "/www/wwwroot",
            "/root",
            "/www/.Recycle_bin",
            "/data/.Recycle_bin",
        ] {
            assert!(is_protected(Path::new(p)), "{p}");
        }
        for p in [
            "/www/wwwroot/site",
            "/etc/nginx/x.conf",
            "/tmp/a",
            "/root/file",
        ] {
            assert!(!is_protected(Path::new(p)), "{p}");
        }
        assert!(in_bin(Path::new("/www/.Recycle_bin/abc_x/y")));
        assert!(!in_bin(Path::new("/www/wwwroot/x")));
    }

    #[test]
    fn bin_is_on_the_same_filesystem() {
        let dir = std::env::temp_dir().join(format!("lp-rb-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        std::fs::write(&file, "x").unwrap();
        let bin = bin_for(&file).unwrap();
        assert_eq!(bin.file_name().unwrap(), BIN_NAME);
        // Same device as the file, so a rename works.
        let bin_parent = bin.parent().unwrap();
        assert_eq!(device_of(bin_parent), device_of(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
