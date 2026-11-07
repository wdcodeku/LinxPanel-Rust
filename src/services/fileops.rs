//! File manager operations on more than one file: extracting and creating
//! archives, copying and moving, and giving new files the owner of the
//! directory they land in (so a website's own user can still change what
//! the panel - running as root - put there).
//!
//! Archives are read with the zip / tar crates where possible; the formats
//! those cannot read (xz, bzip2, zstd, 7z, rar) go to the system's tools.
//! Entries that would land outside the destination (`../`, absolute paths,
//! links pointing out) are skipped.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use super::{shell, tasks};

/// How often (in entries) a progress line goes to the task window.
const PROGRESS_EVERY: usize = 500;
/// Permission bits archives may set (never setuid / setgid / sticky).
const MODE_MASK: u32 = 0o777;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    TarZst,
    Gz,
    SevenZip,
    Rar,
}

impl ArchiveKind {
    pub fn of(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        let ends = |suffixes: &[&str]| suffixes.iter().any(|s| lower.ends_with(s));
        Some(if ends(&[".tar.gz", ".tgz"]) {
            Self::TarGz
        } else if ends(&[".tar.bz2", ".tbz2", ".tbz"]) {
            Self::TarBz2
        } else if ends(&[".tar.xz", ".txz"]) {
            Self::TarXz
        } else if ends(&[".tar.zst", ".tzst"]) {
            Self::TarZst
        } else if ends(&[".tar"]) {
            Self::Tar
        } else if ends(&[".zip", ".jar", ".war"]) {
            Self::Zip
        } else if ends(&[".gz"]) {
            Self::Gz
        } else if ends(&[".7z"]) {
            Self::SevenZip
        } else if ends(&[".rar"]) {
            Self::Rar
        } else {
            return None;
        })
    }
}

// ===== Ownership =====

#[cfg(unix)]
fn owner_of(path: &Path) -> Option<(u32, u32)> {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).ok().map(|m| (m.uid(), m.gid()))
}

