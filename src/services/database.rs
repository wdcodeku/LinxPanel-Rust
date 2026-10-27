//! Database servers: install and run MySQL (MariaDB), PostgreSQL and Redis, and
//! manage databases, users, passwords and backups the way BaoTa / 1Panel do.
//!
//! Everything goes through the engines' own CLI clients. SQL and passwords are
//! fed on stdin (never as arguments, which `ps` would show), and every name is
//! allowlisted before it reaches a statement.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::{mongo, runtime, shell, sysservice};
use crate::db;

const SQL_TIMEOUT: Duration = Duration::from_secs(60);
const DUMP_TIMEOUT: Duration = Duration::from_secs(3600);

/// Panel-owned MySQL account, created through root's socket login at install time.
pub const SETTING_MYSQL_ADMIN_USER: &str = "mysql_admin_user";
pub const SETTING_MYSQL_ADMIN_PASSWORD: &str = "mysql_admin_password";
const PANEL_MYSQL_USER: &str = "linxpanel";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Mysql,
    Postgresql,
    Redis,
    Mongodb,
}

impl Engine {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw {
            "mysql" | "mariadb" => Ok(Self::Mysql),
            "postgresql" | "postgres" | "pgsql" => Ok(Self::Postgresql),
            "redis" => Ok(Self::Redis),
            "mongodb" | "mongo" => Ok(Self::Mongodb),
            other => Err(format!("Unknown database engine: {}", other)),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mysql => "mysql",
            Self::Postgresql => "postgresql",
            Self::Redis => "redis",
            Self::Mongodb => "mongodb",
        }
    }

    pub(crate) fn display_name(self) -> &'static str {
        match self {
            Self::Mysql => "MySQL / MariaDB",
            Self::Postgresql => "PostgreSQL",
            Self::Redis => "Redis",
            Self::Mongodb => "MongoDB",
        }
    }

    pub(crate) fn services(self) -> &'static [&'static str] {
        match self {
            Self::Mysql => &["mariadb", "mysql", "mysqld"],
            Self::Postgresql => &["postgresql"],
            Self::Redis => &["redis-server", "redis"],
            Self::Mongodb => &["mongod"],
        }
    }

    fn default_port(self) -> u16 {
        match self {
            Self::Mysql => 3306,
            Self::Postgresql => 5432,
            Self::Redis => 6379,
            Self::Mongodb => 27017,
        }
    }

    fn packages(self, manager: &str) -> &'static [&'static str] {
        match (self, manager) {
            (Self::Mysql, "apt-get") => &["mariadb-server", "mariadb-client"],
            (Self::Mysql, "dnf" | "yum") => &["mariadb-server", "mariadb"],
            (Self::Mysql, "apk") => &["mariadb", "mariadb-client", "mariadb-server-utils"],
            (Self::Postgresql, "apt-get") => &["postgresql", "postgresql-contrib"],
            (Self::Postgresql, "dnf" | "yum") => &["postgresql-server", "postgresql-contrib"],
            (Self::Postgresql, "apk") => &["postgresql", "postgresql-contrib"],
            (Self::Redis, "apt-get") => &["redis-server"],
            (Self::Redis, "dnf" | "yum" | "apk") => &["redis"],
            _ => &[],
        }
    }
}

pub const ENGINES: [Engine; 4] = [
    Engine::Mysql,
    Engine::Postgresql,
    Engine::Redis,
    Engine::Mongodb,
];

#[derive(Debug, Clone, Serialize)]
pub struct EngineStatus {
    pub engine: Engine,
    pub display_name: &'static str,
    pub installed: bool,
    pub version: String,
    pub service: String,
    pub running: bool,
    pub port: u16,
    /// Whether the panel can administer it (MySQL needs stored credentials).
    pub manageable: bool,
    pub message: String,
}

fn client_binary(engine: Engine) -> Option<&'static str> {
    let candidates: &[&str] = match engine {
        Engine::Mysql => &["mariadb", "mysql"],
        Engine::Postgresql => &["psql"],
        Engine::Redis => &["redis-server"],
        Engine::Mongodb => &["mongosh"],
    };
    candidates
        .iter()
        .copied()
        .find(|bin| shell::has_binary(bin))
}

pub(crate) fn service_name(engine: Engine) -> String {
    let names: Vec<String> = engine.services().iter().map(|s| s.to_string()).collect();
    sysservice::first_existing(&names).unwrap_or_else(|| names[0].clone())
}

fn first_line_version(program: &str) -> String {
    shell::cached_version_output(program, &["--version"])
        .map(|text| text.lines().next().unwrap_or_default().trim().to_string())
        .unwrap_or_default()
}

pub fn status(engine: Engine, admin: Option<&MysqlAdmin>) -> EngineStatus {
    let client = client_binary(engine);
    // PostgreSQL's client can be installed without a server; require both.
    let server_present = match engine {
        Engine::Postgresql => {
            shell::has_binary("pg_ctlcluster")
                || shell::has_binary("postgres")
                || Path::new("/usr/lib/postgresql").exists()
        }
        Engine::Mysql => {
            shell::has_binary("mariadbd")
                || shell::has_binary("mysqld")
                || Path::new("/usr/sbin/mariadbd").exists()
                || Path::new("/usr/sbin/mysqld").exists()
        }
        Engine::Redis => true,
        Engine::Mongodb => super::mongo::server_present(),
    };
    let installed = client.is_some() && server_present;
    let service = service_name(engine);
    let running = installed && sysservice::is_active(&service);
    let version = match engine {
        // mongosh --version prints the shell's version, not the server's.
        Engine::Mongodb if installed => first_line_version("mongod"),
        _ => client.map(first_line_version).unwrap_or_default(),
    };

    let (manageable, message) = match (engine, installed, running) {
        (_, false, _) => (false, "Not installed".to_string()),
        (_, true, false) => (false, "Installed but not running".to_string()),
        (Engine::Mysql, true, true) => match admin {
            Some(admin) => match mysql_query(admin, "SELECT 1") {
                Ok(_) => (true, String::new()),
                Err(err) => (false, format!("Stored admin login failed: {}", err)),
            },
            None => (
                false,
                "The panel has no administrator login for this MySQL; enter the root password"
                    .to_string(),
            ),
        },
        (Engine::Mongodb, true, true) => match admin {
            Some(admin) => match super::mongo::ping(Some(admin)) {
                Ok(()) => (true, String::new()),
                Err(err) => (false, format!("Stored admin login failed: {}", err)),
            },
            None => (
                false,
                "The panel has no administrator login for this MongoDB; enter an administrator login"
                    .to_string(),
            ),
        },
        (_, true, true) => (true, String::new()),
    };

    EngineStatus {
        engine,
        display_name: engine.display_name(),
        installed,
        version,
        service,
        running,
        port: if engine == Engine::Mongodb {
            super::mongo::port()
        } else {
            engine.default_port()
        },
        manageable,
        message,
    }
}

// ===== Validation =====

