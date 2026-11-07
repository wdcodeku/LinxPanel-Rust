//! Off-site copies of backups: S3-compatible object storage (AWS S3, Aliyun
//! OSS, Tencent COS, Cloudflare R2, MinIO...), WebDAV (NAS, Nextcloud,
//! Jianguoyun...) or FTP / FTPS. Transfers run through `curl` with its
//! configuration on stdin (credentials never on the command line); S3
//! requests are signed here with AWS Signature V4.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::notify::MASK;
use super::shell;

pub const SETTING_KEY: &str = "backup_remote";
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(6 * 3600);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    /// s3 | webdav | ftp
    pub kind: String,
    /// S3: https://s3.amazonaws.com, https://oss-cn-hangzhou.aliyuncs.com ...
    /// WebDAV: https://dav.example.com/remote.php/dav/files/me
    /// FTP: ftp://host[:port] or ftps://host
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    /// Bucket in the path (MinIO) instead of the host name.
    pub path_style: bool,
    pub access_key: String,
    pub secret_key: String,
    pub username: String,
    pub password: String,
    /// Folder (key prefix) for the panel's backups.
    pub prefix: String,
    /// Remote copies kept per backup job; 0 keeps all.
    pub keep: u32,
}

pub async fn load(pool: &SqlitePool) -> Config {
    crate::db::get_setting(pool, SETTING_KEY)
        .await
        .ok()
        .flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn masked(mut config: Config) -> Config {
    for field in [&mut config.secret_key, &mut config.password] {
        if !field.is_empty() {
            *field = MASK.to_string();
        }
    }
    config
}

/// Puts back stored secrets where the browser sent the mask.
pub async fn merge_secrets(pool: &SqlitePool, mut config: Config) -> Config {
    let stored = load(pool).await;
    if config.secret_key == MASK {
        config.secret_key = stored.secret_key;
    }
    if config.password == MASK {
        config.password = stored.password;
    }
    config
}

pub async fn save(pool: &SqlitePool, config: Config) -> Result<(), String> {
    let config = merge_secrets(pool, config).await;
    if config.enabled {
        validate(&config)?;
    }
    crate::db::set_setting(
        pool,
        SETTING_KEY,
        &serde_json::to_string(&config).map_err(|e| e.to_string())?,
    )
    .await
    .map_err(|e| e.to_string())
}

fn clean(value: &str) -> bool {
    !value.chars().any(|c| c.is_control()) && value.len() < 500
}

pub fn validate(c: &Config) -> Result<(), String> {
    let endpoint = c.endpoint.trim();
    let ok_scheme = match c.kind.as_str() {
        "s3" | "webdav" => endpoint.starts_with("https://") || endpoint.starts_with("http://"),
        "ftp" => endpoint.starts_with("ftp://") || endpoint.starts_with("ftps://"),
        _ => return Err("Choose a storage type".into()),
    };
    if !ok_scheme || endpoint.contains(' ') || !clean(endpoint) {
        return Err(match c.kind.as_str() {
            "ftp" => "Enter the server as ftp://host or ftps://host",
            _ => "Enter the endpoint as https://...",
        }
        .into());
    }
    if c.kind == "s3" {
        if c.bucket.trim().is_empty() || c.access_key.trim().is_empty() || c.secret_key.is_empty() {
            return Err("Bucket, access key and secret key are required".into());
        }
        if !c
            .bucket
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '.' || ch == '_')
        {
            return Err("Invalid bucket name".into());
        }
    }
    for v in [
        &c.region,
        &c.bucket,
        &c.access_key,
        &c.secret_key,
        &c.username,
        &c.password,
        &c.prefix,
    ] {
        if !clean(v) {
            return Err("Invalid characters in the storage settings".into());
        }
    }
    if c.prefix.split('/').any(|seg| seg == "..") {
        return Err("The folder may not contain ..".into());
    }
    Ok(())
}

/// Remote object key for a backup file: prefix/hostname/group/file.
pub fn object_key(c: &Config, group: &str, file_name: &str) -> String {
    let host = sysinfo::System::host_name().unwrap_or_else(|| "server".into());
    let mut parts: Vec<String> = c
        .prefix
        .split('/')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    parts.push(host);
    parts.push(group.to_string());
    parts.push(file_name.to_string());
    parts.join("/")
}

// ===== AWS Signature V4 =====

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn sha256_hex(data: &[u8]) -> String {
    hex(ring::digest::digest(&ring::digest::SHA256, data).as_ref())
}

fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key);
    ring::hmac::sign(&key, data.as_bytes()).as_ref().to_vec()
}

/// RFC 3986 encoding as S3 wants it ('/' kept in paths).
pub fn uri_encode(text: &str, keep_slash: bool) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) || (keep_slash && b == b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

/// The Authorization header for one request. `headers` are lower-case
/// (name, value) pairs including host, x-amz-date and x-amz-content-sha256.
#[allow(clippy::too_many_arguments)]
pub fn sigv4_authorization(
    method: &str,
    canonical_uri: &str,
    canonical_query: &str,
    headers: &[(&str, String)],
    payload_hash: &str,
    amz_date: &str,
    region: &str,
    access_key: &str,
    secret_key: &str,
) -> String {
    let mut sorted: Vec<&(&str, String)> = headers.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    let canonical_headers: String = sorted
        .iter()
        .map(|(k, v)| format!("{}:{}\n", k, v.trim()))
        .collect();
    let signed_headers = sorted.iter().map(|(k, _)| *k).collect::<Vec<_>>().join(";");
    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        method, canonical_uri, canonical_query, canonical_headers, signed_headers, payload_hash
    );
    let date = &amz_date[..8];
    let scope = format!("{}/{}/s3/aws4_request", date, region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        amz_date,
        scope,
        sha256_hex(canonical_request.as_bytes())
    );
    let k_date = hmac(format!("AWS4{}", secret_key).as_bytes(), date);
    let k_region = hmac(&k_date, region);
    let k_service = hmac(&k_region, "s3");
    let k_signing = hmac(&k_service, "aws4_request");
    let signature = hex(&hmac(&k_signing, &string_to_sign));
    format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        access_key, scope, signed_headers, signature
    )
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        ctx.update(&buf[..n]);
    }
    Ok(hex(ctx.finish().as_ref()))
}

/// (url, host, canonical uri) for an S3 object.
fn s3_target(c: &Config, key: &str) -> Result<(String, String, String), String> {
    let endpoint = c.endpoint.trim().trim_end_matches('/');
    let (scheme, rest) = endpoint.split_once("://").ok_or("Invalid endpoint")?;
    let host = rest.split('/').next().unwrap_or(rest).to_string();
    let bucket = c.bucket.trim();
    let encoded_key = uri_encode(key, true);
    if c.path_style {
        let uri = format!("/{}/{}", bucket, encoded_key);
        Ok((format!("{}://{}{}", scheme, host, uri), host, uri))
    } else {
        let host = format!("{}.{}", bucket, host);
        let uri = format!("/{}", encoded_key);
        Ok((format!("{}://{}{}", scheme, host, uri), host, uri))
    }
}

fn s3_region(c: &Config) -> String {
    match c.region.trim() {
        "" => "us-east-1".into(),
        r => r.to_string(),
    }
}

/// Runs curl with `config` on stdin; HTTP errors fail with the body's start.
fn curl(config: &str, input_file: Option<&Path>) -> Result<String, String> {
    let mut config = format!("silent\nshow-error\nfail\nconnect-timeout = 20\n{}", config);
    if let Some(file) = input_file {
        config.push_str(&format!(
            "upload-file = {}\n",
            quoted(&file.to_string_lossy())
        ));
    }
    let output = shell::run_with_input("curl", &["-K", "-"], Some(&config), TRANSFER_TIMEOUT)?;
    if output.ok() {
        Ok(output.stdout)
    } else {
        let detail = format!("{} {}", output.stderr.trim(), output.stdout.trim());
        Err(detail.chars().take(400).collect())
    }
}

fn quoted(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    )
}

fn s3_request(c: &Config, method: &str, key: &str, file: Option<&Path>) -> Result<String, String> {
    let (url, host, uri) = s3_target(c, key)?;
    let payload_hash = match file {
        Some(path) => file_sha256(path)?,
        None => sha256_hex(b""),
    };
    let amz_date = chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let headers = vec![
        ("host", host),
        ("x-amz-content-sha256", payload_hash.clone()),
        ("x-amz-date", amz_date.clone()),
    ];
    let authorization = sigv4_authorization(
        method,
        &uri,
        "",
        &headers,
        &payload_hash,
        &amz_date,
        &s3_region(c),
        c.access_key.trim(),
        &c.secret_key,
    );
    let mut config = format!(
        "url = {}\nrequest = {}\nheader = {}\nheader = {}\nheader = {}\n",
        quoted(&url),
        quoted(method),
        quoted(&format!("Authorization: {}", authorization)),
        quoted(&format!("x-amz-date: {}", amz_date)),
        quoted(&format!("x-amz-content-sha256: {}", payload_hash)),
    );
    if file.is_none() && method == "PUT" {
        config.push_str("data-binary = \"\"\n");
    }
    curl(&config, file)
}

