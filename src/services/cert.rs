//! TLS certificate management.
//!
//! Certificates live under `data/certs/<id>/` and are described by rows in the
//! `certificates` table. Metadata (subject, issuer, validity, SANs) is read back
//! from the PEM with `openssl`, which every target platform already ships, rather
//! than pulling in an X.509 parsing dependency.
//!
//! Issuance uses whichever ACME client is installed — acme.sh or certbot — in
//! webroot mode, so an existing site keeps serving while the challenge runs.

use std::path::{Path, PathBuf};

use chrono::{NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use super::shell;

/// Renew this many days before expiry.
pub const RENEW_WINDOW_DAYS: i64 = 30;
/// Flag a certificate as "expiring" in the UI at this threshold.
const EXPIRING_SOON_DAYS: i64 = 15;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct CertRecord {
    pub id: i64,
    pub name: String,
    /// Comma-separated domain list this certificate covers.
    pub domains: String,
    pub cert_path: String,
    pub key_path: String,
    pub issuer: String,
    pub subject: String,
    pub not_before: String,
    pub not_after: String,
    /// uploaded | self-signed | acme
    pub source: String,
    pub auto_renew: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CertView {
    #[serde(flatten)]
    pub record: CertRecord,
    /// Negative once expired.
    pub days_remaining: i64,
    /// valid | expiring | expired | unknown
    pub status: String,
    /// Sites currently using this certificate.
    pub used_by: Vec<String>,
}

/// Parsed straight out of the PEM.
#[derive(Debug, Clone, Default)]
pub struct CertMeta {
    pub subject: String,
    pub issuer: String,
    pub not_before: String,
    pub not_after: String,
    pub domains: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct UploadCertRequest {
    pub name: String,
    /// Full-chain certificate in PEM form.
    pub cert_pem: String,
    pub key_pem: String,
}

#[derive(Debug, Deserialize)]
pub struct SelfSignedRequest {
    pub name: String,
    pub domains: Vec<String>,
    #[serde(default = "default_days")]
    pub days: i64,
}

fn default_days() -> i64 {
    825
}

#[derive(Debug, Deserialize)]
pub struct AcmeRequest {
    pub name: String,
    pub domains: Vec<String>,
    pub email: String,
    /// Directory served at `http://<domain>/.well-known/acme-challenge/`.
    pub webroot: String,
    /// Issue against the staging CA, for testing without hitting rate limits.
    #[serde(default)]
    pub staging: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AcmeStatus {
    /// acme.sh | certbot | none
    pub client: String,
    pub available: bool,
    pub version: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CertIssueResult {
    pub success: bool,
    pub cert_id: Option<i64>,
    pub log: String,
}

// ===== Storage layout =====

fn cert_dir(data_dir: &Path, id: i64) -> PathBuf {
    data_dir.join("certs").join(id.to_string())
}

/// Certificates are written to a temporary id first, then moved once the row exists.
fn staging_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("certs").join(".staging")
}

// ===== Validation =====

/// Domains reach openssl and ACME clients as arguments, so the format is strict.
pub fn validate_domain(domain: &str) -> Result<String, String> {
    let value = domain.trim().to_lowercase();
    if value.is_empty() || value.len() > 253 {
        return Err(format!("Invalid domain: {}", domain));
    }
    // A single leading wildcard label is allowed (DNS-01 only, but store it anyway).
    let body = value.strip_prefix("*.").unwrap_or(&value);
    let valid = !body.is_empty()
        && body.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    if !valid {
        return Err(format!("Invalid domain: {}", domain));
    }
    Ok(value)
}

fn validate_domains(domains: &[String]) -> Result<Vec<String>, String> {
    if domains.is_empty() {
        return Err("At least one domain is required".into());
    }
    if domains.len() > 100 {
        return Err("Too many domains".into());
    }
    let mut out = Vec::with_capacity(domains.len());
    for domain in domains {
        let normalized = validate_domain(domain)?;
        if !out.contains(&normalized) {
            out.push(normalized);
        }
    }
    Ok(out)
}

fn validate_email(email: &str) -> Result<String, String> {
    let value = email.trim();
    let valid = value.len() >= 3
        && value.len() <= 254
        && value.matches('@').count() == 1
        && !value.starts_with('@')
        && !value.ends_with('@')
        && !value.contains(char::is_whitespace)
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '-' | '_' | '+')
        });
    if valid {
        Ok(value.to_string())
    } else {
        Err(format!("Invalid email address: {}", email))
    }
}

/// Rejects anything that is not a PEM block of the expected type.
fn validate_pem(pem: &str, kind: &str) -> Result<String, String> {
    let text = pem.trim();
    let (begin, expected) = match kind {
        "cert" => ("-----BEGIN CERTIFICATE-----", "certificate"),
        "key" => ("-----BEGIN", "private key"),
        _ => return Err("Unknown PEM kind".into()),
    };
    if !text.starts_with(begin) {
        return Err(format!("Not a valid PEM {}", expected));
    }
    if kind == "key" {
        let is_key = text.starts_with("-----BEGIN PRIVATE KEY-----")
            || text.starts_with("-----BEGIN RSA PRIVATE KEY-----")
            || text.starts_with("-----BEGIN EC PRIVATE KEY-----");
        if !is_key {
            return Err("Not a valid PEM private key".into());
        }
    }
    Ok(format!("{}\n", text))
}

// ===== openssl-backed inspection =====

/// Reads subject, issuer, validity window and SANs out of a PEM certificate.
pub fn inspect(cert_path: &Path) -> Result<CertMeta, String> {
    if !shell::has_binary("openssl") {
        return Err("openssl is not installed on this system".into());
    }
    let path = cert_path.to_string_lossy().to_string();

    let output = shell::run(
        "openssl",
        &[
            "x509", "-in", &path, "-noout", "-subject", "-issuer", "-startdate", "-enddate",
        ],
    )?;
    if !output.ok() {
        return Err(format!("Cannot read certificate: {}", output.error_text()));
    }

    let mut meta = CertMeta::default();
    for line in output.stdout.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().to_string();
        match key.trim() {
            "subject" => meta.subject = value,
            "issuer" => meta.issuer = value,
            "notBefore" => meta.not_before = value,
            "notAfter" => meta.not_after = value,
            _ => {}
        }
    }

    // SANs are the authoritative domain list; the CN is only a fallback.
    if let Ok(san) = shell::run(
        "openssl",
        &["x509", "-in", &path, "-noout", "-ext", "subjectAltName"],
    ) {
        for line in san.stdout.lines() {
            for part in line.split(',') {
                if let Some(dns) = part.trim().strip_prefix("DNS:") {
                    let dns = dns.trim().to_lowercase();
                    if !dns.is_empty() && !meta.domains.contains(&dns) {
                        meta.domains.push(dns);
                    }
                }
            }
        }
    }
    if meta.domains.is_empty() {
        if let Some(cn) = meta
            .subject
            .split(['/', ','])
            .find_map(|part| part.trim().strip_prefix("CN="))
        {
            let cn = cn.trim().to_lowercase();
            if !cn.is_empty() {
                meta.domains.push(cn);
            }
        }
    }

    Ok(meta)
}

