//! MongoDB, through `mongosh` and the database tools that ship with it.
//!
//! - Credentials never reach a command line (where `ps` shows them): each
//!   script runs from a private temporary file that connects first, and the
//!   dump tools read their password from a private config file.
//! - The panel keeps its own administrator (`linxpanel`, role root) and turns
//!   on authorization once that exists, so a MongoDB opened to the network is
//!   never open without a password.
//! - Each panel database gets a user of its own (readWrite + dbAdmin on it),
//!   authenticated against that database.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Deserialize;

use super::database::MysqlAdmin as DbAdmin;
use super::{shell, sysservice};

pub const SETTING_ADMIN_USER: &str = "mongodb_admin_user";
pub const SETTING_ADMIN_PASSWORD: &str = "mongodb_admin_password";
pub const SETTING_ROOT_PASSWORD: &str = "mongodb_root_password";
const PANEL_USER: &str = "linxpanel";
pub const INSTALL_SCRIPT: &str = include_str!("../../scripts/mongodb-install.sh");
const EVAL_TIMEOUT: Duration = Duration::from_secs(90);
const SYSTEM_DBS: &[&str] = &["admin", "config", "local"];

pub fn conf_path() -> PathBuf {
    ["/etc/mongod.conf", "/etc/mongodb.conf"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("/etc/mongod.conf"))
}

fn conf_text() -> String {
    std::fs::read_to_string(conf_path()).unwrap_or_default()
}

pub fn port() -> u16 {
    yaml_get(&conf_text(), &["net", "port"])
        .and_then(|p| p.parse().ok())
        .unwrap_or(27017)
}

pub fn server_present() -> bool {
    shell::has_binary("mongod")
}

pub fn client_present() -> bool {
    shell::has_binary("mongosh")
}

/// A JavaScript string literal (JSON strings are valid JS).
fn js(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

fn uri(user: Option<(&str, &str, &str)>) -> String {
    let base = format!("127.0.0.1:{}", port());
    match user {
        Some((user, password, auth_db)) => format!(
            "mongodb://{}:{}@{}/{}?authSource={}&directConnection=true&serverSelectionTimeoutMS=8000",
            percent_encode(user),
            percent_encode(password),
            base,
            auth_db,
            auth_db
        ),
        None => format!(
            "mongodb://{}/admin?directConnection=true&serverSelectionTimeoutMS=8000",
            base
        ),
    }
}

/// Runs a script as the panel's administrator (or without credentials when
/// authorization is not on yet). The script's printed output is returned.
pub fn eval(admin: Option<&DbAdmin>, script: &str) -> Result<String, String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if !client_present() {
        return Err("mongosh is not installed".into());
    }
    let connect = uri(admin.map(|a| (a.user.as_str(), a.password.as_str(), "admin")));
    let body = format!(
        "db = connect({});\ntry {{\n{}\n}} catch (e) {{ print('LP_ERROR ' + (e.codeName ? e.codeName + ': ' : '') + e.message); quit(3); }}\n",
        js(&connect),
        script
    );
    let path = std::env::temp_dir().join(format!(
        "linxpanel-mongo-{}.js",
        uuid::Uuid::new_v4().simple()
    ));
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| e.to_string())?;
        file.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    }
    let mut command = Command::new("mongosh");
    command.args(["--quiet", "--nodb", "--norc"]).arg(&path);
    let output = shell::run_command(command, None, EVAL_TIMEOUT);
    let _ = std::fs::remove_file(&path);
    let output = output?;
    if output.ok() {
        return Ok(output.stdout.trim().to_string());
    }
    let text = format!("{}\n{}", output.stdout, output.stderr);
    let message = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("LP_ERROR "))
        .map(String::from)
        .or_else(|| {
            text.lines()
                .map(str::trim)
                .rfind(|l| l.contains("Error") || l.contains("error"))
                .map(String::from)
        })
        .unwrap_or_else(|| output.error_text());
    Err(message)
}

/// Runs a script whose last printed line is JSON.
fn eval_json<T: DeserializeOwned>(admin: Option<&DbAdmin>, script: &str) -> Result<T, String> {
    let text = eval(admin, script)?;
    let line = text.lines().last().unwrap_or_default();
    serde_json::from_str(line).map_err(|e| format!("Unexpected mongosh output ({}): {}", e, line))
}