fn webdav_url(c: &Config, key: &str) -> String {
    format!(
        "{}/{}",
        c.endpoint.trim().trim_end_matches('/'),
        uri_encode(key, true)
    )
}

fn auth_line(c: &Config) -> String {
    if c.username.is_empty() {
        String::new()
    } else {
        format!(
            "user = {}\n",
            quoted(&format!("{}:{}", c.username, c.password))
        )
    }
}

fn webdav_mkcols(c: &Config, key: &str) {
    // Create each parent collection; "already exists" answers are fine.
    let mut path = String::new();
    let parts: Vec<&str> = key.split('/').collect();
    for seg in &parts[..parts.len().saturating_sub(1)] {
        path = if path.is_empty() {
            seg.to_string()
        } else {
            format!("{}/{}", path, seg)
        };
        let config = format!(
            "url = {}\nrequest = \"MKCOL\"\n{}",
            quoted(&format!("{}/", webdav_url(c, &path))),
            auth_line(c)
        );
        let _ = shell::run_with_input(
            "curl",
            &["-K", "-"],
            Some(&format!("silent\n{}", config)),
            Duration::from_secs(60),
        );
    }
}

fn ftp_url(c: &Config, key: &str) -> String {
    format!(
        "{}/{}",
        c.endpoint.trim().trim_end_matches('/'),
        uri_encode(key, true)
    )
}

/// Uploads `file` as `key`. Blocking.
pub fn upload(c: &Config, file: &Path, key: &str) -> Result<(), String> {
    validate(c)?;
    match c.kind.as_str() {
        "s3" => s3_request(c, "PUT", key, Some(file)).map(|_| ()),
        "webdav" => {
            webdav_mkcols(c, key);
            curl(
                &format!("url = {}\n{}", quoted(&webdav_url(c, key)), auth_line(c)),
                Some(file),
            )
            .map(|_| ())
        }
        "ftp" => {
            let tls = if c.endpoint.starts_with("ftps://") {
                ""
            } else {
                "ssl\n"
            };
            curl(
                &format!(
                    "url = {}\nftp-create-dirs\n{}{}",
                    quoted(&ftp_url(c, key)),
                    tls,
                    auth_line(c)
                ),
                Some(file),
            )
            .map(|_| ())
        }
        _ => Err("Choose a storage type".into()),
    }
}

/// Deletes `key` remotely. Blocking.
pub fn delete(c: &Config, key: &str) -> Result<(), String> {
    match c.kind.as_str() {
        "s3" => s3_request(c, "DELETE", key, None).map(|_| ()),
        "webdav" => curl(
            &format!(
                "url = {}\nrequest = \"DELETE\"\n{}",
                quoted(&webdav_url(c, key)),
                auth_line(c)
            ),
            None,
        )
        .map(|_| ()),
        "ftp" => {
            // Quote commands run right after login, in the login directory,
            // so the path is given relative to it; no transfer follows.
            let base = c.endpoint.trim().trim_end_matches('/');
            let tls = if c.endpoint.starts_with("ftps://") {
                ""
            } else {
                "ssl\n"
            };
            curl(
                &format!(
                    "url = {}\nnobody\nquote = {}\n{}{}",
                    quoted(&format!("{}/", base)),
                    quoted(&format!("DELE {}", key)),
                    tls,
                    auth_line(c)
                ),
                None,
            )
            .map(|_| ())
        }
        _ => Err("Choose a storage type".into()),
    }
}