/// Confirms a key matches its certificate, so a bad pair is rejected on upload
/// rather than taking nginx down on the next reload.
fn key_matches_cert(cert_path: &Path, key_path: &Path) -> Result<bool, String> {
    let cert = cert_path.to_string_lossy().to_string();
    let key = key_path.to_string_lossy().to_string();

    let cert_modulus = shell::run("openssl", &["x509", "-in", &cert, "-noout", "-pubkey"])?;
    if !cert_modulus.ok() {
        return Err(format!(
            "Cannot read certificate public key: {}",
            cert_modulus.error_text()
        ));
    }
    let key_modulus = shell::run("openssl", &["pkey", "-in", &key, "-pubout"])?;
    if !key_modulus.ok() {
        return Err(format!(
            "Cannot read private key: {}",
            key_modulus.error_text()
        ));
    }
    Ok(cert_modulus.stdout.trim() == key_modulus.stdout.trim())
}

/// `Sep 20 15:09:53 2026 GMT` -> days from now (negative once expired).
pub fn days_until(not_after: &str) -> Option<i64> {
    let cleaned = not_after.trim().trim_end_matches(" GMT").trim();
    // Collapse the double space openssl emits for single-digit days.
    let normalized = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let parsed = NaiveDateTime::parse_from_str(&normalized, "%b %d %H:%M:%S %Y").ok()?;
    let expiry = Utc.from_utc_datetime(&parsed);
    Some((expiry - Utc::now()).num_days())
}

