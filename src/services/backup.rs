//! Website backups (tar.gz of the site directory) and the command line that
//! scheduled backups run from cron. Database dumps live in `database`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::database::{list_backups, validate_backup_file, BackupFile};
use super::shell;

pub fn site_backup_dir(data_dir: &Path, site_id: i64) -> PathBuf {
    data_dir
        .join("backups")
        .join("site")
        .join(site_id.to_string())
}

pub fn list_site_backups(data_dir: &Path, site_id: i64) -> Vec<BackupFile> {
    list_backups(&site_backup_dir(data_dir, site_id))
}

fn file_stem_for(site_name: &str) -> String {
    let stem: String = site_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let stem = stem.trim_matches(['.', '_']).to_string();
    if stem.is_empty() {
        "site".into()
    } else {
        stem.chars().take(60).collect()
    }
}

fn split_doc_root(doc_root: &Path) -> Result<(String, String), String> {
    let parent = doc_root
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or("Invalid site directory")?;
    let name = doc_root.file_name().ok_or("Invalid site directory")?;
    Ok((
        parent.to_string_lossy().to_string(),
        name.to_string_lossy().to_string(),
    ))
}

/// Archives the site directory; keeps only the newest `keep` archives when given.
pub fn backup_site(
    data_dir: &Path,
    site_id: i64,
    site_name: &str,
    doc_root: &Path,
    keep: Option<usize>,
) -> Result<PathBuf, String> {
    if !doc_root.is_dir() {
        return Err(format!("Site directory not found: {}", doc_root.display()));
    }
    let dir = site_backup_dir(data_dir, site_id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let target = dir.join(format!("{}_{}.tar.gz", file_stem_for(site_name), stamp));
    let (parent, name) = split_doc_root(doc_root)?;
    let target_str = target.to_string_lossy().to_string();
    let output = shell::run_with_timeout(
        "tar",
        &["-czf", &target_str, "-C", &parent, &name],
        Duration::from_secs(3600),
    )?;
    if !output.ok() {
        let _ = std::fs::remove_file(&target);
        return Err(format!("Backup failed: {}", output.error_text()));
    }
    if let Some(keep) = keep.filter(|k| *k > 0) {
        for old in list_backups(&dir).into_iter().skip(keep) {
            let _ = std::fs::remove_file(old.path);
        }
    }
    Ok(target)
}

/// Replaces the site directory with an archive's contents. The current
/// directory is set aside first and put back if extraction fails.
pub fn restore_site(
    data_dir: &Path,
    site_id: i64,
    doc_root: &Path,
    file: &str,
) -> Result<(), String> {
    validate_backup_file(file)?;
    let archive = site_backup_dir(data_dir, site_id).join(file);
    if !archive.is_file() {
        return Err("Backup file not found".into());
    }
    let (parent, name) = split_doc_root(doc_root)?;
    // The archive must contain exactly this directory, not arbitrary paths.
    let listing = shell::run_with_timeout(
        "tar",
        &["-tzf", &archive.to_string_lossy()],
        Duration::from_secs(600),
    )?;
    if !listing.ok() {
        return Err(format!("Unreadable archive: {}", listing.error_text()));
    }
    let prefix = format!("{}/", name);
    let foreign = listing.stdout.lines().any(|entry| {
        let entry = entry.trim_start_matches("./");
        !(entry == name || entry.starts_with(&prefix)) || entry.split('/').any(|part| part == "..")
    });
    if foreign {
        return Err("The archive does not belong to this site directory".into());
    }

    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S");
    let aside = PathBuf::from(format!("{}.lp-restore-{}", doc_root.display(), stamp));
    if doc_root.exists() {
        std::fs::rename(doc_root, &aside)
            .map_err(|e| format!("Cannot move the current files aside: {}", e))?;
    }
    let output = shell::run_with_timeout(
        "tar",
        &["-xzpf", &archive.to_string_lossy(), "-C", &parent],
        Duration::from_secs(3600),
    );
    match output {
        Ok(output) if output.ok() => {
            let _ = std::fs::remove_dir_all(&aside);
            Ok(())
        }
        result => {
            let _ = std::fs::remove_dir_all(doc_root);
            let _ = std::fs::rename(&aside, doc_root);
            Err(match result {
                Ok(output) => format!("Restore failed: {}", output.error_text()),
                Err(err) => err,
            })
        }
    }
}

pub fn delete_site_backup(data_dir: &Path, site_id: i64, file: &str) -> Result<(), String> {
    validate_backup_file(file)?;
    std::fs::remove_file(site_backup_dir(data_dir, site_id).join(file)).map_err(|e| e.to_string())
}

/// What a cron job runs to call this binary's `backup` command against this
/// panel's database, e.g. `DATABASE_URL='sqlite:...' /opt/linxpanel/bin/linxpanel`.
pub fn cron_command_prefix() -> String {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "/opt/linxpanel/bin/linxpanel".into());
    match std::env::var("DATABASE_URL") {
        Ok(url) if !url.contains('\'') => format!("DATABASE_URL='{}' {}", url, exe),
        _ => exe,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_backup_roundtrip() {
        let base = std::env::temp_dir().join(format!("lp-site-{}", uuid::Uuid::new_v4()));
        let data = base.join("data");
        let doc_root = base.join("www").join("example.com");
        std::fs::create_dir_all(doc_root.join("sub")).unwrap();
        std::fs::write(doc_root.join("index.html"), "v1").unwrap();
        std::fs::write(doc_root.join("sub/a.txt"), "a").unwrap();

        let archive = backup_site(&data, 7, "example.com", &doc_root, Some(5)).unwrap();
        assert!(archive.to_string_lossy().contains("example.com_"));

        std::fs::write(doc_root.join("index.html"), "v2").unwrap();
        std::fs::write(doc_root.join("new.txt"), "new").unwrap();
        let file = archive.file_name().unwrap().to_string_lossy().to_string();
        restore_site(&data, 7, &doc_root, &file).unwrap();
        assert_eq!(
            std::fs::read_to_string(doc_root.join("index.html")).unwrap(),
            "v1"
        );
        assert!(!doc_root.join("new.txt").exists());
        assert!(doc_root.join("sub/a.txt").exists());

        // An archive of another directory is refused and leaves files untouched.
        let other = base.join("www").join("other.com");
        std::fs::create_dir_all(&other).unwrap();
        assert!(restore_site(&data, 7, &other, &file).is_err());
        assert!(restore_site(&data, 7, &doc_root, "../../etc/passwd").is_err());

        assert_eq!(list_site_backups(&data, 7).len(), 1);
        delete_site_backup(&data, 7, &file).unwrap();
        assert!(list_site_backups(&data, 7).is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn file_stems_are_safe() {
        assert_eq!(file_stem_for("blog.example.com"), "blog.example.com");
        assert_eq!(file_stem_for("My Site/1"), "My_Site_1");
        assert_eq!(file_stem_for("..."), "site");
    }
}
