use sqlx::SqlitePool;

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query("PRAGMA journal_mode=WAL").execute(pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            role TEXT NOT NULL DEFAULT 'admin',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sites (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            domains TEXT NOT NULL,
            doc_root TEXT NOT NULL,
            site_type TEXT NOT NULL DEFAULT 'static',
            index_files TEXT NOT NULL DEFAULT 'index.html index.htm index.php',
            status TEXT NOT NULL DEFAULT 'running',
            ssl_enabled INTEGER NOT NULL DEFAULT 0,
            ssl_cert_path TEXT,
            ssl_key_path TEXT,
            proxy_pass TEXT,
            stopped_page_html TEXT,
            extra_config TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS domains (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            site_id INTEGER NOT NULL,
            domain TEXT NOT NULL,
            is_primary INTEGER NOT NULL DEFAULT 0,
            sort_order INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (site_id) REFERENCES sites(id) ON DELETE CASCADE
        )",
    )
    .execute(pool)
    .await?;

    let has_sort_order: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM pragma_table_info('domains') WHERE name = 'sort_order' LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    if has_sort_order.is_none() {
        sqlx::query("ALTER TABLE domains ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0")
            .execute(pool)
            .await?;

        let existing_domains: Vec<(i64, i64)> = sqlx::query_as(
            "SELECT id, site_id FROM domains ORDER BY site_id ASC, is_primary DESC, id ASC",
        )
        .fetch_all(pool)
        .await?;
        let mut current_site_id = i64::MIN;
        let mut index: i64 = 0;
        for (domain_id, site_id) in existing_domains {
            if site_id != current_site_id {
                current_site_id = site_id;
                index = 0;
            }
            sqlx::query("UPDATE domains SET sort_order = ? WHERE id = ?")
                .bind(index)
                .bind(domain_id)
                .execute(pool)
                .await?;
            index += 1;
        }
    }

    let has_index_files: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM pragma_table_info('sites') WHERE name = 'index_files' LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    if has_index_files.is_none() {
        sqlx::query(
            "ALTER TABLE sites ADD COLUMN index_files TEXT NOT NULL DEFAULT 'index.html index.htm index.php'",
        )
        .execute(pool)
        .await?;
    }

    let has_stopped_page_html: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM pragma_table_info('sites') WHERE name = 'stopped_page_html' LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    if has_stopped_page_html.is_none() {
        sqlx::query("ALTER TABLE sites ADD COLUMN stopped_page_html TEXT")
            .execute(pool)
            .await?;
    }

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL,
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS operation_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER,
            action TEXT NOT NULL,
            target TEXT,
            detail TEXT,
            ip TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS apps (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            runtime TEXT NOT NULL,
            work_dir TEXT NOT NULL,
            entry TEXT NOT NULL,
            args TEXT NOT NULL DEFAULT '',
            env TEXT NOT NULL DEFAULT '',
            port INTEGER NOT NULL DEFAULT 0,
            autostart INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    // Sites of an app type proxy to a managed application instead of a raw address.
    let has_app_id: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM pragma_table_info('sites') WHERE name = 'app_id' LIMIT 1")
            .fetch_optional(pool)
            .await?;
    if has_app_id.is_none() {
        sqlx::query("ALTER TABLE sites ADD COLUMN app_id INTEGER")
            .execute(pool)
            .await?;
    }

    let has_rewrite: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM pragma_table_info('sites') WHERE name = 'rewrite_rules' LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    if has_rewrite.is_none() {
        sqlx::query("ALTER TABLE sites ADD COLUMN rewrite_rules TEXT")
            .execute(pool)
            .await?;
    }

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS certificates (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            domains TEXT NOT NULL DEFAULT '',
            cert_path TEXT NOT NULL DEFAULT '',
            key_path TEXT NOT NULL DEFAULT '',
            issuer TEXT NOT NULL DEFAULT '',
            subject TEXT NOT NULL DEFAULT '',
            not_before TEXT NOT NULL DEFAULT '',
            not_after TEXT NOT NULL DEFAULT '',
            source TEXT NOT NULL DEFAULT 'uploaded',
            auto_renew INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    // Bumped on every password change; tokens carry it, so a change signs out
    // every existing session.
    let has_session_version: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM pragma_table_info('users') WHERE name = 'session_version' LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    if has_session_version.is_none() {
        sqlx::query("ALTER TABLE users ADD COLUMN session_version INTEGER NOT NULL DEFAULT 0")
            .execute(pool)
            .await?;
    }

    // Two-factor login: the active TOTP secret, one being set up, and the last
    // accepted time step (so a code cannot be replayed).
    for (column, definition) in [
        ("totp_secret", "TEXT"),
        ("totp_pending", "TEXT"),
        ("totp_last_step", "INTEGER NOT NULL DEFAULT 0"),
    ] {
        let exists: Option<(i64,)> =
            sqlx::query_as("SELECT 1 FROM pragma_table_info('users') WHERE name = ? LIMIT 1")
                .bind(column)
                .fetch_optional(pool)
                .await?;
        if exists.is_none() {
            sqlx::query(&format!(
                "ALTER TABLE users ADD COLUMN {} {}",
                column, definition
            ))
            .execute(pool)
            .await?;
        }
    }

    // Website programs deployed from the app store.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS app_installs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            app TEXT NOT NULL,
            site_id INTEGER NOT NULL,
            domain TEXT NOT NULL,
            doc_root TEXT NOT NULL,
            db_name TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
    )
    .execute(pool)
    .await?;

    // Databases created through the panel, with the credentials it generated.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS databases (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            engine TEXT NOT NULL,
            name TEXT NOT NULL,
            username TEXT NOT NULL DEFAULT '',
            password TEXT NOT NULL DEFAULT '',
            host TEXT NOT NULL DEFAULT 'localhost',
            note TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(engine, name)
        )",
    )
    .execute(pool)
    .await?;

    // Active defense: addresses banned in the kernel, and what the engine did.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS ip_bans (
            ip TEXT PRIMARY KEY,
            source TEXT NOT NULL DEFAULT 'manual',
            reason TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            expires_at INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS defense_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            at INTEGER NOT NULL,
            ip TEXT NOT NULL,
            source TEXT NOT NULL,
            action TEXT NOT NULL,
            detail TEXT NOT NULL DEFAULT ''
        )",
    )
    .execute(pool)
    .await?;

    // Opt-in HTTP->HTTPS redirect and HSTS, per site.
    for column in ["force_https", "hsts"] {
        let exists: Option<(i64,)> =
            sqlx::query_as("SELECT 1 FROM pragma_table_info('sites') WHERE name = ? LIMIT 1")
                .bind(column)
                .fetch_optional(pool)
                .await?;
        if exists.is_none() {
            sqlx::query(&format!(
                "ALTER TABLE sites ADD COLUMN {} INTEGER NOT NULL DEFAULT 0",
                column
            ))
            .execute(pool)
            .await?;
        }
    }

    tracing::info!("Database migration completed");
    Ok(())
}