/// Database and user names: letters, digits and underscore, starting with a letter.
pub fn validate_name(name: &str, what: &str) -> Result<(), String> {
    let ok = (1..=32).contains(&name.len())
        && name.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "{} must be 1-32 letters, digits or '_', starting with a letter",
            what
        ))
    }
}

/// Passwords avoid quotes, backslashes and whitespace, so they embed in SQL
/// literals and client config files without any escaping.
pub fn validate_db_password(password: &str) -> Result<(), String> {
    let ok = (8..=64).contains(&password.len())
        && password
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "!@#%^*_+=-.,:~".contains(c));
    if ok {
        Ok(())
    } else {
        Err("Password must be 8-64 characters: letters, digits and !@#%^*_+=-.,:~".into())
    }
}

/// Where a MySQL user may connect from: localhost, any (%) or an IP / pattern.
pub fn validate_host(host: &str) -> Result<(), String> {
    let ok = !host.is_empty()
        && host.len() <= 64
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".:%_-/".contains(c));
    if ok {
        Ok(())
    } else {
        Err("Invalid access host".into())
    }
}

pub fn generate_db_password() -> String {
    db::generate_password(20)
}

// ===== MySQL =====

#[derive(Debug, Clone)]
pub struct MysqlAdmin {
    pub user: String,
    pub password: String,
}

pub async fn load_mysql_admin(pool: &SqlitePool) -> Option<MysqlAdmin> {
    let user = db::get_setting(pool, SETTING_MYSQL_ADMIN_USER)
        .await
        .ok()
        .flatten()?;
    let password = db::get_setting(pool, SETTING_MYSQL_ADMIN_PASSWORD)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    (!user.is_empty()).then_some(MysqlAdmin { user, password })
}

/// The panel's administrator login for an engine (MySQL and MongoDB have one).
pub async fn load_admin(pool: &SqlitePool, engine: Engine) -> Option<MysqlAdmin> {
    match engine {
        Engine::Mysql => load_mysql_admin(pool).await,
        Engine::Mongodb => {
            let user = db::get_setting(pool, super::mongo::SETTING_ADMIN_USER)
                .await
                .ok()
                .flatten()?;
            let password = db::get_setting(pool, super::mongo::SETTING_ADMIN_PASSWORD)
                .await
                .ok()
                .flatten()
                .unwrap_or_default();
            (!user.is_empty()).then_some(MysqlAdmin { user, password })
        }
        _ => None,
    }
}

pub async fn save_admin(
    pool: &SqlitePool,
    engine: Engine,
    admin: &MysqlAdmin,
) -> anyhow::Result<()> {
    match engine {
        Engine::Mongodb => {
            db::set_setting(pool, super::mongo::SETTING_ADMIN_USER, &admin.user).await?;
            db::set_setting(pool, super::mongo::SETTING_ADMIN_PASSWORD, &admin.password).await
        }
        _ => save_mysql_admin(pool, admin).await,
    }
}

pub async fn save_mysql_admin(pool: &SqlitePool, admin: &MysqlAdmin) -> anyhow::Result<()> {
    db::set_setting(pool, SETTING_MYSQL_ADMIN_USER, &admin.user).await?;
    db::set_setting(pool, SETTING_MYSQL_ADMIN_PASSWORD, &admin.password).await
}

/// A private client config holding the login, removed when dropped. Keeps the
/// password out of the command line.
struct MysqlDefaults(PathBuf);

impl MysqlDefaults {
    fn new(admin: &MysqlAdmin) -> Result<Self, String> {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("linxpanel-my-{}.cnf", uuid::Uuid::new_v4()));
        let content = format!(
            "[client]\nuser={}\npassword=\"{}\"\n",
            admin.user, admin.password
        );
        write_private(&path, &content)?;
        Ok(Self(path))
    }

    fn arg(&self) -> String {
        format!("--defaults-extra-file={}", self.0.display())
    }
}

impl Drop for MysqlDefaults {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn write_private(path: &Path, content: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| e.to_string())?;
        file.write_all(content.as_bytes())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, content).map_err(|e| e.to_string())
    }
}

fn mysql_client() -> &'static str {
    if shell::has_binary("mariadb") {
        "mariadb"
    } else {
        "mysql"
    }
}

/// Runs SQL as the panel's admin login; returns tab-separated rows.
pub fn mysql_query(admin: &MysqlAdmin, sql: &str) -> Result<String, String> {
    let defaults = MysqlDefaults::new(admin)?;
    let arg = defaults.arg();
    // --defaults-extra-file must come first.
    let output = shell::run_with_input(
        mysql_client(),
        &[&arg, "--batch", "--skip-column-names"],
        Some(sql),
        SQL_TIMEOUT,
    )?;
    if output.ok() {
        Ok(output.stdout)
    } else {
        Err(output.error_text())
    }
}

/// First-time setup: log in as root over the unix socket (MariaDB's default for
/// root when run as root) and create the panel's own admin account.
pub fn bootstrap_mysql_admin() -> Result<MysqlAdmin, String> {
    if !shell::is_root() {
        return Err("Setting up MySQL administration requires the panel to run as root".into());
    }
    let password = generate_db_password();
    let sql = format!(
        "CREATE USER IF NOT EXISTS '{user}'@'localhost' IDENTIFIED BY '{pw}';\n\
         ALTER USER '{user}'@'localhost' IDENTIFIED BY '{pw}';\n\
         GRANT ALL PRIVILEGES ON *.* TO '{user}'@'localhost' WITH GRANT OPTION;\n\
         FLUSH PRIVILEGES;\n",
        user = PANEL_MYSQL_USER,
        pw = password
    );
    let output = shell::run_with_input(mysql_client(), &["-uroot"], Some(&sql), SQL_TIMEOUT)?;
    if !output.ok() {
        return Err(format!(
            "Cannot log in as MySQL root without a password ({}); enter the root password instead",
            output.error_text()
        ));
    }
    Ok(MysqlAdmin {
        user: PANEL_MYSQL_USER.into(),
        password,
    })
}

/// Checks a login supplied by the admin (for an existing MySQL with a root password).
pub fn verify_mysql_admin(admin: &MysqlAdmin) -> Result<(), String> {
    if admin.user.is_empty() || admin.user.len() > 80 || admin.user.contains(['\'', '"', '\n']) {
        return Err("Invalid user name".into());
    }
    if admin.password.contains(['"', '\n', '\\']) {
        return Err("Passwords with quotes, backslashes or newlines are not supported".into());
    }
    mysql_query(admin, "SELECT 1").map(|_| ())
}

const MYSQL_SYSTEM_DBS: &[&str] = &["information_schema", "mysql", "performance_schema", "sys"];

fn mysql_databases(admin: &MysqlAdmin) -> Result<Vec<String>, String> {
    let rows = mysql_query(admin, "SHOW DATABASES;")?;
    Ok(rows
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty() && !MYSQL_SYSTEM_DBS.contains(name))
        .map(String::from)
        .collect())
}