/// Gives `path` (and everything below it) the owner of `dir`, when that is
/// not the panel's own user. Links are changed themselves, never followed.
pub fn inherit_owner(path: &Path, dir: &Path) {
    #[cfg(unix)]
    {
        let Some((uid, gid)) = owner_of(dir) else {
            return;
        };
        if !shell::is_root() || (uid == 0 && gid == 0) {
            return;
        }
        let mut stack = vec![path.to_path_buf()];
        while let Some(current) = stack.pop() {
            let _ = std::os::unix::fs::lchown(&current, Some(uid), Some(gid));
            let is_dir = fs::symlink_metadata(&current)
                .map(|m| m.is_dir())
                .unwrap_or(false);
            if is_dir {
                if let Ok(entries) = fs::read_dir(&current) {
                    stack.extend(entries.flatten().map(|e| e.path()));
                }
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (path, dir);
    }
}

/// Creates `dir` and its missing parents, each owned like the nearest
/// existing ancestor.
pub fn create_dirs_like_parent(dir: &Path) -> io::Result<()> {
    let mut missing = Vec::new();
    let mut current = dir;
    while !current.exists() {
        missing.push(current.to_path_buf());
        match current.parent() {
            Some(parent) => current = parent,
            None => break,
        }
    }
    let anchor = current.to_path_buf();
    fs::create_dir_all(dir)?;
    for created in missing.iter().rev() {
        inherit_owner_single(created, &anchor);
    }
    Ok(())
}

fn inherit_owner_single(path: &Path, dir: &Path) {
    #[cfg(unix)]
    if let Some((uid, gid)) = owner_of(dir) {
        if shell::is_root() && !(uid == 0 && gid == 0) {
            let _ = std::os::unix::fs::lchown(path, Some(uid), Some(gid));
        }
    }
    #[cfg(not(unix))]
    let _ = (path, dir);
}

// ===== Paths =====

/// `rel` joined to `base` when it stays inside: no absolute paths, no `..`.
fn inside(base: &Path, rel: &Path) -> Option<PathBuf> {
    let mut out = base.to_path_buf();
    for part in rel.components() {
        match part {
            Component::Normal(name) => out.push(name),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (out != base).then_some(out)
}

/// The parent of `target` must resolve inside `root` (an existing symlinked
/// directory inside the destination could otherwise point anywhere).
fn parent_stays_inside(root: &Path, target: &Path) -> bool {
    let Some(parent) = target.parent() else {
        return false;
    };
    match (fs::canonicalize(parent), fs::canonicalize(root)) {
        (Ok(parent), Ok(root)) => parent.starts_with(root),
        _ => false,
    }
}

/// The first path component of an archive entry (to fix ownership after).
fn top_level(rel: &Path) -> Option<String> {
    rel.components().find_map(|c| match c {
        Component::Normal(name) => Some(name.to_string_lossy().to_string()),
        _ => None,
    })
}

// ===== Extract =====

pub struct ExtractSummary {
    pub files: usize,
    pub skipped: usize,
}

/// Extracts `archive` into `dest` (created when missing). Existing files
/// are replaced when `overwrite`, otherwise kept.
pub fn extract(archive: &Path, dest: &Path, overwrite: bool) -> Result<ExtractSummary, String> {
    let name = archive
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let kind = ArchiveKind::of(&name).ok_or("This file type cannot be extracted")?;
    create_dirs_like_parent(dest).map_err(|e| format!("{}: {}", dest.display(), e))?;
    tasks::note(&format!(
        "[LinxPanel] Extracting {} into {}",
        name,
        dest.display()
    ));
    let mut tops = std::collections::BTreeSet::new();
    let summary = match kind {
        ArchiveKind::Zip => extract_zip(archive, dest, overwrite, &mut tops)?,
        ArchiveKind::Tar => {
            let file = File::open(archive).map_err(|e| e.to_string())?;
            extract_tar(BufReader::new(file), dest, overwrite, &mut tops)?
        }
        ArchiveKind::TarGz => {
            let file = File::open(archive).map_err(|e| e.to_string())?;
            let decoder = flate2::read::MultiGzDecoder::new(BufReader::new(file));
            extract_tar(decoder, dest, overwrite, &mut tops)?
        }
        ArchiveKind::Gz => extract_gz(archive, dest, overwrite, &mut tops)?,
        ArchiveKind::TarBz2 | ArchiveKind::TarXz | ArchiveKind::TarZst => {
            extract_with_tar_tool(archive, dest, overwrite, &mut tops)?
        }
        ArchiveKind::SevenZip | ArchiveKind::Rar => {
            extract_with_7z(archive, dest, overwrite, kind, &mut tops)?
        }
    };
    for top in &tops {
        inherit_owner(&dest.join(top), dest);
    }
    tasks::note(&format!(
        "[LinxPanel] Done: {} files{}",
        summary.files,
        if summary.skipped > 0 {
            format!(", {} skipped", summary.skipped)
        } else {
            String::new()
        }
    ));
    Ok(summary)
}

fn extract_zip(
    archive: &Path,
    dest: &Path,
    overwrite: bool,
    tops: &mut std::collections::BTreeSet<String>,
) -> Result<ExtractSummary, String> {
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|e| format!("Not a valid zip archive: {}", e))?;
    let mut summary = ExtractSummary {
        files: 0,
        skipped: 0,
    };
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
        let Some(rel) = entry.enclosed_name() else {
            tasks::note(&format!(
                "[LinxPanel] Skipped unsafe path: {}",
                entry.name()
            ));
            summary.skipped += 1;
            continue;
        };
        let Some(target) = inside(dest, &rel) else {
            summary.skipped += 1;
            continue;
        };
        if let Some(top) = top_level(&rel) {
            tops.insert(top);
        }
        let mode = entry.unix_mode();
        // Links in a zip are rare and can point anywhere: not recreated.
        if mode.is_some_and(|m| m & 0o170000 == 0o120000) {
            summary.skipped += 1;
            continue;
        }
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| format!("{}: {}", target.display(), e))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {}", parent.display(), e))?;
        }
        if !parent_stays_inside(dest, &target) {
            summary.skipped += 1;
            continue;
        }
        if target.exists() && !overwrite {
            summary.skipped += 1;
            continue;
        }
        if fs::symlink_metadata(&target).is_ok_and(|m| m.file_type().is_symlink()) {
            let _ = fs::remove_file(&target);
        }
        let mut out = File::create(&target).map_err(|e| format!("{}: {}", target.display(), e))?;
        io::copy(&mut entry, &mut out).map_err(|e| format!("{}: {}", target.display(), e))?;
        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&target, fs::Permissions::from_mode(mode & MODE_MASK));
        }
        summary.files += 1;
        if summary.files % PROGRESS_EVERY == 0 {
            tasks::note(&format!("[LinxPanel] {} files ...", summary.files));
        }
    }
    Ok(summary)
}

