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
            status TEXT NOT NULL DEFAULT 'running',
            ssl_enabled INTEGER NOT NULL DEFAULT 0,
            ssl_cert_path TEXT,
            ssl_key_path TEXT,
            proxy_pass TEXT,
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

    let has_sort_order: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM pragma_table_info('domains') WHERE name = 'sort_order' LIMIT 1")
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

    tracing::info!("Database migration completed");
    Ok(())
}

pub async fn seed_admin(pool: &SqlitePool) -> anyhow::Result<()> {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;

    if count.0 == 0 {
        let hash = bcrypt::hash("linxpanel", 10)?;
        sqlx::query("INSERT INTO users (username, password_hash, role) VALUES (?, ?, ?)")
            .bind("admin")
            .bind(&hash)
            .bind("admin")
            .execute(pool)
            .await?;
        tracing::info!("Default admin user created (admin/linxpanel)");
    }
    Ok(())
}