// ===== PostgreSQL =====

/// Runs SQL through `psql` as the `postgres` OS account (peer authentication).
pub(crate) fn psql(sql: &str, database: Option<&str>) -> Result<String, String> {
    let mut args: Vec<&str> = vec![
        "-u",
        "postgres",
        "--",
        "psql",
        "-v",
        "ON_ERROR_STOP=1",
        "-X",
        "-q",
        "-At",
    ];
    if let Some(database) = database {
        args.push("-d");
        args.push(database);
    }
    let output = if shell::is_root() && shell::has_binary("runuser") {
        shell::run_with_input("runuser", &args, Some(sql), SQL_TIMEOUT)?
    } else {
        let mut sudo_args = vec!["-n", "-u", "postgres", "--"];
        sudo_args.extend_from_slice(&args[3..]);
        shell::run_with_input("sudo", &sudo_args, Some(sql), SQL_TIMEOUT)?
    };
    if output.ok() {
        Ok(output.stdout)
    } else {
        Err(output.error_text())
    }
}

fn pg_databases() -> Result<Vec<String>, String> {
    let rows = psql(
        "SELECT datname FROM pg_database WHERE NOT datistemplate AND datname <> 'postgres' ORDER BY 1;",
        None,
    )?;
    Ok(rows
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(String::from)
        .collect())
}

// ===== Install / service =====

/// Installs and starts an engine; for MySQL also sets up the panel's admin login.
/// Returns the install log and, for MySQL, the new admin credentials.
/// Installs the engine; `version` is an id from `dbnet::versions` (None or
/// "default": the distro default packages).
pub fn install(engine: Engine, version: Option<&str>) -> (String, Option<MysqlAdmin>) {
    let mut log = format!("[LinxPanel] Installing {}\n", engine.display_name());
    super::tasks::note(&log);
    if engine == Engine::Mongodb {
        return install_mongodb(version, log);
    }
    let manager = runtime::package_manager();
    // MariaDB and MySQL (or two PostgreSQL clusters on one port) do not mix.
    if version.is_some_and(|v| !v.is_empty() && v != "default") && status(engine, None).installed {
        let line =
            "[LinxPanel] Already installed; remove the current version before choosing another\n";
        super::tasks::note(line);
        log.push_str(line);
        return (log, None);
    }
    let chosen = super::dbnet::packages_for(engine, manager, version);
    let chosen_refs: Vec<&str> = chosen.iter().flatten().map(String::as_str).collect();
    let packages: &[&str] = if chosen.is_some() {
        &chosen_refs
    } else {
        engine.packages(manager)
    };
    if packages.is_empty() {
        log.push_str("[LinxPanel] No install recipe for this system\n");
        return (log, None);
    }
    if !runtime::install_packages(packages, &mut log) {
        log.push_str("\n[LinxPanel] Package installation failed\n");
        return (log, None);
    }

    // Distros that do not initialise the data directory on install.
    match (engine, manager) {
        (Engine::Postgresql, "dnf" | "yum") => {
            run_logged("postgresql-setup", &["--initdb"], &mut log);
        }
        (Engine::Postgresql, "apk") => {
            run_logged("/etc/init.d/postgresql", &["setup"], &mut log);
        }
        (Engine::Mysql, "apk") => {
            run_logged("/etc/init.d/mariadb", &["setup"], &mut log);
        }
        _ => {}
    }

    let service = service_name(engine);
    match sysservice::enable_and_restart(&service) {
        Ok(text) => log.push_str(&text),
        Err(err) => {
            log.push_str(&format!("[LinxPanel] Cannot start {}: {}\n", service, err));
            return (log, None);
        }
    }

    let mut admin = None;
    if engine == Engine::Mysql {
        // The server needs a moment to open its socket after starting.
        for attempt in 0..10 {
            match bootstrap_mysql_admin() {
                Ok(created) => {
                    log.push_str("[LinxPanel] Created the panel's MySQL administrator account\n");
                    admin = Some(created);
                    break;
                }
                Err(err) if attempt == 9 => {
                    log.push_str(&format!("[LinxPanel] {}\n", err));
                }
                Err(_) => std::thread::sleep(Duration::from_secs(1)),
            }
        }
    }
    let ready = format!("\n[LinxPanel] {} is ready\n", engine.display_name());
    super::tasks::note(&ready);
    log.push_str(&ready);
    (log, admin)
}

/// MongoDB comes from MongoDB's own repository (distributions do not ship
/// it); once it runs, the panel creates its administrator and turns
/// authorization on.
fn install_mongodb(version: Option<&str>, mut log: String) -> (String, Option<MysqlAdmin>) {
    let version = match version {
        Some(v) if !v.is_empty() && v != "default" => v,
        _ => "8.0",
    };
    match mongo::install(version) {
        Ok(text) => log.push_str(&text),
        Err(err) => {
            log.push_str(&err);
            log.push_str("\n[LinxPanel] MongoDB installation failed\n");
            return (log, None);
        }
    }
    let mut admin = None;
    for attempt in 0..15 {
        match mongo::bootstrap_admin() {
            Ok(created) => {
                let line = "[LinxPanel] Created the panel's MongoDB administrator and enabled authorization\n";
                super::tasks::note(line);
                log.push_str(line);
                admin = Some(created);
                break;
            }
            Err(err) if attempt == 14 => {
                let line = format!("[LinxPanel] {}\n", err);
                super::tasks::note(&line);
                log.push_str(&line);
            }
            Err(_) => std::thread::sleep(Duration::from_secs(2)),
        }
    }
    let ready = "\n[LinxPanel] MongoDB is ready\n";
    super::tasks::note(ready);
    log.push_str(ready);
    (log, admin)
}

fn run_logged(program: &str, args: &[&str], log: &mut String) {
    log.push_str(&format!("$ {} {}\n", program, args.join(" ")));
    match shell::run_privileged_with_timeout(program, args, Duration::from_secs(300)) {
        Ok(output) => {
            log.push_str(output.stdout.trim_end());
            log.push_str(output.stderr.trim_end());
            log.push('\n');
        }
        Err(err) => log.push_str(&format!("{}\n", err)),
    }
}

pub fn restart(engine: Engine) -> Result<String, String> {
    sysservice::enable_and_restart(&service_name(engine))
}

// ===== Databases =====

/// A database row: what the server has, joined with what the panel recorded.
#[derive(Debug, Clone, Serialize)]
pub struct DatabaseRow {
    pub engine: Engine,
    pub name: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub host: Option<String>,
    pub note: Option<String>,
    /// Created through the panel (so its user and password are known).
    pub managed: bool,
    pub created_at: Option<String>,
    /// Number of tables, when the server reports it cheaply.
    pub tables: Option<u64>,
    /// Data plus indexes, in bytes.
    pub size: Option<u64>,
    /// Who can use a database the panel did not create: `user@host` for
    /// MySQL grants, the owner role for PostgreSQL.
    pub users: Vec<String>,
}