fn extract_tar<R: Read>(
    reader: R,
    dest: &Path,
    overwrite: bool,
    tops: &mut std::collections::BTreeSet<String>,
) -> Result<ExtractSummary, String> {
    let mut archive = tar::Archive::new(reader);
    archive.set_preserve_permissions(true);
    archive.set_preserve_ownerships(false);
    archive.set_unpack_xattrs(false);
    archive.set_overwrite(overwrite);
    let mut summary = ExtractSummary {
        files: 0,
        skipped: 0,
    };
    let entries = archive
        .entries()
        .map_err(|e| format!("Not a valid tar archive: {}", e))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("Broken archive: {}", e))?;
        let rel = entry.path().map(|p| p.into_owned()).unwrap_or_default();
        if let Some(top) = top_level(&rel) {
            tops.insert(top);
        }
        let target = inside(dest, &rel);
        if target.as_ref().is_some_and(|t| t.exists())
            && !overwrite
            && !entry.header().entry_type().is_dir()
        {
            summary.skipped += 1;
            continue;
        }
        // unpack_in refuses paths leaving `dest`, also through links.
        match entry.unpack_in(dest) {
            Ok(true) => {
                if !entry.header().entry_type().is_dir() {
                    summary.files += 1;
                }
            }
            Ok(false) => {
                tasks::note(&format!(
                    "[LinxPanel] Skipped unsafe path: {}",
                    rel.display()
                ));
                summary.skipped += 1;
            }
            Err(err) => return Err(format!("{}: {}", rel.display(), err)),
        }
        if summary.files > 0 && summary.files % PROGRESS_EVERY == 0 {
            tasks::note(&format!("[LinxPanel] {} files ...", summary.files));
        }
    }
    #[cfg(unix)]
    clear_special_bits(dest, tops);
    Ok(summary)
}

/// Archives may carry setuid / setgid bits; those never survive extraction.
#[cfg(unix)]
fn clear_special_bits(dest: &Path, tops: &std::collections::BTreeSet<String>) {
    use std::os::unix::fs::PermissionsExt;
    let mut stack: Vec<PathBuf> = tops.iter().map(|t| dest.join(t)).collect();
    while let Some(path) = stack.pop() {
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        let mode = meta.permissions().mode();
        if mode & 0o7000 != 0 {
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(mode & MODE_MASK));
        }
        if meta.is_dir() {
            if let Ok(entries) = fs::read_dir(&path) {
                stack.extend(entries.flatten().map(|e| e.path()));
            }
        }
    }
}

fn extract_gz(
    archive: &Path,
    dest: &Path,
    overwrite: bool,
    tops: &mut std::collections::BTreeSet<String>,
) -> Result<ExtractSummary, String> {
    let name = archive
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let out_name = name
        .strip_suffix(".gz")
        .or_else(|| name.strip_suffix(".GZ"))
        .filter(|n| !n.is_empty())
        .unwrap_or("extracted");
    let target = dest.join(out_name);
    if target.exists() && !overwrite {
        return Ok(ExtractSummary {
            files: 0,
            skipped: 1,
        });
    }
    let file = File::open(archive).map_err(|e| e.to_string())?;
    let mut decoder = flate2::read::MultiGzDecoder::new(BufReader::new(file));
    let mut out = File::create(&target).map_err(|e| format!("{}: {}", target.display(), e))?;
    io::copy(&mut decoder, &mut out).map_err(|e| format!("Not a valid gzip file: {}", e))?;
    tops.insert(out_name.to_string());
    Ok(ExtractSummary {
        files: 1,
        skipped: 0,
    })
}