pub fn ping(admin: Option<&DbAdmin>) -> Result<(), String> {
    eval(admin, "db.adminCommand({ ping: 1 }); print('ok');").map(|_| ())
}

pub fn auth_enabled() -> bool {
    yaml_get(&conf_text(), &["security", "authorization"]).as_deref() == Some("enabled")
}

/// Creates (or resets) the panel's administrator and turns authorization
/// on. Works on a fresh server, and on one with authorization already on as
/// long as no user exists yet (MongoDB's localhost exception).
pub fn bootstrap_admin() -> Result<DbAdmin, String> {
    let password = super::database::generate_db_password();
    let script = format!(
        "const a = db.getSiblingDB('admin');\n\
         if (a.getUser({u})) {{ a.updateUser({u}, {{ pwd: {p}, roles: [{{ role: 'root', db: 'admin' }}] }}); }}\n\
         else {{ a.createUser({{ user: {u}, pwd: {p}, roles: [{{ role: 'root', db: 'admin' }}] }}); }}\n\
         print('ok');",
        u = js(PANEL_USER),
        p = js(&password)
    );
    eval(None, &script).map_err(|e| {
        format!(
            "Cannot create the panel's MongoDB administrator ({}); if MongoDB already has users, enter an administrator login instead",
            e
        )
    })?;
    let admin = DbAdmin {
        user: PANEL_USER.into(),
        password,
    };
    if !auth_enabled() {
        let path = conf_path();
        let previous = conf_text();
        let next = yaml_set(&previous, &["security", "authorization"], Some("enabled"));
        std::fs::write(&path, next).map_err(|e| format!("{}: {}", path.display(), e))?;
        sysservice::enable_and_restart("mongod")?;
        wait_ready(Some(&admin));
    }
    ping(Some(&admin))?;
    Ok(admin)
}