/// Size, table count and users of every database, in one or two queries.
#[derive(Debug, Default)]
struct DatabaseFacts {
    tables: std::collections::HashMap<String, u64>,
    size: std::collections::HashMap<String, u64>,
    users: std::collections::HashMap<String, Vec<String>>,
}

/// A MySQL string literal (names reaching here may come from the server,
/// e.g. databases created on the command line).
pub(crate) fn mysql_str(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// A PostgreSQL string literal.
pub(crate) fn pg_str(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn mysql_facts(admin: &MysqlAdmin) -> DatabaseFacts {
    let mut facts = DatabaseFacts::default();
    if let Ok(rows) = mysql_query(
        admin,
        "SELECT TABLE_SCHEMA, COUNT(*), COALESCE(SUM(DATA_LENGTH + INDEX_LENGTH), 0) \
         FROM information_schema.TABLES GROUP BY TABLE_SCHEMA;",
    ) {
        for line in rows.lines() {
            let mut cols = line.split('\t');
            if let (Some(db), Some(count), Some(size)) = (cols.next(), cols.next(), cols.next()) {
                facts.tables.insert(db.into(), count.parse().unwrap_or(0));
                facts.size.insert(db.into(), size.parse().unwrap_or(0));
            }
        }
    }
    if let Ok(rows) = mysql_query(admin, "SELECT Db, User, Host FROM mysql.db ORDER BY User;") {
        for line in rows.lines() {
            let mut cols = line.split('\t');
            if let (Some(db), Some(user), Some(host)) = (cols.next(), cols.next(), cols.next()) {
                // Grants store `_` and `%` escaped (they are wildcards there).
                let db = db.replace("\\_", "_").replace("\\%", "%");
                facts
                    .users
                    .entry(db)
                    .or_default()
                    .push(format!("{}@{}", user, host));
            }
        }
    }
    facts
}

fn pg_facts() -> DatabaseFacts {
    let mut facts = DatabaseFacts::default();
    if let Ok(rows) = psql(
        "SELECT datname || chr(9) || pg_database_size(datname) || chr(9) || pg_get_userbyid(datdba) \
         FROM pg_database WHERE NOT datistemplate;",
        None,
    ) {
        for line in rows.lines() {
            let mut cols = line.split('\t');
            if let (Some(db), Some(size), Some(owner)) = (cols.next(), cols.next(), cols.next()) {
                facts.size.insert(db.into(), size.parse().unwrap_or(0));
                facts.users.insert(db.into(), vec![owner.to_string()]);
            }
        }
    }
    facts
}

/// One table of a database.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TableInfo {
    pub name: String,
    /// An estimate for InnoDB / PostgreSQL; None when never analyzed.
    pub rows: Option<u64>,
    pub size: u64,
    pub engine: Option<String>,
    pub collation: Option<String>,
    pub comment: Option<String>,
    pub updated: Option<String>,
}

fn none_if_null(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty() && *v != "NULL")
        .map(String::from)
}

fn parse_mysql_tables(rows: &str) -> Vec<TableInfo> {
    rows.lines()
        .filter_map(|line| {
            let cols: Vec<&str> = line.split('\t').collect();
            let name = cols.first()?.to_string();
            Some(TableInfo {
                name,
                rows: cols.get(1).and_then(|v| v.parse().ok()),
                size: cols.get(2).and_then(|v| v.parse().ok()).unwrap_or(0),
                engine: none_if_null(cols.get(3).copied()),
                collation: none_if_null(cols.get(4).copied()),
                comment: none_if_null(cols.get(5).copied()),
                updated: none_if_null(cols.get(6).copied()),
            })
        })
        .collect()
}

fn parse_pg_tables(rows: &str) -> Vec<TableInfo> {
    rows.lines()
        .filter_map(|line| {
            let cols: Vec<&str> = line.split('\t').collect();
            let name = cols.first()?.to_string();
            Some(TableInfo {
                name,
                // -1: never vacuumed or analyzed, so unknown.
                rows: cols
                    .get(1)
                    .and_then(|v| v.parse::<f64>().ok())
                    .filter(|v| *v >= 0.0)
                    .map(|v| v as u64),
                size: cols.get(2).and_then(|v| v.parse().ok()).unwrap_or(0),
                engine: None,
                collation: None,
                comment: none_if_null(cols.get(3).copied()),
                updated: None,
            })
        })
        .collect()
}