/// xz / bzip2 / zstd tarballs: GNU tar with the matching compressor.
fn extract_with_tar_tool(
    archive: &Path,
    dest: &Path,
    overwrite: bool,
    tops: &mut std::collections::BTreeSet<String>,
) -> Result<ExtractSummary, String> {
    if !shell::has_binary("tar") {
        return Err("The tar command is not installed".into());
    }
    // List first: names for ownership, and the file count.
    let listing = shell::run_with_timeout(
        "tar",
        &["-tf", &archive.to_string_lossy()],
        Duration::from_secs(1800),
    )?;
    if !listing.ok() {
        return Err(format!(
            "Cannot read the archive (is xz / bzip2 / zstd installed?): {}",
            listing.error_text()
        ));
    }
    let mut files = 0;
    for line in listing.stdout.lines() {
        let rel = Path::new(line.trim_start_matches("./"));
        if let Some(top) = top_level(rel) {
            tops.insert(top);
        }
        if !line.ends_with('/') {
            files += 1;
        }
    }
    let mut args = vec![
        "-xf".to_string(),
        archive.to_string_lossy().to_string(),
        "-C".to_string(),
        dest.to_string_lossy().to_string(),
        "--no-same-owner".to_string(),
        "--no-same-permissions".to_string(),
    ];
    if !overwrite {
        args.push("--skip-old-files".into());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = shell::run_with_timeout("tar", &refs, Duration::from_secs(4 * 3600))?;
    if !output.ok() {
        return Err(format!("Extraction failed: {}", output.error_text()));
    }
    Ok(ExtractSummary { files, skipped: 0 })
}

/// 7z and rar: 7z (p7zip) when installed, unrar for rar otherwise.
fn extract_with_7z(
    archive: &Path,
    dest: &Path,
    overwrite: bool,
    kind: ArchiveKind,
    tops: &mut std::collections::BTreeSet<String>,
) -> Result<ExtractSummary, String> {
    let before: std::collections::BTreeSet<String> = fs::read_dir(dest)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    let archive_text = archive.to_string_lossy().to_string();
    let output = if let Some(seven) = ["7z", "7za", "7zz"]
        .into_iter()
        .find(|b| shell::has_binary(b))
    {
        let out_flag = format!("-o{}", dest.display());
        let mode_flag = if overwrite { "-aoa" } else { "-aos" };
        shell::run_with_timeout(
            seven,
            &["x", "-y", mode_flag, &out_flag, &archive_text],
            Duration::from_secs(4 * 3600),
        )?
    } else if kind == ArchiveKind::Rar && shell::has_binary("unrar") {
        let dest_text = format!("{}/", dest.display());
        let mode_flag = if overwrite { "-o+" } else { "-o-" };
        shell::run_with_timeout(
            "unrar",
            &["x", "-y", mode_flag, &archive_text, &dest_text],
            Duration::from_secs(4 * 3600),
        )?
    } else {
        return Err(if kind == ArchiveKind::Rar {
            "Extracting .rar needs 7-Zip or unrar: install p7zip-full (or unrar) first".into()
        } else {
            "Extracting .7z needs 7-Zip: install p7zip-full (Debian / Ubuntu) or p7zip (CentOS) first".into()
        });
    };
    if !output.ok() {
        return Err(format!("Extraction failed: {}", output.error_text()));
    }
    let mut files = 0;
    if let Ok(rd) = fs::read_dir(dest) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !before.contains(&name) {
                tops.insert(name);
                files += 1;
            }
        }
    }
    // Links the tool created must not point outside the destination.
    #[cfg(unix)]
    {
        remove_escaping_links(dest, tops);
        clear_special_bits(dest, tops);
    }
    Ok(ExtractSummary { files, skipped: 0 })
}