pub const DEFAULT_ADMIN_USER: &str = "admin";
pub const DEFAULT_ADMIN_PASSWORD: &str = "linxpanel";

/// Minimum bar for a panel password; the panel controls the whole server.
pub fn validate_password(password: &str) -> Result<(), &'static str> {
    if password.chars().count() < 8 {
        return Err("Password must be at least 8 characters");
    }
    if password.chars().count() > 72 {
        // bcrypt silently ignores everything past 72 bytes.
        return Err("Password must be at most 72 characters");
    }
    if password.chars().any(|c| c.is_control()) {
        return Err("Password must not contain control characters");
    }
    Ok(())
}

/// Sets a user's password, creating the account as an admin when it does not exist.
/// Existing sessions of that user stop working because `session_version` is bumped.
pub async fn set_user_password(
    pool: &SqlitePool,
    username: &str,
    password: &str,
) -> anyhow::Result<()> {
    let hash = bcrypt::hash(password, 10)?;
    sqlx::query(
        "INSERT INTO users (username, password_hash, role) VALUES (?, ?, 'admin')
         ON CONFLICT(username) DO UPDATE SET password_hash = excluded.password_hash,
         session_version = session_version + 1, updated_at = datetime('now')",
    )
    .bind(username)
    .bind(&hash)
    .execute(pool)
    .await?;
    Ok(())
}

/// A random password from an unambiguous alphabet (no 0/O, 1/l/I).
pub fn generate_password(len: usize) -> String {
    const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut out = String::with_capacity(len);
    while out.len() < len {
        // v4 UUIDs come from the OS CSPRNG; bytes 6 and 8 carry fixed version bits.
        let bytes = uuid::Uuid::new_v4().into_bytes();
        for (index, &byte) in bytes.iter().enumerate() {
            if index == 6 || index == 8 {
                continue;
            }
            // Rejection sampling keeps the distribution uniform.
            let limit = 256 - (256 % ALPHABET.len());
            if (byte as usize) < limit && out.len() < len {
                out.push(ALPHABET[byte as usize % ALPHABET.len()] as char);
            }
        }
    }
    out
}