fn status_for(days: Option<i64>) -> String {
    match days {
        Some(d) if d < 0 => "expired".into(),
        Some(d) if d <= EXPIRING_SOON_DAYS => "expiring".into(),
        Some(_) => "valid".into(),
        None => "unknown".into(),
    }
}

// ===== Persistence =====

pub async fn list(pool: &sqlx::SqlitePool) -> Result<Vec<CertView>, String> {
    let records = sqlx::query_as::<_, CertRecord>("SELECT * FROM certificates ORDER BY id DESC")
        .fetch_all(pool)
        .await
        .map_err(|err| err.to_string())?;

    let mut views = Vec::with_capacity(records.len());
    for record in records {
        let days = days_until(&record.not_after);
        let used_by = sqlx::query_as::<_, (String,)>(
            "SELECT name FROM sites WHERE ssl_cert_path = ? ORDER BY name",
        )
        .bind(&record.cert_path)
        .fetch_all(pool)
        .await
        .map(|rows| rows.into_iter().map(|(name,)| name).collect())
        .unwrap_or_default();

        views.push(CertView {
            days_remaining: days.unwrap_or(0),
            status: status_for(days),
            used_by,
            record,
        });
    }
    Ok(views)
}

pub async fn get(pool: &sqlx::SqlitePool, id: i64) -> Result<CertRecord, String> {
    sqlx::query_as::<_, CertRecord>("SELECT * FROM certificates WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "Certificate not found".to_string())
}

/// Moves a validated cert/key pair into place and records it.
async fn persist(
    pool: &sqlx::SqlitePool,
    data_dir: &Path,
    name: &str,
    source: &str,
    auto_renew: bool,
    cert_pem: &str,
    key_pem: &str,
) -> Result<i64, String> {
    let staging = staging_dir(data_dir);
    std::fs::create_dir_all(&staging).map_err(|err| err.to_string())?;
    let staged_cert = staging.join("fullchain.pem");
    let staged_key = staging.join("privkey.pem");

    write_secret(&staged_cert, cert_pem, 0o644)?;
    write_secret(&staged_key, key_pem, 0o600)?;

    let meta = inspect(&staged_cert)?;
    if !key_matches_cert(&staged_cert, &staged_key)? {
        let _ = std::fs::remove_file(&staged_cert);
        let _ = std::fs::remove_file(&staged_key);
        return Err("The private key does not match the certificate".into());
    }

    let domains = meta.domains.join(",");
    let result = sqlx::query(
        "INSERT INTO certificates
         (name, domains, cert_path, key_path, issuer, subject, not_before, not_after, source, auto_renew, created_at, updated_at)
         VALUES (?, ?, '', '', ?, ?, ?, ?, ?, ?, datetime('now'), datetime('now'))",
    )
    .bind(name.trim())
    .bind(&domains)
    .bind(&meta.issuer)
    .bind(&meta.subject)
    .bind(&meta.not_before)
    .bind(&meta.not_after)
    .bind(source)
    .bind(auto_renew)
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;
    let id = result.last_insert_rowid();

    let dir = cert_dir(data_dir, id);
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let cert_path = dir.join("fullchain.pem");
    let key_path = dir.join("privkey.pem");
    std::fs::rename(&staged_cert, &cert_path).map_err(|err| err.to_string())?;
    std::fs::rename(&staged_key, &key_path).map_err(|err| err.to_string())?;

    sqlx::query("UPDATE certificates SET cert_path = ?, key_path = ? WHERE id = ?")
        .bind(cert_path.to_string_lossy().to_string())
        .bind(key_path.to_string_lossy().to_string())
        .bind(id)
        .execute(pool)
        .await
        .map_err(|err| err.to_string())?;

    Ok(id)
}

/// Writes a file and, on unix, sets its mode (private keys must not be world-readable).
fn write_secret(path: &Path, contents: &str, mode: u32) -> Result<(), String> {
    std::fs::write(path, contents).map_err(|err| err.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .map_err(|err| err.to_string())?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}

pub async fn upload(
    pool: &sqlx::SqlitePool,
    data_dir: &Path,
    request: &UploadCertRequest,
) -> Result<i64, String> {
    if request.name.trim().is_empty() {
        return Err("Certificate name is required".into());
    }
    let cert_pem = validate_pem(&request.cert_pem, "cert")?;
    let key_pem = validate_pem(&request.key_pem, "key")?;
    persist(pool, data_dir, &request.name, "uploaded", false, &cert_pem, &key_pem).await
}

pub async fn self_signed(
    pool: &sqlx::SqlitePool,
    data_dir: &Path,
    request: &SelfSignedRequest,
) -> Result<i64, String> {
    if request.name.trim().is_empty() {
        return Err("Certificate name is required".into());
    }
    if !shell::has_binary("openssl") {
        return Err("openssl is not installed on this system".into());
    }
    let domains = validate_domains(&request.domains)?;
    let days = request.days.clamp(1, 3650).to_string();

    let staging = staging_dir(data_dir);
    std::fs::create_dir_all(&staging).map_err(|err| err.to_string())?;
    let cert_file = staging.join("gen-cert.pem");
    let key_file = staging.join("gen-key.pem");

    let subject = format!("/CN={}", domains[0]);
    let san = format!(
        "subjectAltName={}",
        domains
            .iter()
            .map(|d| format!("DNS:{}", d))
            .collect::<Vec<_>>()
            .join(",")
    );
    let cert_str = cert_file.to_string_lossy().to_string();
    let key_str = key_file.to_string_lossy().to_string();

    let output = shell::run(
        "openssl",
        &[
            "req", "-x509", "-newkey", "rsa:2048", "-nodes",
            "-keyout", &key_str,
            "-out", &cert_str,
            "-days", &days,
            "-subj", &subject,
            "-addext", &san,
        ],
    )?;
    if !output.ok() {
        return Err(format!(
            "Failed to generate certificate: {}",
            output.error_text()
        ));
    }

    let cert_pem = std::fs::read_to_string(&cert_file).map_err(|err| err.to_string())?;
    let key_pem = std::fs::read_to_string(&key_file).map_err(|err| err.to_string())?;
    let _ = std::fs::remove_file(&cert_file);
    let _ = std::fs::remove_file(&key_file);

    persist(pool, data_dir, &request.name, "self-signed", false, &cert_pem, &key_pem).await
}

pub async fn delete(
    pool: &sqlx::SqlitePool,
    data_dir: &Path,
    id: i64,
) -> Result<(), String> {
    let record = get(pool, id).await?;

    // Refuse while a site still points at it, or nginx would fail to reload.
    let in_use: Option<(String,)> =
        sqlx::query_as("SELECT name FROM sites WHERE ssl_cert_path = ? LIMIT 1")
            .bind(&record.cert_path)
            .fetch_optional(pool)
            .await
            .map_err(|err| err.to_string())?;
    if let Some((site,)) = in_use {
        return Err(format!("Certificate is still used by site '{}'", site));
    }

    sqlx::query("DELETE FROM certificates WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|err| err.to_string())?;
    let _ = std::fs::remove_dir_all(cert_dir(data_dir, id));
    Ok(())
}

// ===== ACME =====

pub fn acme_status() -> AcmeStatus {
    if shell::has_binary("acme.sh") {
        let version = shell::run("acme.sh", &["--version"])
            .ok()
            .map(|out| out.stdout.lines().last().unwrap_or("").trim().to_string())
            .unwrap_or_default();
        return AcmeStatus {
            client: "acme.sh".into(),
            available: true,
            version,
            message: String::new(),
        };
    }
    if shell::has_binary("certbot") {
        let version = shell::run("certbot", &["--version"])
            .ok()
            .map(|out| {
                let text = if out.stdout.trim().is_empty() {
                    out.stderr
                } else {
                    out.stdout
                };
                text.trim().to_string()
            })
            .unwrap_or_default();
        return AcmeStatus {
            client: "certbot".into(),
            available: true,
            version,
            message: String::new(),
        };
    }
    AcmeStatus {
        client: "none".into(),
        available: false,
        version: String::new(),
        message: "No ACME client found. Install acme.sh or certbot to issue Let's Encrypt certificates.".into(),
    }
}

/// Issues a certificate over HTTP-01 using the site's webroot.
pub async fn issue_acme(
    pool: &sqlx::SqlitePool,
    data_dir: &Path,
    request: &AcmeRequest,
) -> Result<CertIssueResult, String> {
    if request.name.trim().is_empty() {
        return Err("Certificate name is required".into());
    }
    let domains = validate_domains(&request.domains)?;
    let email = validate_email(&request.email)?;
    let webroot = request.webroot.trim().to_string();
    if webroot.is_empty() || !Path::new(&webroot).is_dir() {
        return Err(format!("Webroot directory does not exist: {}", webroot));
    }

    let status = acme_status();
    if !status.available {
        return Err(status.message);
    }

    let mut log = format!(
        "[LinxPanel] Issuing certificate for {} via {}\n",
        domains.join(", "),
        status.client
    );

    // Both clients write their output where we can pick it up afterwards.
    let out_dir = staging_dir(data_dir).join("acme");
    std::fs::create_dir_all(&out_dir).map_err(|err| err.to_string())?;
    let cert_out = out_dir.join("fullchain.pem");
    let key_out = out_dir.join("privkey.pem");
    let cert_out_str = cert_out.to_string_lossy().to_string();
    let key_out_str = key_out.to_string_lossy().to_string();

    let (program, args) = match status.client.as_str() {
        "acme.sh" => {
            let mut args: Vec<String> = vec!["--issue".into(), "-w".into(), webroot.clone()];
            for domain in &domains {
                args.push("-d".into());
                args.push(domain.clone());
            }
            args.push("--accountemail".into());
            args.push(email.clone());
            if request.staging {
                args.push("--staging".into());
            }
            ("acme.sh", args)
        }
        "certbot" => {
            let mut args: Vec<String> = vec![
                "certonly".into(),
                "--webroot".into(),
                "-w".into(),
                webroot.clone(),
                "--non-interactive".into(),
                "--agree-tos".into(),
                "-m".into(),
                email.clone(),
                "--cert-name".into(),
                domains[0].clone(),
            ];
            for domain in &domains {
                args.push("-d".into());
                args.push(domain.clone());
            }
            if request.staging {
                args.push("--staging".into());
            }
            ("certbot", args)
        }
        other => return Err(format!("Unsupported ACME client: {}", other)),
    };

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    log.push_str(&format!("\n$ {} {}\n", program, arg_refs.join(" ")));

    let issue = shell::run_privileged_with_timeout(
        program,
        &arg_refs,
        std::time::Duration::from_secs(600),
    )?;
    log.push_str(issue.stdout.trim_end());
    if !issue.stderr.trim().is_empty() {
        log.push('\n');
        log.push_str(issue.stderr.trim_end());
    }
    log.push('\n');

    if !issue.ok() {
        return Ok(CertIssueResult {
            success: false,
            cert_id: None,
            log,
        });
    }

    // Copy the issued material into the panel's own store.
    match status.client.as_str() {
        "acme.sh" => {
            let mut install: Vec<String> = vec![
                "--install-cert".into(),
                "-d".into(),
                domains[0].clone(),
                "--fullchain-file".into(),
                cert_out_str.clone(),
                "--key-file".into(),
                key_out_str.clone(),
            ];
            let install_refs: Vec<&str> = install.iter_mut().map(|s| s.as_str()).collect();
            let installed = shell::run_privileged("acme.sh", &install_refs)?;
            log.push_str(installed.stdout.trim_end());
            if !installed.ok() {
                log.push_str(&format!("\n{}\n", installed.error_text()));
                return Ok(CertIssueResult {
                    success: false,
                    cert_id: None,
                    log,
                });
            }
        }
        "certbot" => {
            let live = format!("/etc/letsencrypt/live/{}", domains[0]);
            let copy_cert = shell::run_privileged(
                "cp",
                &[&format!("{}/fullchain.pem", live), &cert_out_str],
            )?;
            let copy_key = shell::run_privileged(
                "cp",
                &[&format!("{}/privkey.pem", live), &key_out_str],
            )?;
            if !copy_cert.ok() || !copy_key.ok() {
                log.push_str("\nFailed to copy issued certificate from /etc/letsencrypt/live\n");
                return Ok(CertIssueResult {
                    success: false,
                    cert_id: None,
                    log,
                });
            }
        }
        _ => {}
    }

    let cert_pem = std::fs::read_to_string(&cert_out)
        .map_err(|err| format!("Cannot read issued certificate: {}", err))?;
    let key_pem = std::fs::read_to_string(&key_out)
        .map_err(|err| format!("Cannot read issued key: {}", err))?;
    let _ = std::fs::remove_file(&cert_out);
    let _ = std::fs::remove_file(&key_out);

    let id = persist(pool, data_dir, &request.name, "acme", true, &cert_pem, &key_pem).await?;
    log.push_str("\n[LinxPanel] Certificate stored and ready to bind to a site.\n");

    Ok(CertIssueResult {
        success: true,
        cert_id: Some(id),
        log,
    })
}

/// Renews an ACME certificate in place and refreshes its stored metadata.
pub async fn renew(pool: &sqlx::SqlitePool, id: i64) -> Result<CertIssueResult, String> {
    let record = get(pool, id).await?;
    if record.source != "acme" {
        return Err("Only ACME certificates can be renewed automatically".into());
    }
    let status = acme_status();
    if !status.available {
        return Err(status.message);
    }

    let primary = record
        .domains
        .split(',')
        .next()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .ok_or_else(|| "Certificate has no domains".to_string())?;
    let primary = validate_domain(primary)?;

    let mut log = format!("[LinxPanel] Renewing {}\n", primary);
    let output = match status.client.as_str() {
        "acme.sh" => shell::run_privileged_with_timeout(
            "acme.sh",
            &["--renew", "-d", &primary, "--force"],
            std::time::Duration::from_secs(600),
        )?,
        "certbot" => shell::run_privileged_with_timeout(
            "certbot",
            &["renew", "--cert-name", &primary, "--non-interactive"],
            std::time::Duration::from_secs(600),
        )?,
        other => return Err(format!("Unsupported ACME client: {}", other)),
    };
    log.push_str(output.stdout.trim_end());
    if !output.stderr.trim().is_empty() {
        log.push('\n');
        log.push_str(output.stderr.trim_end());
    }
    if !output.ok() {
        return Ok(CertIssueResult {
            success: false,
            cert_id: Some(id),
            log,
        });
    }

    // Pull the refreshed material back into the panel store, keeping paths stable
    // so bound sites need no config change.
    let cert_path = PathBuf::from(&record.cert_path);
    let key_path = PathBuf::from(&record.key_path);
    match status.client.as_str() {
        "acme.sh" => {
            let install_args = vec![
                "--install-cert".to_string(),
                "-d".to_string(),
                primary.clone(),
                "--fullchain-file".to_string(),
                record.cert_path.clone(),
                "--key-file".to_string(),
                record.key_path.clone(),
            ];
            let refs: Vec<&str> = install_args.iter().map(String::as_str).collect();
            let installed = shell::run_privileged("acme.sh", &refs)?;
            if !installed.ok() {
                log.push_str(&format!("\n{}\n", installed.error_text()));
                return Ok(CertIssueResult {
                    success: false,
                    cert_id: Some(id),
                    log,
                });
            }
        }
        "certbot" => {
            let live = format!("/etc/letsencrypt/live/{}", primary);
            let _ = shell::run_privileged(
                "cp",
                &[&format!("{}/fullchain.pem", live), &record.cert_path],
            )?;
            let _ = shell::run_privileged(
                "cp",
                &[&format!("{}/privkey.pem", live), &record.key_path],
            )?;
        }
        _ => {}
    }

    let meta = inspect(&cert_path)?;
    let _ = key_path;
    sqlx::query(
        "UPDATE certificates SET issuer = ?, subject = ?, not_before = ?, not_after = ?,
         domains = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(&meta.issuer)
    .bind(&meta.subject)
    .bind(&meta.not_before)
    .bind(&meta.not_after)
    .bind(meta.domains.join(","))
    .bind(id)
    .execute(pool)
    .await
    .map_err(|err| err.to_string())?;

    log.push_str(&format!("\n[LinxPanel] Renewed, now valid until {}\n", meta.not_after));
    Ok(CertIssueResult {
        success: true,
        cert_id: Some(id),
        log,
    })
}

/// Certificates that are due for renewal, used by the daily check.
pub async fn due_for_renewal(pool: &sqlx::SqlitePool) -> Vec<CertRecord> {
    let records = sqlx::query_as::<_, CertRecord>(
        "SELECT * FROM certificates WHERE source = 'acme' AND auto_renew = 1",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();

    records
        .into_iter()
        .filter(|record| {
            days_until(&record.not_after)
                .map(|days| days <= RENEW_WINDOW_DAYS)
                .unwrap_or(false)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_and_wildcard_domains() {
        assert_eq!(validate_domain(" Example.COM ").unwrap(), "example.com");
        assert_eq!(validate_domain("*.example.com").unwrap(), "*.example.com");
        assert_eq!(validate_domain("a-b.c-d.example").unwrap(), "a-b.c-d.example");
    }

    #[test]
    fn rejects_domains_that_could_become_extra_arguments() {
        for bad in [
            "", "exa mple.com", "-bad.com", "bad-.com", "ex;rm -rf /.com",
            "$(whoami).com", "a..b.com", "--force",
        ] {
            assert!(validate_domain(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn rejects_malformed_emails() {
        assert!(validate_email("admin@example.com").is_ok());
        for bad in ["", "no-at-sign", "a@b@c", "@example.com", "a b@example.com"] {
            assert!(validate_email(bad).is_err(), "should reject {bad:?}");
        }
    }

    #[test]
    fn rejects_non_pem_input() {
        assert!(validate_pem("not a cert", "cert").is_err());
        assert!(validate_pem("-----BEGIN CERTIFICATE-----\nabc\n", "cert").is_ok());
        assert!(validate_pem("-----BEGIN CERTIFICATE-----\nabc\n", "key").is_err());
        assert!(validate_pem("-----BEGIN PRIVATE KEY-----\nabc\n", "key").is_ok());
    }

    #[test]
    fn parses_openssl_expiry_format() {
        // Single-digit days come through double-spaced, which must still parse.
        assert!(days_until("Dec 19 15:09:53 2099 GMT").unwrap() > 0);
        assert!(days_until("Sep  2 15:09:53 2000 GMT").unwrap() < 0);
        assert!(days_until("nonsense").is_none());
    }

    #[test]
    fn maps_days_to_status() {
        assert_eq!(status_for(Some(90)), "valid");
        assert_eq!(status_for(Some(5)), "expiring");
        assert_eq!(status_for(Some(-1)), "expired");
        assert_eq!(status_for(None), "unknown");
    }
}