#[cfg(unix)]
fn remove_escaping_links(dest: &Path, tops: &std::collections::BTreeSet<String>) {
    let Ok(root) = fs::canonicalize(dest) else {
        return;
    };
    let mut stack: Vec<PathBuf> = tops.iter().map(|t| dest.join(t)).collect();
    while let Some(path) = stack.pop() {
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            let target = fs::read_link(&path).unwrap_or_default();
            let resolved = path.parent().unwrap_or(dest).join(&target);
            let ok = fs::canonicalize(&resolved).is_ok_and(|r| r.starts_with(&root));
            if !ok {
                let _ = fs::remove_file(&path);
                tasks::note(&format!(
                    "[LinxPanel] Removed a link pointing outside: {}",
                    path.display()
                ));
            }
        } else if meta.is_dir() {
            if let Ok(entries) = fs::read_dir(&path) {
                stack.extend(entries.flatten().map(|e| e.path()));
            }
        }
    }
}

// ===== Compress =====

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressFormat {
    Zip,
    TarGz,
}

impl CompressFormat {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "zip" => Ok(Self::Zip),
            "tar.gz" | "tgz" => Ok(Self::TarGz),
            other => Err(format!("Unsupported format: {}", other)),
        }
    }
}

/// Packs `names` (entries of `dir`) into `output`. Written to a temporary
/// file first, so a failure never leaves a broken archive.
pub fn compress(
    dir: &Path,
    names: &[String],
    output: &Path,
    format: CompressFormat,
) -> Result<usize, String> {
    if names.is_empty() {
        return Err("Nothing to compress".into());
    }
    for name in names {
        if name.is_empty() || name.contains('/') || name == "." || name == ".." {
            return Err(format!("Invalid name: {}", name));
        }
        if fs::symlink_metadata(dir.join(name)).is_err() {
            return Err(format!("Not found: {}", name));
        }
    }
    let parent = output.parent().ok_or("Invalid output path")?;
    let partial = parent.join(format!(
        ".{}.partial-{}",
        output
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        uuid::Uuid::new_v4().simple()
    ));
    tasks::note(&format!("[LinxPanel] Creating {}", output.display()));
    let result = match format {
        CompressFormat::Zip => compress_zip(dir, names, &partial, output),
        CompressFormat::TarGz => compress_tar_gz(dir, names, &partial, output),
    };
    match result {
        Ok(count) => {
            fs::rename(&partial, output).map_err(|e| {
                let _ = fs::remove_file(&partial);
                format!("{}: {}", output.display(), e)
            })?;
            inherit_owner(output, dir);
            tasks::note(&format!("[LinxPanel] Done: {} files", count));
            Ok(count)
        }
        Err(err) => {
            let _ = fs::remove_file(&partial);
            Err(err)
        }
    }
}

/// Every entry under `dir/name`, depth first, links not followed.
fn walk(dir: &Path, name: &str) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack = vec![(dir.join(name), name.to_string())];
    while let Some((path, rel)) = stack.pop() {
        let is_dir = fs::symlink_metadata(&path)
            .map(|m| m.is_dir())
            .unwrap_or(false);
        out.push((path.clone(), rel.clone()));
        if is_dir {
            if let Ok(entries) = fs::read_dir(&path) {
                let mut children: Vec<_> = entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect();
                children.sort();
                for child in children.into_iter().rev() {
                    stack.push((path.join(&child), format!("{}/{}", rel, child)));
                }
            }
        }
    }
    out
}