/// The current session version of a user, or None when the user no longer exists.
pub async fn session_version(pool: &SqlitePool, user_id: i64) -> anyhow::Result<Option<i64>> {
    let row: Option<(i64,)> = sqlx::query_as("SELECT session_version FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(version,)| version))
}

pub struct AccountSecurity {
    pub username: String,
    pub mfa_enabled: bool,
}

pub async fn account_security(
    pool: &SqlitePool,
    user_id: i64,
) -> anyhow::Result<Option<AccountSecurity>> {
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT username, totp_secret FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(username, secret)| AccountSecurity {
        username,
        mfa_enabled: secret.is_some_and(|s| !s.is_empty()),
    }))
}

/// The user's TOTP state: (active secret, pending secret, last accepted step).
pub async fn totp_state(
    pool: &SqlitePool,
    user_id: i64,
) -> anyhow::Result<(Option<String>, Option<String>, i64)> {
    let row: Option<(Option<String>, Option<String>, i64)> =
        sqlx::query_as("SELECT totp_secret, totp_pending, totp_last_step FROM users WHERE id = ?")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    let (secret, pending, step) = row.unwrap_or((None, None, 0));
    let non_empty = |value: Option<String>| value.filter(|s| !s.is_empty());
    Ok((non_empty(secret), non_empty(pending), step))
}

pub async fn set_totp_pending(pool: &SqlitePool, user_id: i64, secret: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE users SET totp_pending = ? WHERE id = ?")
        .bind(secret)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Turns the pending secret into the active one.
pub async fn activate_totp(pool: &SqlitePool, user_id: i64, step: i64) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE users SET totp_secret = totp_pending, totp_pending = NULL, totp_last_step = ?
         WHERE id = ?",
    )
    .bind(step)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn record_totp_step(pool: &SqlitePool, user_id: i64, step: i64) -> anyhow::Result<()> {
    sqlx::query("UPDATE users SET totp_last_step = ? WHERE id = ?")
        .bind(step)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Turns two-factor login off; returns false when the user does not exist.
pub async fn disable_totp(pool: &SqlitePool, username: &str) -> anyhow::Result<bool> {
    let result =
        sqlx::query("UPDATE users SET totp_secret = NULL, totp_pending = NULL WHERE username = ?")
            .bind(username)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

/// The panel's own account (the first one created), which may have been renamed.
pub async fn primary_username(pool: &SqlitePool) -> anyhow::Result<String> {
    let row: Option<(String,)> = sqlx::query_as("SELECT username FROM users ORDER BY id LIMIT 1")
        .fetch_optional(pool)
        .await?;
    Ok(row.map_or_else(|| DEFAULT_ADMIN_USER.to_string(), |(name,)| name))
}

pub fn validate_username(username: &str) -> Result<(), &'static str> {
    let valid_len = (3..=32).contains(&username.len());
    let valid_chars = username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if valid_len && valid_chars {
        Ok(())
    } else {
        Err("Username must be 3-32 letters, digits, '_', '-' or '.'")
    }
}

pub async fn get_setting(pool: &SqlitePool, key: &str) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(value,)| value))
}

pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn seed_admin(pool: &SqlitePool) -> anyhow::Result<()> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;

    if count.0 == 0 {
        set_user_password(pool, DEFAULT_ADMIN_USER, DEFAULT_ADMIN_PASSWORD).await?;
        tracing::warn!(
            "Default admin user created ({}/{}); change it, or run `linxpanel reset-password`",
            DEFAULT_ADMIN_USER,
            DEFAULT_ADMIN_PASSWORD
        );
    }
    Ok(())
}

/// Returns the persisted JWT signing secret, creating one on first boot.
///
/// A per-boot random secret would invalidate every session on restart, so the
/// secret lives in the settings table. `JWT_SECRET` in the environment still wins.
pub async fn resolve_jwt_secret(pool: &SqlitePool) -> anyhow::Result<String> {
    if let Ok(secret) = std::env::var("JWT_SECRET") {
        if !secret.trim().is_empty() {
            return Ok(secret);
        }
    }

    let existing: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind("jwt_secret")
        .fetch_optional(pool)
        .await?;
    if let Some((secret,)) = existing {
        if !secret.trim().is_empty() {
            return Ok(secret);
        }
    }

    let secret = format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind("jwt_secret")
    .bind(&secret)
    .execute(pool)
    .await?;
    tracing::info!("Generated and stored a new JWT secret");
    Ok(secret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_passwords_are_valid_and_distinct() {
        let first = generate_password(16);
        let second = generate_password(16);
        assert_eq!(first.len(), 16);
        assert!(validate_password(&first).is_ok());
        assert_ne!(first, second);
        assert!(first.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn password_validation_rejects_weak_input() {
        assert!(validate_password("short").is_err());
        assert!(validate_password(&"a".repeat(73)).is_err());
        assert!(validate_password("has\nnewline").is_err());
        assert!(validate_password("good-password").is_ok());
    }
}