/// The tables of a database, whoever created them.
pub async fn tables(
    engine: Engine,
    name: &str,
    admin: Option<MysqlAdmin>,
) -> Result<Vec<TableInfo>, String> {
    let name = name.to_string();
    tokio::task::spawn_blocking(move || match engine {
        Engine::Mysql => {
            let admin = admin.ok_or("MySQL is not set up for the panel")?;
            if !mysql_databases(&admin)?.contains(&name) {
                return Err(format!("Database {} does not exist", name));
            }
            let rows = mysql_query(
                &admin,
                &format!(
                    "SELECT TABLE_NAME, TABLE_ROWS, COALESCE(DATA_LENGTH + INDEX_LENGTH, 0), ENGINE, \
                     TABLE_COLLATION, TABLE_COMMENT, COALESCE(UPDATE_TIME, CREATE_TIME) \
                     FROM information_schema.TABLES WHERE TABLE_SCHEMA = {} ORDER BY TABLE_NAME;",
                    mysql_str(&name)
                ),
            )?;
            Ok(parse_mysql_tables(&rows))
        }
        Engine::Postgresql => {
            if !pg_databases()?.contains(&name) {
                return Err(format!("Database {} does not exist", name));
            }
            let rows = psql(
                "SELECT n.nspname || '.' || c.relname || chr(9) || c.reltuples::bigint || chr(9) \
                 || pg_total_relation_size(c.oid) || chr(9) || COALESCE(obj_description(c.oid, 'pg_class'), '') \
                 FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace \
                 WHERE c.relkind IN ('r', 'p') AND n.nspname NOT IN ('pg_catalog', 'information_schema') \
                 AND n.nspname NOT LIKE 'pg_toast%' ORDER BY 1;",
                Some(&name),
            )?;
            Ok(parse_pg_tables(&rows))
        }
        Engine::Redis => Err("Redis has no tables".into()),
        Engine::Mongodb => Ok(super::mongo::collections(admin.as_ref(), &name)?
            .into_iter()
            .map(|c| TableInfo {
                name: c.name,
                rows: Some(c.count),
                size: c.size,
                engine: None,
                collation: None,
                comment: None,
                updated: None,
            })
            .collect()),
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct DatabaseRecord {
    name: String,
    username: String,
    password: String,
    host: String,
    note: String,
    created_at: String,
}

pub async fn list(
    pool: &SqlitePool,
    engine: Engine,
    admin: Option<MysqlAdmin>,
) -> Result<Vec<DatabaseRow>, String> {
    let (names, facts) = tokio::task::spawn_blocking(move || match engine {
        Engine::Mysql => {
            let admin = admin.ok_or_else(|| "MySQL is not set up for the panel".to_string())?;
            Ok((mysql_databases(&admin)?, mysql_facts(&admin)))
        }
        Engine::Postgresql => Ok((pg_databases()?, pg_facts())),
        Engine::Redis => Ok((Vec::new(), DatabaseFacts::default())),
        Engine::Mongodb => {
            let mut facts = DatabaseFacts::default();
            let mut names = Vec::new();
            for info in super::mongo::databases(admin.as_ref())? {
                facts.tables.insert(info.name.clone(), info.collections);
                facts.size.insert(info.name.clone(), info.size);
                facts.users.insert(info.name.clone(), info.users);
                names.push(info.name);
            }
            Ok((names, facts))
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e: String| e)?;

    let records: Vec<DatabaseRecord> = sqlx::query_as(
        "SELECT name, username, password, host, note, created_at
         FROM databases WHERE engine = ?",
    )
    .bind(engine.as_str())
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut names = names;
    // MongoDB creates a database with its first collection: until then the
    // panel's record is all there is, and it is still listed.
    if engine == Engine::Mongodb {
        for record in &records {
            if !names.contains(&record.name) {
                names.push(record.name.clone());
            }
        }
    }
    let mut rows: Vec<DatabaseRow> = names
        .into_iter()
        .map(|name| {
            let record = records.iter().find(|r| r.name == name);
            DatabaseRow {
                engine,
                managed: record.is_some(),
                username: record.map(|r| r.username.clone()),
                password: record.map(|r| r.password.clone()),
                host: record.map(|r| r.host.clone()),
                note: record.map(|r| r.note.clone()),
                created_at: record.map(|r| r.created_at.clone()),
                // An empty MySQL database has no row in information_schema.TABLES.
                tables: match engine {
                    Engine::Mysql | Engine::Mongodb => {
                        Some(facts.tables.get(&name).copied().unwrap_or(0))
                    }
                    _ => None,
                },
                size: facts.size.get(&name).copied().or(match engine {
                    Engine::Mysql | Engine::Mongodb => Some(0),
                    _ => None,
                }),
                users: facts.users.get(&name).cloned().unwrap_or_default(),
                name,
            }
        })
        .collect();
    rows.sort_by(|a, b| b.managed.cmp(&a.managed).then(a.name.cmp(&b.name)));
    Ok(rows)
}

#[derive(Debug, Deserialize)]
pub struct CreateDatabaseRequest {
    pub engine: String,
    pub name: String,
    /// Defaults to the database name.
    #[serde(default)]
    pub username: String,
    /// Generated when empty.
    #[serde(default)]
    pub password: String,
    /// MySQL only: localhost (default), % or an IP.
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatedDatabase {
    pub engine: Engine,
    pub name: String,
    pub username: String,
    pub password: String,
    pub host: String,
}

pub async fn create(
    pool: &SqlitePool,
    request: CreateDatabaseRequest,
    admin: Option<MysqlAdmin>,
) -> Result<CreatedDatabase, String> {
    let engine = Engine::parse(&request.engine)?;
    if engine == Engine::Redis {
        return Err("Redis has no databases to create".into());
    }
    let name = request.name.trim().to_string();
    validate_name(&name, "Database name")?;
    let username = match request.username.trim() {
        "" => name.clone(),
        user => user.to_string(),
    };
    validate_name(&username, "User name")?;
    let password = match request.password.trim() {
        "" => generate_db_password(),
        pw => pw.to_string(),
    };
    validate_db_password(&password)?;
    let host = match request.host.trim() {
        "" => "localhost".to_string(),
        host => host.to_string(),
    };
    validate_host(&host)?;

    let (n, u, p, h) = (
        name.clone(),
        username.clone(),
        password.clone(),
        host.clone(),
    );
    tokio::task::spawn_blocking(move || match engine {
        Engine::Mysql => {
            let admin = admin.ok_or("MySQL is not set up for the panel")?;
            if mysql_databases(&admin)?.contains(&n) {
                return Err(format!("Database {} already exists", n));
            }
            mysql_query(
                &admin,
                &format!(
                    "CREATE DATABASE `{n}` CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;\n\
                     CREATE USER IF NOT EXISTS '{u}'@'{h}' IDENTIFIED BY '{p}';\n\
                     ALTER USER '{u}'@'{h}' IDENTIFIED BY '{p}';\n\
                     GRANT ALL PRIVILEGES ON `{n}`.* TO '{u}'@'{h}';\n\
                     FLUSH PRIVILEGES;\n"
                ),
            )
            .map(|_| ())
        }
        Engine::Postgresql => {
            if pg_databases()?.contains(&n) {
                return Err(format!("Database {} already exists", n));
            }
            // Role and database names are allowlisted, so quoting is not needed;
            // CREATE DATABASE cannot run inside a DO block, hence two calls.
            psql(
                &format!(
                    "DO $$ BEGIN\n\
                       IF EXISTS (SELECT FROM pg_roles WHERE rolname = '{u}') THEN\n\
                         ALTER ROLE {u} WITH LOGIN PASSWORD '{p}';\n\
                       ELSE\n\
                         CREATE ROLE {u} WITH LOGIN PASSWORD '{p}';\n\
                       END IF;\n\
                     END $$;\n"
                ),
                None,
            )?;
            psql(
                &format!("CREATE DATABASE {n} OWNER {u} ENCODING 'UTF8' TEMPLATE template0;\n"),
                None,
            )
            .map(|_| ())
        }
        Engine::Redis => unreachable!(),
        Engine::Mongodb => super::mongo::create(admin.as_ref(), &n, &u, &p),
    })
    .await
    .map_err(|e| e.to_string())??;

    sqlx::query(
        "INSERT INTO databases (engine, name, username, password, host, note)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(engine, name) DO UPDATE SET username = excluded.username,
           password = excluded.password, host = excluded.host, note = excluded.note",
    )
    .bind(engine.as_str())
    .bind(&name)
    .bind(&username)
    .bind(&password)
    .bind(&host)
    .bind(request.note.trim())
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(CreatedDatabase {
        engine,
        name,
        username,
        password,
        host,
    })
}

#[derive(Debug, Deserialize)]
pub struct AdoptRequest {
    /// Defaults to the database name.
    #[serde(default)]
    pub username: String,
    pub password: String,
    /// MySQL only: localhost (default), % or an IP.
    #[serde(default)]
    pub host: String,
    /// When the user already exists: set this password (its applications
    /// then need it too) instead of checking that it is the current one.
    #[serde(default)]
    pub reset_password: bool,
}

/// Takes over a database created outside the panel (e.g. on the command
/// line): gives it a user the panel knows, so passwords, access and the
/// connection details can be managed from here like any other.
pub async fn adopt(
    pool: &SqlitePool,
    engine: Engine,
    name: &str,
    request: AdoptRequest,
    admin: Option<MysqlAdmin>,
) -> Result<CreatedDatabase, String> {
    validate_name(name, "Database name").map_err(|e| {
        format!(
            "{} (only letters, digits and _ can be managed by the panel)",
            e
        )
    })?;
    if record(pool, engine, name).await.is_some() {
        return Err("This database is already managed by the panel".into());
    }
    let username = match request.username.trim() {
        "" => name.to_string(),
        user => user.to_string(),
    };
    validate_name(&username, "User name")?;
    let password = request.password.trim().to_string();
    validate_db_password(&password)?;
    let host = match request.host.trim() {
        "" => "localhost".to_string(),
        host => host.to_string(),
    };
    validate_host(&host)?;

    let (n, u, p, h, reset) = (
        name.to_string(),
        username.clone(),
        password.clone(),
        host.clone(),
        request.reset_password,
    );
    tokio::task::spawn_blocking(move || match engine {
        Engine::Mysql => {
            let admin = admin.ok_or("MySQL is not set up for the panel")?;
            if !mysql_databases(&admin)?.contains(&n) {
                return Err(format!("Database {} does not exist", n));
            }
            let exists = mysql_query(
                &admin,
                &format!(
                    "SELECT COUNT(*) FROM mysql.user WHERE User = {} AND Host = {};",
                    mysql_str(&u),
                    mysql_str(&h)
                ),
            )?
            .trim()
                != "0";
            let mut sql = String::new();
            if !exists {
                sql.push_str(&format!("CREATE USER '{u}'@'{h}' IDENTIFIED BY '{p}';\n"));
            } else if reset {
                sql.push_str(&format!("ALTER USER '{u}'@'{h}' IDENTIFIED BY '{p}';\n"));
            } else if h == "localhost" {
                // Keep the user's password: only record it once it is proven.
                mysql_query(
                    &MysqlAdmin {
                        user: u.clone(),
                        password: p.clone(),
                    },
                    "SELECT 1",
                )
                .map_err(|_| {
                    format!(
                        "User {u} already exists and this is not its password. \
                         Enter its current password, or choose to set a new one."
                    )
                })?;
            } else {
                return Err(format!(
                    "User {u}@{h} already exists and its password cannot be checked from here. \
                     Choose to set a new password, or use another user name."
                ));
            }
            sql.push_str(&format!(
                "GRANT ALL PRIVILEGES ON `{n}`.* TO '{u}'@'{h}';\nFLUSH PRIVILEGES;\n"
            ));
            mysql_query(&admin, &sql).map(|_| ())
        }
        Engine::Postgresql => {
            if !pg_databases()?.contains(&n) {
                return Err(format!("Database {} does not exist", n));
            }
            let exists = psql(
                &format!("SELECT COUNT(*) FROM pg_roles WHERE rolname = {};", pg_str(&u)),
                None,
            )?
            .trim()
                != "0";
            if exists && !reset {
                return Err(format!(
                    "Role {u} already exists. Choose to set a new password, or use another user name."
                ));
            }
            let verb = if exists { "ALTER" } else { "CREATE" };
            psql(
                &format!(
                    "{verb} ROLE {u} WITH LOGIN PASSWORD '{p}';\n\
                     GRANT ALL PRIVILEGES ON DATABASE {n} TO {u};\n\
                     ALTER DATABASE {n} OWNER TO {u};\n"
                ),
                None,
            )?;
            // Owning the database does not reach the tables already in it
            // (they keep their creator): grant those, schema by schema.
            psql(
                &format!(
                    "DO $$ DECLARE s text; BEGIN\n\
                       FOR s IN SELECT nspname FROM pg_namespace\n\
                         WHERE nspname NOT LIKE 'pg\\_%' AND nspname <> 'information_schema' LOOP\n\
                         EXECUTE format('GRANT ALL ON SCHEMA %I TO {u}', s);\n\
                         EXECUTE format('GRANT ALL ON ALL TABLES IN SCHEMA %I TO {u}', s);\n\
                         EXECUTE format('GRANT ALL ON ALL SEQUENCES IN SCHEMA %I TO {u}', s);\n\
                       END LOOP;\n\
                     END $$;\n"
                ),
                Some(&n),
            )
            .map(|_| ())
        }
        Engine::Redis => Err("Redis has no databases".into()),
        Engine::Mongodb => {
            let existing = super::mongo::databases(admin.as_ref())?;
            if !existing.iter().any(|d| d.name == n) {
                return Err(format!("Database {} does not exist", n));
            }
            super::mongo::adopt(admin.as_ref(), &n, &u, &p, reset)
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    sqlx::query(
        "INSERT INTO databases (engine, name, username, password, host, note)
         VALUES (?, ?, ?, ?, ?, '')
         ON CONFLICT(engine, name) DO UPDATE SET username = excluded.username,
           password = excluded.password, host = excluded.host",
    )
    .bind(engine.as_str())
    .bind(name)
    .bind(&username)
    .bind(&password)
    .bind(&host)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(CreatedDatabase {
        engine,
        name: name.to_string(),
        username,
        password,
        host,
    })
}

async fn record(pool: &SqlitePool, engine: Engine, name: &str) -> Option<DatabaseRecord> {
    sqlx::query_as(
        "SELECT name, username, password, host, note, created_at
         FROM databases WHERE engine = ? AND name = ?",
    )
    .bind(engine.as_str())
    .bind(name)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
}

/// Drops the database, and its user when the panel created it.
pub async fn delete(
    pool: &SqlitePool,
    engine: Engine,
    name: &str,
    admin: Option<MysqlAdmin>,
) -> Result<(), String> {
    validate_name(name, "Database name")?;
    let user = record(pool, engine, name)
        .await
        .map(|r| (r.username, r.host));
    let n = name.to_string();
    tokio::task::spawn_blocking(move || match engine {
        Engine::Mysql => {
            let admin = admin.ok_or("MySQL is not set up for the panel")?;
            let mut sql = format!("DROP DATABASE IF EXISTS `{}`;\n", n);
            if let Some((user, host)) = &user {
                sql.push_str(&format!("DROP USER IF EXISTS '{}'@'{}';\n", user, host));
            }
            mysql_query(&admin, &sql).map(|_| ())
        }
        Engine::Postgresql => {
            // End open sessions first, or DROP DATABASE refuses.
            psql(
                &format!(
                    "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = '{n}';\n\
                     DROP DATABASE IF EXISTS {n};\n"
                ),
                None,
            )?;
            if let Some((user, _)) = &user {
                // The role may own objects elsewhere; keep it in that case.
                let _ = psql(&format!("DROP ROLE IF EXISTS {};\n", user), None);
            }
            Ok(())
        }
        Engine::Redis => Err("Redis has no databases to delete".into()),
        Engine::Mongodb => super::mongo::drop(
            admin.as_ref(),
            &n,
            user.as_ref().map(|(user, _)| user.as_str()),
        ),
    })
    .await
    .map_err(|e| e.to_string())??;

    sqlx::query("DELETE FROM databases WHERE engine = ? AND name = ?")
        .bind(engine.as_str())
        .bind(name)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Sets a new password for the database's user; returns it.
/// Changes where a panel-created MySQL user may connect from
/// (localhost / % / an IP), keeping its password and grants.
pub async fn change_host(
    pool: &SqlitePool,
    name: &str,
    host: &str,
    admin: Option<MysqlAdmin>,
) -> Result<(), String> {
    let host = host.trim().to_string();
    validate_host(&host)?;
    let record = record(pool, Engine::Mysql, name)
        .await
        .ok_or("This database was not created by the panel, so its user is unknown")?;
    if record.host == host {
        return Ok(());
    }
    let (u, old, new) = (record.username.clone(), record.host.clone(), host.clone());
    tokio::task::spawn_blocking(move || {
        let admin = admin.ok_or("MySQL is not set up for the panel")?;
        mysql_query(
            &admin,
            &format!("RENAME USER '{u}'@'{old}' TO '{u}'@'{new}';\nFLUSH PRIVILEGES;\n"),
        )
        .map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())??;
    sqlx::query("UPDATE databases SET host = ? WHERE engine = 'mysql' AND name = ?")
        .bind(&host)
        .bind(name)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn change_password(
    pool: &SqlitePool,
    engine: Engine,
    name: &str,
    password: Option<String>,
    admin: Option<MysqlAdmin>,
) -> Result<String, String> {
    let record = record(pool, engine, name)
        .await
        .ok_or("This database was not created by the panel, so its user is unknown")?;
    let password = password
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .unwrap_or_else(generate_db_password);
    validate_db_password(&password)?;

    let (u, h, p, n) = (
        record.username.clone(),
        record.host.clone(),
        password.clone(),
        name.to_string(),
    );
    tokio::task::spawn_blocking(move || match engine {
        Engine::Mysql => {
            let admin = admin.ok_or("MySQL is not set up for the panel")?;
            mysql_query(
                &admin,
                &format!("ALTER USER '{u}'@'{h}' IDENTIFIED BY '{p}';\nFLUSH PRIVILEGES;\n"),
            )
            .map(|_| ())
        }
        Engine::Postgresql => {
            psql(&format!("ALTER ROLE {u} WITH PASSWORD '{p}';\n"), None).map(|_| ())
        }
        Engine::Redis => Err("Use the Redis password setting".into()),
        Engine::Mongodb => super::mongo::change_password(admin.as_ref(), &n, &u, &p),
    })
    .await
    .map_err(|e| e.to_string())??;

    sqlx::query("UPDATE databases SET password = ? WHERE engine = ? AND name = ?")
        .bind(&password)
        .bind(engine.as_str())
        .bind(name)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(password)
}

// ===== Redis =====

pub(crate) fn redis_conf() -> Option<PathBuf> {
    [
        "/etc/redis/redis.conf",
        "/etc/redis.conf",
        "/etc/redis/redis-server.conf",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|path| path.exists())
}

/// Sets (or with an empty value clears) Redis's `requirepass` and restarts it.
pub fn set_redis_password(password: &str) -> Result<(), String> {
    if !password.is_empty() {
        validate_db_password(password)?;
    }
    let conf = redis_conf().ok_or("redis.conf not found")?;
    let text = std::fs::read_to_string(&conf).map_err(|e| e.to_string())?;
    let mut lines: Vec<String> = text
        .lines()
        .filter(|line| !line.trim_start().starts_with("requirepass "))
        .map(String::from)
        .collect();
    if !password.is_empty() {
        lines.push(format!("requirepass {}", password));
    }
    std::fs::write(&conf, lines.join("\n") + "\n").map_err(|e| e.to_string())?;
    restart(Engine::Redis).map(|_| ())
}

pub fn redis_password() -> Option<String> {
    let text = std::fs::read_to_string(redis_conf()?).ok()?;
    text.lines()
        .find_map(|line| line.trim().strip_prefix("requirepass "))
        .map(|value| value.trim().trim_matches('"').to_string())
}

// ===== Backups =====

#[derive(Debug, Clone, Serialize)]
pub struct BackupFile {
    pub file: String,
    pub path: String,
    pub size: u64,
    pub created_at: String,
}

pub fn backup_dir(data_dir: &Path, engine: Engine, name: &str) -> PathBuf {
    data_dir
        .join("backups")
        .join("database")
        .join(engine.as_str())
        .join(name)
}

pub fn list_backups(dir: &Path) -> Vec<BackupFile> {
    let mut files: Vec<BackupFile> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let meta = entry.metadata().ok()?;
                    if !meta.is_file() {
                        return None;
                    }
                    let modified: chrono::DateTime<chrono::Local> = meta.modified().ok()?.into();
                    Some(BackupFile {
                        file: entry.file_name().to_string_lossy().to_string(),
                        path: entry.path().to_string_lossy().to_string(),
                        size: meta.len(),
                        created_at: modified.format("%Y-%m-%d %H:%M:%S").to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.file.cmp(&a.file)));
    files
}

/// A backup file name must be a plain file name inside the backup directory.
pub fn validate_backup_file(file: &str) -> Result<(), String> {
    let ok = !file.is_empty()
        && file.len() <= 200
        && !file.starts_with('.')
        && file
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    if ok {
        Ok(())
    } else {
        Err("Invalid backup file name".into())
    }
}

/// Runs `producer` and pipes its stdout through gzip into `target`.
fn dump_to_gzip(mut producer: Command, target: &Path) -> Result<(), String> {
    let file = std::fs::File::create(target).map_err(|e| e.to_string())?;
    let mut dump = producer
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Cannot run dump: {}", e))?;
    let dump_out = dump.stdout.take().ok_or("dump has no stdout")?;
    let mut gzip = Command::new("gzip")
        .arg("-c")
        .stdin(Stdio::from(dump_out))
        .stdout(Stdio::from(file))
        .spawn()
        .map_err(|e| format!("Cannot run gzip: {}", e))?;
    let dump_status = wait_with_timeout(&mut dump, DUMP_TIMEOUT)?;
    let mut dump_err = String::new();
    if let Some(mut stderr) = dump.stderr.take() {
        use std::io::Read;
        let _ = stderr.read_to_string(&mut dump_err);
    }
    let gzip_status = gzip.wait().map_err(|e| e.to_string())?;
    if !dump_status.success() || !gzip_status.success() {
        let _ = std::fs::remove_file(target);
        return Err(format!("Backup failed: {}", dump_err.trim()));
    }
    Ok(())
}

/// Pipes `gunzip -c source` into `consumer`'s stdin.
fn restore_from_gzip(mut consumer: Command, source: &Path) -> Result<(), String> {
    let mut gunzip = Command::new("gzip")
        .args(["-dc"])
        .arg(source)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Cannot run gzip: {}", e))?;
    let sql = gunzip.stdout.take().ok_or("gzip has no stdout")?;
    let mut client = consumer
        .stdin(Stdio::from(sql))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Cannot run client: {}", e))?;
    let status = wait_with_timeout(&mut client, DUMP_TIMEOUT)?;
    let _ = gunzip.wait();
    if !status.success() {
        let mut err = String::new();
        if let Some(mut stderr) = client.stderr.take() {
            use std::io::Read;
            let _ = stderr.read_to_string(&mut err);
        }
        return Err(format!("Restore failed: {}", err.trim()));
    }
    Ok(())
}

fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Result<std::process::ExitStatus, String> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok(status);
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            return Err("Timed out".into());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn postgres_command(program: &str) -> Command {
    let mut command = if shell::is_root() && shell::has_binary("runuser") {
        let mut c = Command::new("runuser");
        c.args(["-u", "postgres", "--", program]);
        c
    } else {
        let mut c = Command::new("sudo");
        c.args(["-n", "-u", "postgres", "--", program]);
        c
    };
    // psql/pg_dump refuse to start from a directory postgres cannot read.
    command.current_dir("/");
    command
}

/// Dumps a database to `<backup dir>/<name>_<timestamp>.sql.gz`; returns the path.
/// Keeps only the newest `keep` backups when given.
pub fn backup(
    data_dir: &Path,
    engine: Engine,
    name: &str,
    admin: Option<&MysqlAdmin>,
    keep: Option<usize>,
) -> Result<PathBuf, String> {
    validate_name(name, "Database name")?;
    let dir = backup_dir(data_dir, engine, name);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let ext = if engine == Engine::Mongodb {
        "archive.gz"
    } else {
        "sql.gz"
    };
    let target = dir.join(format!("{}_{}.{}", name, stamp, ext));

    match engine {
        Engine::Mysql => {
            let admin = admin.ok_or("MySQL is not set up for the panel")?;
            let defaults = MysqlDefaults::new(admin)?;
            let dumper = if shell::has_binary("mariadb-dump") {
                "mariadb-dump"
            } else {
                "mysqldump"
            };
            let mut command = Command::new(dumper);
            command.arg(defaults.arg()).args([
                "--single-transaction",
                "--routines",
                "--triggers",
                "--events",
                "--default-character-set=utf8mb4",
                "--databases",
                name,
            ]);
            dump_to_gzip(command, &target)?;
            drop(defaults);
        }
        Engine::Postgresql => {
            let mut command = postgres_command("pg_dump");
            // Ownership is kept so restored tables still belong to the app's role.
            command.args(["--clean", "--if-exists", name]);
            dump_to_gzip(command, &target)?;
        }
        Engine::Redis => return Err("Redis backups are not supported".into()),
        Engine::Mongodb => {
            let admin = admin.ok_or("MongoDB is not set up for the panel")?;
            let auth = super::mongo::ToolAuth::new(admin)?;
            dump_to_gzip(super::mongo::dump_command(&auth, admin, name), &target)?;
        }
    }

    if let Some(keep) = keep.filter(|k| *k > 0) {
        for old in list_backups(&dir).into_iter().skip(keep) {
            let _ = std::fs::remove_file(old.path);
        }
    }
    Ok(target)
}

pub fn restore(
    data_dir: &Path,
    engine: Engine,
    name: &str,
    file: &str,
    admin: Option<&MysqlAdmin>,
) -> Result<(), String> {
    validate_name(name, "Database name")?;
    validate_backup_file(file)?;
    let source = backup_dir(data_dir, engine, name).join(file);
    if !source.is_file() {
        return Err("Backup file not found".into());
    }
    match engine {
        Engine::Mysql => {
            let admin = admin.ok_or("MySQL is not set up for the panel")?;
            let defaults = MysqlDefaults::new(admin)?;
            let mut command = Command::new(mysql_client());
            command.arg(defaults.arg()).arg(name);
            restore_from_gzip(command, &source)
        }
        Engine::Postgresql => {
            let mut command = postgres_command("psql");
            command.args(["-v", "ON_ERROR_STOP=0", "-q", "-d", name]);
            restore_from_gzip(command, &source)
        }
        Engine::Redis => Err("Redis backups are not supported".into()),
        Engine::Mongodb => {
            let admin = admin.ok_or("MongoDB is not set up for the panel")?;
            let auth = super::mongo::ToolAuth::new(admin)?;
            restore_from_gzip(super::mongo::restore_command(&auth, admin, name), &source)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_passwords_are_allowlisted() {
        assert!(validate_name("wp_site1", "x").is_ok());
        assert!(validate_name("1abc", "x").is_err());
        assert!(validate_name("a-b", "x").is_err());
        assert!(validate_name("a`b", "x").is_err());
        assert!(validate_name(&"a".repeat(33), "x").is_err());
        assert!(validate_db_password(&generate_db_password()).is_ok());
        assert!(validate_db_password("short").is_err());
        assert!(validate_db_password("has'quote1").is_err());
        assert!(validate_db_password("has space1").is_err());
        assert!(validate_host("localhost").is_ok());
        assert!(validate_host("%").is_ok());
        assert!(validate_host("10.0.0.%").is_ok());
        assert!(validate_host("a'b").is_err());
        assert!(validate_backup_file("wp_20260101_120000.sql.gz").is_ok());
        assert!(validate_backup_file("../etc/passwd").is_err());
        assert!(validate_backup_file(".hidden").is_err());
    }

    #[test]
    fn engines_parse() {
        assert_eq!(Engine::parse("mariadb").unwrap(), Engine::Mysql);
        assert_eq!(Engine::parse("postgres").unwrap(), Engine::Postgresql);
        assert!(Engine::parse("oracle").is_err());
    }

    #[test]
    fn string_literals_are_escaped() {
        assert_eq!(mysql_str("shop"), "'shop'");
        assert_eq!(mysql_str("a'b\\c"), "'a\\'b\\\\c'");
        assert_eq!(pg_str("o'k"), "'o''k'");
    }

    #[test]
    fn parses_mysql_tables() {
        let out = "orders\t1200\t49152\tInnoDB\tutf8mb4_unicode_ci\tCustomer orders\t2026-10-27 05:00:00\n\
                   empty\tNULL\t0\tNULL\tNULL\t\tNULL\n";
        let tables = parse_mysql_tables(out);
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0].name, "orders");
        assert_eq!(tables[0].rows, Some(1200));
        assert_eq!(tables[0].size, 49152);
        assert_eq!(tables[0].engine.as_deref(), Some("InnoDB"));
        assert_eq!(tables[0].comment.as_deref(), Some("Customer orders"));
        assert_eq!(tables[1].rows, None);
        assert_eq!(tables[1].engine, None);
        assert_eq!(tables[1].comment, None);
        assert_eq!(tables[1].updated, None);
    }

    #[test]
    fn parses_pg_tables() {
        let tables =
            parse_pg_tables("public.users\t42\t16384\t\npublic.fresh\t-1\t8192\tnew one\n");
        assert_eq!(tables[0].name, "public.users");
        assert_eq!(tables[0].rows, Some(42));
        assert_eq!(tables[1].rows, None);
        assert_eq!(tables[1].comment.as_deref(), Some("new one"));
    }
}