fn compress_zip(
    dir: &Path,
    names: &[String],
    partial: &Path,
    output: &Path,
) -> Result<usize, String> {
    use zip::write::SimpleFileOptions;
    let file = File::create(partial).map_err(|e| format!("{}: {}", partial.display(), e))?;
    let mut zip = zip::ZipWriter::new(BufWriter::new(file));
    let mut count = 0;
    let output_canon = fs::canonicalize(output.parent().unwrap_or(dir)).ok();
    for name in names {
        for (path, rel) in walk(dir, name) {
            // Never pack the archive being written.
            if path == partial || output_canon.as_ref().is_some_and(|_| path == output) {
                continue;
            }
            let Ok(meta) = fs::symlink_metadata(&path) else {
                continue;
            };
            #[cfg(unix)]
            let mode = {
                use std::os::unix::fs::PermissionsExt;
                meta.permissions().mode() & MODE_MASK
            };
            #[cfg(not(unix))]
            let mode = 0o644;
            if meta.file_type().is_symlink() {
                let target = fs::read_link(&path).unwrap_or_default();
                zip.add_symlink(rel, target.to_string_lossy(), SimpleFileOptions::default())
                    .map_err(|e| e.to_string())?;
                continue;
            }
            let options = SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(mode)
                .large_file(meta.len() >= u32::MAX as u64);
            if meta.is_dir() {
                zip.add_directory(format!("{}/", rel), options)
                    .map_err(|e| e.to_string())?;
                continue;
            }
            zip.start_file(rel.clone(), options)
                .map_err(|e| e.to_string())?;
            let mut input = File::open(&path).map_err(|e| format!("{}: {}", path.display(), e))?;
            io::copy(&mut input, &mut zip).map_err(|e| format!("{}: {}", path.display(), e))?;
            count += 1;
            if count % PROGRESS_EVERY == 0 {
                tasks::note(&format!("[LinxPanel] {} files ...", count));
            }
        }
    }
    let mut inner = zip.finish().map_err(|e| e.to_string())?;
    inner.flush().map_err(|e| e.to_string())?;
    Ok(count)
}

fn compress_tar_gz(
    dir: &Path,
    names: &[String],
    partial: &Path,
    output: &Path,
) -> Result<usize, String> {
    let file = File::create(partial).map_err(|e| format!("{}: {}", partial.display(), e))?;
    let encoder =
        flate2::write::GzEncoder::new(BufWriter::new(file), flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);
    builder.follow_symlinks(false);
    let mut count = 0;
    for name in names {
        for (path, rel) in walk(dir, name) {
            if path == partial || path == output {
                continue;
            }
            builder
                .append_path_with_name(&path, &rel)
                .map_err(|e| format!("{}: {}", path.display(), e))?;
            if fs::symlink_metadata(&path)
                .map(|m| m.is_file())
                .unwrap_or(false)
            {
                count += 1;
                if count % PROGRESS_EVERY == 0 {
                    tasks::note(&format!("[LinxPanel] {} files ...", count));
                }
            }
        }
    }
    let encoder = builder.into_inner().map_err(|e| e.to_string())?;
    let mut inner = encoder.finish().map_err(|e| e.to_string())?;
    inner.flush().map_err(|e| e.to_string())?;
    Ok(count)
}

// ===== Copy / move =====

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    /// Replace what is there.
    Overwrite,
    /// Leave the existing item, do not copy this one.
    Skip,
    /// Keep both: the new one gets "name (2)".
    Rename,
}

impl Conflict {
    pub fn parse(value: &str) -> Self {
        match value {
            "overwrite" => Self::Overwrite,
            "skip" => Self::Skip,
            _ => Self::Rename,
        }
    }
}

/// "name (2).ext" style alternative that does not exist yet.
pub(crate) fn free_name(dir: &Path, name: &str) -> PathBuf {
    let (stem, ext) = match name.rfind('.') {
        Some(at) if at > 0 => (&name[..at], &name[at..]),
        _ => (name, ""),
    };
    (2..)
        .map(|n| dir.join(format!("{} ({}){}", stem, n, ext)))
        .find(|p| fs::symlink_metadata(p).is_err())
        .unwrap_or_else(|| dir.join(name))
}

fn copy_tree(from: &Path, to: &Path, count: &mut usize) -> io::Result<()> {
    let meta = fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        let target = fs::read_link(from)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, to)?;
        return Ok(());
    }
    if meta.is_dir() {
        fs::create_dir_all(to)?;
        fs::set_permissions(to, meta.permissions())?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()), count)?;
        }
        return Ok(());
    }
    fs::copy(from, to)?;
    *count += 1;
    if *count % PROGRESS_EVERY == 0 {
        tasks::note(&format!("[LinxPanel] {} files ...", count));
    }
    Ok(())
}