/// Uploads a small file and deletes it again: proves the settings work.
pub fn test(c: &Config) -> Result<String, String> {
    validate(c)?;
    let dir = std::env::temp_dir();
    let file = dir.join(format!(
        "linxpanel-test-{}.txt",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::write(&file, "LinxPanel remote backup test\n").map_err(|e| e.to_string())?;
    let key = object_key(
        c,
        "_test",
        &file.file_name().unwrap_or_default().to_string_lossy(),
    );
    let result = upload(c, &file, &key).and_then(|_| delete(c, &key));
    let _ = std::fs::remove_file(&file);
    result.map(|_| key)
}

/// After a scheduled backup: upload it, record it, apply remote retention.
pub async fn after_backup(
    pool: &SqlitePool,
    group: &str,
    file: &Path,
) -> Result<Option<String>, String> {
    let config = load(pool).await;
    if !config.enabled {
        return Ok(None);
    }
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .ok_or("Invalid backup file")?;
    let key = object_key(&config, group, &name);
    let size = std::fs::metadata(file).map(|m| m.len() as i64).unwrap_or(0);
    {
        let (config, file, key) = (config.clone(), file.to_path_buf(), key.clone());
        tokio::task::spawn_blocking(move || upload(&config, &file, &key))
            .await
            .map_err(|e| e.to_string())??;
    }
    let _ = sqlx::query(
        "INSERT INTO remote_backups (kind, group_key, object_key, size) VALUES (?, ?, ?, ?)",
    )
    .bind(&config.kind)
    .bind(group)
    .bind(&key)
    .bind(size)
    .execute(pool)
    .await;
    if config.keep > 0 {
        let old: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id, object_key FROM remote_backups WHERE group_key = ? AND kind = ? ORDER BY id DESC LIMIT -1 OFFSET ?",
        )
        .bind(group)
        .bind(&config.kind)
        .bind(i64::from(config.keep))
        .fetch_all(pool)
        .await
        .unwrap_or_default();
        for (id, old_key) in old {
            let (c, k) = (config.clone(), old_key.clone());
            let removed = tokio::task::spawn_blocking(move || delete(&c, &k)).await;
            if matches!(removed, Ok(Ok(()))) {
                let _ = sqlx::query("DELETE FROM remote_backups WHERE id = ?")
                    .bind(id)
                    .execute(pool)
                    .await;
            }
        }
    }
    Ok(Some(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sigv4_matches_the_aws_example() {
        // "GET Object" example from the AWS S3 Signature Version 4 documentation.
        let empty = sha256_hex(b"");
        assert_eq!(
            empty,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let headers = vec![
            ("host", "examplebucket.s3.amazonaws.com".to_string()),
            ("range", "bytes=0-9".to_string()),
            ("x-amz-content-sha256", empty.clone()),
            ("x-amz-date", "20130524T000000Z".to_string()),
        ];
        let auth = sigv4_authorization(
            "GET",
            "/test.txt",
            "",
            &headers,
            &empty,
            "20130524T000000Z",
            "us-east-1",
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        );
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, \
             SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, \
             Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    #[test]
    fn targets_and_keys() {
        let mut c = Config {
            kind: "s3".into(),
            endpoint: "https://oss-cn-hangzhou.aliyuncs.com".into(),
            bucket: "bk".into(),
            ..Default::default()
        };
        let (url, host, uri) = s3_target(&c, "a b/文件.tar.gz").unwrap();
        assert_eq!(host, "bk.oss-cn-hangzhou.aliyuncs.com");
        assert_eq!(uri, "/a%20b/%E6%96%87%E4%BB%B6.tar.gz");
        assert_eq!(
            url,
            "https://bk.oss-cn-hangzhou.aliyuncs.com/a%20b/%E6%96%87%E4%BB%B6.tar.gz"
        );
        c.path_style = true;
        c.endpoint = "http://127.0.0.1:9000/".into();
        assert_eq!(s3_target(&c, "k").unwrap().0, "http://127.0.0.1:9000/bk/k");
        c.prefix = "/backups//panel/".into();
        let key = object_key(&c, "site-3", "x.tar.gz");
        assert!(key.starts_with("backups/panel/") && key.ends_with("/site-3/x.tar.gz"));
    }

    #[test]
    fn validation() {
        let c = Config {
            kind: "ftp".into(),
            endpoint: "http://x".into(),
            ..Default::default()
        };
        assert!(validate(&c).is_err());
        let c = Config {
            kind: "webdav".into(),
            endpoint: "https://dav.example.com/x".into(),
            prefix: "a/../b".into(),
            ..Default::default()
        };
        assert!(validate(&c).is_err());
        let c = Config {
            kind: "s3".into(),
            endpoint: "https://s3.amazonaws.com".into(),
            bucket: "b".into(),
            access_key: "a".into(),
            secret_key: "s".into(),
            ..Default::default()
        };
        assert!(validate(&c).is_ok());
    }
}