/// Waits (up to ~20s) until mongod answers.
pub fn wait_ready(admin: Option<&DbAdmin>) -> bool {
    for _ in 0..20 {
        if ping(admin).is_ok() {
            return true;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    false
}

// ===== Databases =====

#[derive(Debug, Deserialize)]
pub struct DbInfo {
    pub name: String,
    pub size: u64,
    pub collections: u64,
    #[serde(default)]
    pub users: Vec<String>,
}

pub fn databases(admin: Option<&DbAdmin>) -> Result<Vec<DbInfo>, String> {
    let script = format!(
        "const skip = {skip};\n\
         const users = {{}};\n\
         try {{ (db.adminCommand({{ usersInfo: {{ forAllDBs: true }} }}).users || []).forEach(u => \
           (u.roles || []).forEach(r => {{ (users[r.db] = users[r.db] || []).push(u.user + '@' + u.db); }})); }} catch (e) {{}}\n\
         const out = (db.adminCommand({{ listDatabases: 1 }}).databases || [])\n\
           .filter(d => !skip.includes(d.name))\n\
           .map(d => ({{ name: d.name, size: Number(d.sizeOnDisk || 0),\n\
             collections: db.getSiblingDB(d.name).getCollectionNames().length,\n\
             users: [...new Set(users[d.name] || [])] }}));\n\
         print(JSON.stringify(out));",
        skip = serde_json::to_string(SYSTEM_DBS).unwrap_or_default()
    );
    eval_json(admin, &script)
}

/// A database with its own user. MongoDB creates the database itself with
/// the first collection; until then the panel lists it from its records.
pub fn create(
    admin: Option<&DbAdmin>,
    name: &str,
    user: &str,
    password: &str,
) -> Result<(), String> {
    let script = format!(
        "const d = db.getSiblingDB({n});\n\
         if (d.getUser({u})) throw new Error('User ' + {u} + ' already exists in ' + {n});\n\
         d.createUser({{ user: {u}, pwd: {p}, roles: [{{ role: 'readWrite', db: {n} }}, {{ role: 'dbAdmin', db: {n} }}] }});\n\
         print('ok');",
        n = js(name),
        u = js(user),
        p = js(password)
    );
    eval(admin, &script).map(|_| ())
}

pub fn drop(admin: Option<&DbAdmin>, name: &str, user: Option<&str>) -> Result<(), String> {
    let script = format!(
        "const d = db.getSiblingDB({n});\n\
         const u = {u};\n\
         if (u && d.getUser(u)) d.dropUser(u);\n\
         d.dropDatabase();\n\
         print('ok');",
        n = js(name),
        u = user.map(js).unwrap_or_else(|| "null".into())
    );
    eval(admin, &script).map(|_| ())
}

pub fn change_password(
    admin: Option<&DbAdmin>,
    name: &str,
    user: &str,
    password: &str,
) -> Result<(), String> {
    let script = format!(
        "db.getSiblingDB({n}).changeUserPassword({u}, {p}); print('ok');",
        n = js(name),
        u = js(user),
        p = js(password)
    );
    eval(admin, &script).map(|_| ())
}

/// Gives an existing database a user the panel knows. An existing user is
/// only taken over with its current password (checked by logging in) unless
/// `reset` allows setting a new one.
pub fn adopt(
    admin: Option<&DbAdmin>,
    name: &str,
    user: &str,
    password: &str,
    reset: bool,
) -> Result<(), String> {
    let script = format!(
        "const d = db.getSiblingDB({n});\n\
         const roles = [{{ role: 'readWrite', db: {n} }}, {{ role: 'dbAdmin', db: {n} }}];\n\
         if (d.getUser({u})) {{\n\
           if ({reset}) {{ d.updateUser({u}, {{ pwd: {p} }}); }}\n\
           else {{\n\
             try {{ connect({check}).runCommand({{ ping: 1 }}); }}\n\
             catch (e) {{ throw new Error('User ' + {u} + ' already exists and this is not its password. Enter its current password, or choose to set a new one.'); }}\n\
           }}\n\
           d.grantRolesToUser({u}, roles);\n\
         }} else {{ d.createUser({{ user: {u}, pwd: {p}, roles: roles }}); }}\n\
         print('ok');",
        n = js(name),
        u = js(user),
        p = js(password),
        reset = reset,
        check = js(&uri(Some((user, password, name))))
    );
    eval(admin, &script).map(|_| ())
}

#[derive(Debug, Deserialize)]
pub struct CollectionInfo {
    pub name: String,
    pub count: u64,
    pub size: u64,
}

pub fn collections(admin: Option<&DbAdmin>, name: &str) -> Result<Vec<CollectionInfo>, String> {
    let script = format!(
        "const d = db.getSiblingDB({n});\n\
         const out = d.getCollectionInfos({{ type: 'collection' }}).map(c => {{\n\
           let s = {{}};\n\
           try {{ s = d.getCollection(c.name).aggregate([{{ $collStats: {{ storageStats: {{}} }} }}]).toArray()[0].storageStats; }} catch (e) {{}}\n\
           return {{ name: c.name, count: Number(s.count || 0), size: Number((s.storageSize || 0) + (s.totalIndexSize || 0)) }};\n\
         }}).sort((a, b) => a.name.localeCompare(b.name));\n\
         print(JSON.stringify(out));",
        n = js(name)
    );
    eval_json(admin, &script)
}

/// The `root` user (role root in admin) for the operator's own tools.
pub fn set_root_password(admin: Option<&DbAdmin>, password: &str) -> Result<(), String> {
    let script = format!(
        "const a = db.getSiblingDB('admin');\n\
         if (a.getUser('root')) {{ a.updateUser('root', {{ pwd: {p} }}); }}\n\
         else {{ a.createUser({{ user: 'root', pwd: {p}, roles: [{{ role: 'root', db: 'admin' }}] }}); }}\n\
         print('ok');",
        p = js(password)
    );
    eval(admin, &script).map(|_| ())
}

#[derive(Debug, Deserialize, Default)]
pub struct ServerFacts {
    #[serde(default)]
    pub cache_bytes: u64,
}

pub fn server_facts(admin: Option<&DbAdmin>) -> ServerFacts {
    eval_json(
        admin,
        "const s = db.serverStatus();\n\
         print(JSON.stringify({ cache_bytes: Number(((s.wiredTiger || {}).cache || {})['maximum bytes configured'] || 0) }));",
    )
    .unwrap_or_default()
}

// ===== Dump / restore =====

/// The dump tools read the password from a private config file.
pub struct ToolAuth {
    path: PathBuf,
}

impl ToolAuth {
    pub fn new(admin: &DbAdmin) -> Result<Self, String> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let path = std::env::temp_dir().join(format!(
            "linxpanel-mongotool-{}.yaml",
            uuid::Uuid::new_v4().simple()
        ));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| e.to_string())?;
        // A YAML double-quoted scalar; JSON escaping is valid there.
        writeln!(file, "password: {}", js(&admin.password)).map_err(|e| e.to_string())?;
        Ok(Self { path })
    }

    pub fn command(&self, program: &str, admin: &DbAdmin) -> Command {
        let mut command = Command::new(program);
        command
            .arg(format!("--config={}", self.path.display()))
            .args(["--host", "127.0.0.1", "--port"])
            .arg(port().to_string())
            .args([
                "--username",
                &admin.user,
                "--authenticationDatabase",
                "admin",
            ]);
        command
    }
}