fn remove_any(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

pub struct TransferSummary {
    pub done: usize,
    pub skipped: usize,
}

/// Copies (or moves) each source into `dest_dir`.
pub fn transfer(
    sources: &[PathBuf],
    dest_dir: &Path,
    move_items: bool,
    conflict: Conflict,
) -> Result<TransferSummary, String> {
    if !dest_dir.is_dir() {
        return Err(format!("{} is not a directory", dest_dir.display()));
    }
    let dest_canon = fs::canonicalize(dest_dir).map_err(|e| e.to_string())?;
    let mut summary = TransferSummary {
        done: 0,
        skipped: 0,
    };
    let mut files = 0;
    for source in sources {
        let meta =
            fs::symlink_metadata(source).map_err(|_| format!("Not found: {}", source.display()))?;
        let name = source
            .file_name()
            .ok_or_else(|| format!("Invalid source: {}", source.display()))?
            .to_string_lossy()
            .to_string();
        if meta.is_dir() {
            if let Ok(src_canon) = fs::canonicalize(source) {
                if dest_canon.starts_with(&src_canon) {
                    return Err(format!("Cannot put {} inside itself", source.display()));
                }
            }
        }
        let mut target = dest_dir.join(&name);
        let same_place = source
            .parent()
            .map(|p| fs::canonicalize(p).ok() == Some(dest_canon.clone()))
            .unwrap_or(false);
        if move_items && same_place {
            summary.skipped += 1;
            continue;
        }
        if fs::symlink_metadata(&target).is_ok() {
            match conflict {
                // Copying into its own folder always keeps both.
                _ if same_place => target = free_name(dest_dir, &name),
                Conflict::Skip => {
                    summary.skipped += 1;
                    continue;
                }
                Conflict::Rename => target = free_name(dest_dir, &name),
                Conflict::Overwrite => {
                    remove_any(&target).map_err(|e| format!("{}: {}", target.display(), e))?;
                }
            }
        }
        tasks::note(&format!(
            "[LinxPanel] {} {} -> {}",
            if move_items { "Move" } else { "Copy" },
            source.display(),
            target.display()
        ));
        if move_items {
            match fs::rename(source, &target) {
                Ok(()) => {}
                // Another filesystem: copy, then remove the original.
                Err(err) if err.raw_os_error() == Some(18) => {
                    copy_tree(source, &target, &mut files)
                        .map_err(|e| format!("{}: {}", source.display(), e))?;
                    remove_any(source).map_err(|e| format!("{}: {}", source.display(), e))?;
                }
                Err(err) => return Err(format!("{}: {}", source.display(), err)),
            }
        } else {
            copy_tree(source, &target, &mut files)
                .map_err(|e| format!("{}: {}", source.display(), e))?;
            inherit_owner(&target, dest_dir);
        }
        summary.done += 1;
    }
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lp-fileops-{}-{}",
            tag,
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn kinds_by_name() {
        assert_eq!(ArchiveKind::of("a.tar.gz"), Some(ArchiveKind::TarGz));
        assert_eq!(ArchiveKind::of("A.TGZ"), Some(ArchiveKind::TarGz));
        assert_eq!(ArchiveKind::of("a.zip"), Some(ArchiveKind::Zip));
        assert_eq!(ArchiveKind::of("a.tar.xz"), Some(ArchiveKind::TarXz));
        assert_eq!(ArchiveKind::of("dump.sql.gz"), Some(ArchiveKind::Gz));
        assert_eq!(ArchiveKind::of("a.txt"), None);
    }

    #[test]
    fn paths_stay_inside() {
        let base = Path::new("/d");
        assert_eq!(
            inside(base, Path::new("a/b")),
            Some(PathBuf::from("/d/a/b"))
        );
        assert_eq!(inside(base, Path::new("../x")), None);
        assert_eq!(inside(base, Path::new("/etc/passwd")), None);
        assert_eq!(inside(base, Path::new("")), None);
    }

    #[test]
    fn zip_and_tar_round_trip() {
        let src = temp_dir("src");
        fs::create_dir_all(src.join("site/sub")).unwrap();
        fs::write(src.join("site/index.php"), "<?php echo 1;").unwrap();
        fs::write(src.join("site/sub/a.txt"), "hello").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                src.join("site/sub/a.txt"),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        for (format, name) in [
            (CompressFormat::Zip, "site.zip"),
            (CompressFormat::TarGz, "site.tar.gz"),
        ] {
            let out = src.join(name);
            let count = compress(&src, &["site".to_string()], &out, format).unwrap();
            assert_eq!(count, 2);
            let dest = temp_dir("dest");
            let summary = extract(&out, &dest, true).unwrap();
            assert_eq!(summary.files, 2, "{}", name);
            assert_eq!(
                fs::read_to_string(dest.join("site/sub/a.txt")).unwrap(),
                "hello"
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = fs::metadata(dest.join("site/sub/a.txt"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777;
                assert_eq!(mode, 0o755, "{}", name);
            }
            // Not overwriting keeps the existing file.
            fs::write(dest.join("site/index.php"), "changed").unwrap();
            let again = extract(&out, &dest, false).unwrap();
            assert!(again.skipped >= 1, "{}", name);
            assert_eq!(
                fs::read_to_string(dest.join("site/index.php")).unwrap(),
                "changed"
            );
            let _ = fs::remove_dir_all(dest);
        }
        let _ = fs::remove_dir_all(src);
    }

    #[test]
    fn zip_slip_is_refused() {
        let dir = temp_dir("slip");
        let archive = dir.join("evil.zip");
        {
            let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("../escaped.txt", options).unwrap();
            zip.write_all(b"x").unwrap();
            zip.start_file("ok.txt", options).unwrap();
            zip.write_all(b"y").unwrap();
            zip.finish().unwrap();
        }
        let dest = dir.join("out");
        let summary = extract(&archive, &dest, true).unwrap();
        assert_eq!(summary.files, 1);
        assert!(summary.skipped >= 1);
        assert!(!dir.join("escaped.txt").exists());
        assert!(dest.join("ok.txt").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn copy_move_and_conflicts() {
        let dir = temp_dir("copy");
        fs::create_dir_all(dir.join("a/inner")).unwrap();
        fs::write(dir.join("a/inner/f.txt"), "1").unwrap();
        fs::create_dir_all(dir.join("dest")).unwrap();
        let summary =
            transfer(&[dir.join("a")], &dir.join("dest"), false, Conflict::Rename).unwrap();
        assert_eq!(summary.done, 1);
        assert!(dir.join("dest/a/inner/f.txt").exists());
        // Again: keeps both.
        transfer(&[dir.join("a")], &dir.join("dest"), false, Conflict::Rename).unwrap();
        assert!(dir.join("dest/a (2)/inner/f.txt").exists());
        // Skip leaves it.
        let skipped = transfer(&[dir.join("a")], &dir.join("dest"), false, Conflict::Skip).unwrap();
        assert_eq!(skipped.skipped, 1);
        // Copy into its own folder: a second copy.
        transfer(
            &[dir.join("a/inner/f.txt")],
            &dir.join("a/inner"),
            false,
            Conflict::Overwrite,
        )
        .unwrap();
        assert!(dir.join("a/inner/f (2).txt").exists());
        // Into itself is refused.
        assert!(transfer(
            &[dir.join("a")],
            &dir.join("a/inner"),
            true,
            Conflict::Rename
        )
        .is_err());
        // Move.
        transfer(
            &[dir.join("a")],
            &dir.join("dest/a"),
            true,
            Conflict::Rename,
        )
        .unwrap();
        assert!(!dir.join("a").exists());
        assert!(dir.join("dest/a/a/inner/f.txt").exists());
        let _ = fs::remove_dir_all(dir);
    }
}