impl Drop for ToolAuth {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub fn dump_command(auth: &ToolAuth, admin: &DbAdmin, name: &str) -> Command {
    let mut command = auth.command("mongodump", admin);
    command
        .arg(format!("--db={}", name))
        .arg("--archive")
        .arg("--quiet");
    command
}

pub fn restore_command(auth: &ToolAuth, admin: &DbAdmin, name: &str) -> Command {
    let mut command = auth.command("mongorestore", admin);
    command
        .arg(format!("--nsInclude={}.*", name))
        .args(["--archive", "--drop", "--quiet"]);
    command
}

// ===== mongod.conf (YAML, as the packages write it: two-space blocks) =====

/// `yaml_get(text, &["net", "bindIp"])`.
pub fn yaml_get(text: &str, path: &[&str]) -> Option<String> {
    let mut depth = 0;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - trimmed.len();
        let level = indent / 2;
        if level < depth {
            depth = level;
        }
        if level != depth || depth >= path.len() {
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        if key.trim() != path[depth] {
            continue;
        }
        if depth + 1 == path.len() {
            let value = value.split(" #").next().unwrap_or("").trim();
            return Some(value.trim_matches(['"', '\'']).to_string());
        }
        depth += 1;
    }
    None
}

/// Sets (None: removes) a value, creating its sections where missing.
pub fn yaml_set(text: &str, path: &[&str], value: Option<&str>) -> String {
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    // Find the deepest existing prefix and where its block ends.
    let mut depth = 0;
    let mut anchor: Option<usize> = None; // line of the deepest found section
    let mut target: Option<usize> = None; // line of the key itself
    let mut index = 0;
    while index < lines.len() {
        let line = &lines[index];
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            index += 1;
            continue;
        }
        let level = (line.len() - trimmed.len()) / 2;
        if level < depth {
            break;
        }
        if level == depth {
            if let Some((key, _)) = trimmed.split_once(':') {
                if key.trim() == path[depth] {
                    if depth + 1 == path.len() {
                        target = Some(index);
                        break;
                    }
                    anchor = Some(index);
                    depth += 1;
                }
            }
        }
        index += 1;
    }
    match (target, value) {
        (Some(at), Some(value)) => {
            lines[at] = format!("{}{}: {}", "  ".repeat(depth), path[depth], value);
        }
        (Some(at), None) => {
            lines.remove(at);
        }
        (None, None) => {}
        (None, Some(value)) => {
            let mut block = Vec::new();
            for (level, key) in path.iter().enumerate().skip(depth) {
                if level + 1 == path.len() {
                    block.push(format!("{}{}: {}", "  ".repeat(level), key, value));
                } else {
                    block.push(format!("{}{}:", "  ".repeat(level), key));
                }
            }
            // Inside the found section: after its last line; otherwise at the end.
            let insert_at = match anchor {
                Some(start) => {
                    let parent_level = depth - 1;
                    let mut end = start + 1;
                    while end < lines.len() {
                        let t = lines[end].trim_start();
                        if !t.is_empty() && !t.starts_with('#') {
                            let level = (lines[end].len() - t.len()) / 2;
                            if level <= parent_level {
                                break;
                            }
                        }
                        end += 1;
                    }
                    // Keep trailing blank lines / comments after the block.
                    while end > start + 1 && lines[end - 1].trim().is_empty() {
                        end -= 1;
                    }
                    end
                }
                None => lines.len(),
            };
            for (offset, line) in block.into_iter().enumerate() {
                lines.insert(insert_at + offset, line);
            }
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

pub fn data_dir() -> String {
    yaml_get(&conf_text(), &["storage", "dbPath"]).unwrap_or_else(|| "/var/lib/mongodb".into())
}

pub fn log_path() -> Option<PathBuf> {
    yaml_get(&conf_text(), &["systemLog", "path"]).map(PathBuf::from)
}

/// Installs MongoDB through the bundled script (streamed to the task window).
pub fn install(version: &str) -> Result<String, String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let path = std::env::temp_dir().join(format!(
        "linxpanel-mongodb-{}.sh",
        uuid::Uuid::new_v4().simple()
    ));
    {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .open(&path)
            .map_err(|e| e.to_string())?;
        file.write_all(INSTALL_SCRIPT.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    let mut command = Command::new("bash");
    command.arg(&path).env("MONGODB_VERSION", version);
    let output = shell::run_command(command, None, Duration::from_secs(45 * 60));
    let _ = std::fs::remove_file(&path);
    let output = output?;
    let text = format!("{}{}", output.stdout, output.stderr);
    if output.ok() {
        Ok(text)
    } else {
        let reason = text
            .lines()
            .rev()
            .find_map(|l| l.trim().strip_prefix("[LinxPanel] "))
            .unwrap_or("MongoDB install failed")
            .to_string();
        Err(format!("{}\n{}", text, reason))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONF: &str = "# mongod.conf\n\n# Where and how to store data.\nstorage:\n  dbPath: /var/lib/mongodb\n#  engine:\n\n# where to write logging data.\nsystemLog:\n  destination: file\n  logAppend: true\n  path: /var/log/mongodb/mongod.log\n\n# network interfaces\nnet:\n  port: 27017\n  bindIp: 127.0.0.1\n\n#security:\n\n#operationProfiling:\n";

    #[test]
    fn reads_values() {
        assert_eq!(yaml_get(CONF, &["net", "port"]).as_deref(), Some("27017"));
        assert_eq!(
            yaml_get(CONF, &["net", "bindIp"]).as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(
            yaml_get(CONF, &["systemLog", "path"]).as_deref(),
            Some("/var/log/mongodb/mongod.log")
        );
        assert_eq!(yaml_get(CONF, &["security", "authorization"]), None);
        // A key of another section with the same name is not taken.
        assert_eq!(yaml_get(CONF, &["storage", "path"]), None);
    }

    #[test]
    fn edits_values_and_creates_sections() {
        let open = yaml_set(CONF, &["net", "bindIp"], Some("0.0.0.0"));
        assert_eq!(
            yaml_get(&open, &["net", "bindIp"]).as_deref(),
            Some("0.0.0.0")
        );
        assert!(open.contains("  bindIp: 0.0.0.0\n"));

        let auth = yaml_set(CONF, &["security", "authorization"], Some("enabled"));
        assert_eq!(
            yaml_get(&auth, &["security", "authorization"]).as_deref(),
            Some("enabled")
        );
        assert!(auth.ends_with("security:\n  authorization: enabled\n"));
        assert!(auth.contains("#security:\n"));

        let cache = yaml_set(
            CONF,
            &["storage", "wiredTiger", "engineConfig", "cacheSizeGB"],
            Some("1.5"),
        );
        assert_eq!(
            yaml_get(
                &cache,
                &["storage", "wiredTiger", "engineConfig", "cacheSizeGB"]
            )
            .as_deref(),
            Some("1.5")
        );
        // Inserted inside storage, before the next section.
        let storage_at = cache.find("storage:").unwrap();
        let cache_at = cache.find("      cacheSizeGB: 1.5").unwrap();
        let system_at = cache.find("systemLog:").unwrap();
        assert!(storage_at < cache_at && cache_at < system_at);
        assert_eq!(
            yaml_get(&cache, &["storage", "dbPath"]).as_deref(),
            Some("/var/lib/mongodb")
        );

        let removed = yaml_set(&open, &["net", "bindIp"], None);
        assert_eq!(yaml_get(&removed, &["net", "bindIp"]), None);
        assert_eq!(
            yaml_get(&removed, &["net", "port"]).as_deref(),
            Some("27017")
        );
    }

    #[test]
    fn credentials_are_encoded() {
        assert_eq!(percent_encode("a@b:c/d"), "a%40b%3Ac%2Fd");
        assert_eq!(js("it's \"x\"\n"), "\"it's \\\"x\\\"\\n\"");
    }
}
