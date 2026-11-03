//! Self-hosted service programs from the app store (File Browser, frps,
//! ddns-go, Gitea). Each is a single release binary from GitHub plus its
//! configuration and generated credentials, run as a managed app so the
//! panel supervises it, restarts it on a crash and starts it at boot.
//!
//! This module holds the catalog and the blocking file-level steps; the
//! orchestration (managed app, firewall, site, rollback) is in
//! `api::svcapps_api`.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::{appproc, appstore, fileops, shell, tasks};

pub const APPS_ROOT: &str = "/www/server/apps";
const STEP_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Serialize)]
pub struct PortDef {
    /// i18n key suffix: web | bind | ssh
    pub role: &'static str,
    pub default: u16,
    /// Always reachable from outside (clients connect to it directly).
    pub external: bool,
    /// 0 turns it off.
    pub optional: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServiceDef {
    pub key: &'static str,
    pub name: &'static str,
    pub homepage: &'static str,
    pub repo: &'static str,
    /// tools | network | dev
    pub category: &'static str,
    /// The first one is the web interface.
    pub ports: &'static [PortDef],
    /// System user it runs as by default ("" = root).
    pub default_user: &'static str,
    /// The user may pick another run user (File Browser serves site files).
    pub user_choice: bool,
    /// Asks for the directory it serves.
    pub root_dir: bool,
    #[serde(skip)]
    binary: &'static str,
}

pub const SERVICES: &[ServiceDef] = &[
    ServiceDef {
        key: "filebrowser",
        name: "File Browser",
        homepage: "https://filebrowser.org",
        repo: "filebrowser/filebrowser",
        category: "tools",
        ports: &[PortDef {
            role: "web",
            default: 8090,
            external: false,
            optional: false,
        }],
        default_user: "www-data",
        user_choice: true,
        root_dir: true,
        binary: "filebrowser",
    },
    ServiceDef {
        key: "frps",
        name: "frp Server",
        homepage: "https://gofrp.org",
        repo: "fatedier/frp",
        category: "network",
        ports: &[
            PortDef {
                role: "dashboard",
                default: 7500,
                external: false,
                optional: false,
            },
            PortDef {
                role: "bind",
                default: 7000,
                external: true,
                optional: false,
            },
        ],
        default_user: "frp",
        user_choice: false,
        root_dir: false,
        binary: "frps",
    },
    ServiceDef {
        key: "ddns-go",
        name: "DDNS-GO",
        homepage: "https://github.com/jeessy2/ddns-go",
        repo: "jeessy2/ddns-go",
        category: "network",
        ports: &[PortDef {
            role: "web",
            default: 9876,
            external: false,
            optional: false,
        }],
        default_user: "ddnsgo",
        user_choice: false,
        root_dir: false,
        binary: "ddns-go",
    },
    ServiceDef {
        key: "gitea",
        name: "Gitea",
        homepage: "https://about.gitea.com",
        repo: "go-gitea/gitea",
        category: "dev",
        ports: &[
            PortDef {
                role: "web",
                default: 3000,
                external: false,
                optional: false,
            },
            PortDef {
                role: "ssh",
                default: 2222,
                external: true,
                optional: true,
            },
        ],
        default_user: "git",
        user_choice: false,
        root_dir: false,
        binary: "gitea",
    },
];

pub fn find(key: &str) -> Option<&'static ServiceDef> {
    SERVICES.iter().find(|def| def.key == key)
}

impl ServiceDef {
    pub fn dir(&self) -> PathBuf {
        Path::new(APPS_ROOT).join(self.key)
    }

    pub fn binary_path(&self, dir: &Path) -> PathBuf {
        dir.join(self.binary)
    }

    /// The release asset for this machine, and whether it is a bare
    /// xz-compressed binary (else a .tar.gz holding the binary).
    pub fn asset(&self, version: &str, xz: bool) -> Result<(String, AssetKind), String> {
        let arch = std::env::consts::ARCH;
        let pick = |table: &[(&'static str, &'static str)]| -> Result<&'static str, String> {
            table
                .iter()
                .find(|(machine, _)| *machine == arch)
                .map(|(_, name)| *name)
                .ok_or_else(|| format!("{} has no release for this CPU ({})", self.name, arch))
        };
        Ok(match self.key {
            // goreleaser: {{.Os}}-{{.Arch}}{{if .Arm}}v{{.Arm}}{{end}}-filebrowser
            "filebrowser" => {
                let a = pick(&[
                    ("x86_64", "amd64"),
                    ("aarch64", "arm64"),
                    ("arm", "armv7"),
                    ("x86", "386"),
                    ("riscv64", "riscv64"),
                ])?;
                (format!("linux-{}-filebrowser.tar.gz", a), AssetKind::TarGz)
            }
            // package.sh: frp_{version}_linux_{arch}; GOARM=7 builds are "arm_hf".
            "frps" => {
                let a = pick(&[
                    ("x86_64", "amd64"),
                    ("aarch64", "arm64"),
                    ("arm", "arm_hf"),
                    ("riscv64", "riscv64"),
                    ("loongarch64", "loong64"),
                ])?;
                (
                    format!("frp_{}_linux_{}.tar.gz", version, a),
                    AssetKind::TarGz,
                )
            }
            // goreleaser: names follow uname (x86_64, i386, armv7).
            "ddns-go" => {
                let a = pick(&[
                    ("x86_64", "x86_64"),
                    ("aarch64", "arm64"),
                    ("arm", "armv7"),
                    ("x86", "i386"),
                    ("riscv64", "riscv64"),
                ])?;
                (
                    format!("ddns-go_{}_linux_{}.tar.gz", version, a),
                    AssetKind::TarGz,
                )
            }
            // xgo targets: gitea-{version}-linux-{amd64,386,arm-6,arm64,riscv64}[.xz]
            "gitea" => {
                let a = pick(&[
                    ("x86_64", "amd64"),
                    ("aarch64", "arm64"),
                    ("arm", "arm-6"),
                    ("x86", "386"),
                    ("riscv64", "riscv64"),
                ])?;
                if xz {
                    (format!("gitea-{}-linux-{}.xz", version, a), AssetKind::Xz)
                } else {
                    (format!("gitea-{}-linux-{}", version, a), AssetKind::Raw)
                }
            }
            _ => return Err("Unknown app".into()),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetKind {
    TarGz,
    Xz,
    Raw,
}

impl AssetKind {
    fn of_url(url: &str) -> Self {
        let path = url
            .split(['?', '#'])
            .next()
            .unwrap_or(url)
            .to_ascii_lowercase();
        if path.ends_with(".tar.gz") || path.ends_with(".tgz") {
            Self::TarGz
        } else if path.ends_with(".xz") {
            Self::Xz
        } else {
            Self::Raw
        }
    }
}

/// "1.2.3" / "v1.2.3" -> "v1.2.3". Only tag-like text, so it is safe in URLs.
pub fn normalize_tag(input: &str) -> Result<String, String> {
    let raw = input.trim();
    let body = raw.strip_prefix('v').unwrap_or(raw);
    let ok = !body.is_empty()
        && body.len() <= 40
        && body.starts_with(|c: char| c.is_ascii_digit())
        && body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    if ok {
        Ok(format!("v{}", body))
    } else {
        Err(format!("Invalid version: {}", raw))
    }
}

/// The tag GitHub redirects /releases/latest to. Works through github.com
/// acceleration proxies, which usually do not cover the API.
pub fn latest_tag(prefix: &str, repo: &str) -> Result<String, String> {
    let url = format!("{}https://github.com/{}/releases/latest", prefix, repo);
    let output = shell::run_with_timeout(
        "curl",
        &[
            "-sS",
            "-o",
            "/dev/null",
            "-w",
            "%{redirect_url}\n",
            "--max-time",
            "25",
            &url,
        ],
        Duration::from_secs(30),
    )?;
    let location = output.stdout.trim().trim_end_matches('/');
    let tag = location.rsplit('/').next().unwrap_or_default();
    if location.contains("/releases/tag/") {
        if let Ok(tag) = normalize_tag(tag) {
            return Ok(tag);
        }
    }
    Err(format!(
        "Could not look up the latest {} release ({}). Enter a version, or set a GitHub \
         proxy in Settings > Updates.",
        repo,
        if output.ok() {
            "no redirect".to_string()
        } else {
            output.error_text()
        }
        .trim()
    ))
}

pub fn download_url(prefix: &str, repo: &str, tag: &str, asset: &str) -> String {
    format!(
        "{}https://github.com/{}/releases/download/{}/{}",
        prefix, repo, tag, asset
    )
}

fn note(log: &mut String, line: &str) {
    tasks::note(line);
    log.push_str(line);
    log.push('\n');
}

/// Downloads the release (or `url`) and leaves the program at `target`,
/// checked to be a Linux executable - a proxy's HTML error page is not.
pub fn fetch_binary(
    def: &ServiceDef,
    url: &str,
    work: &Path,
    target: &Path,
    log: &mut String,
) -> Result<(), String> {
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let kind = AssetKind::of_url(url);
    let file_name = match kind {
        AssetKind::TarGz => "package.tar.gz",
        AssetKind::Xz => "package.xz",
        AssetKind::Raw => "package.bin",
    };
    let archive = work.join(file_name);
    note(log, &format!("[LinxPanel] Downloading {}", url));
    appstore::download(url, &archive, log)?;
    let size = std::fs::metadata(&archive).map(|m| m.len()).unwrap_or(0);
    note(log, &format!("[LinxPanel] Downloaded {} KB", size / 1024));

    let unpacked = match kind {
        AssetKind::TarGz => {
            let out = work.join("x");
            fileops::extract(&archive, &out, true)?;
            find_file(&out, def.binary)
                .ok_or_else(|| format!("{} not found in the package", def.binary))?
        }
        AssetKind::Xz => {
            let out = work.join(def.binary);
            let file = std::fs::File::create(&out).map_err(|e| e.to_string())?;
            let status = Command::new("xz")
                .args(["-dc", &archive.to_string_lossy()])
                .stdout(file)
                .status()
                .map_err(|e| format!("xz: {}", e))?;
            if !status.success() {
                return Err("xz could not decompress the download".into());
            }
            out
        }
        AssetKind::Raw => archive,
    };
    if !is_elf(&unpacked) {
        return Err(
            "The download is not a Linux program (a proxy error page?). Check the download \
             source or the GitHub proxy."
                .into(),
        );
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    // Copy, then rename into place: never a half-written binary at `target`.
    let staging = target.with_extension("download");
    std::fs::copy(&unpacked, &staging).map_err(|e| format!("{}: {}", staging.display(), e))?;
    set_mode(&staging, 0o755)?;
    std::fs::rename(&staging, target).map_err(|e| e.to_string())?;
    Ok(())
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in std::fs::read_dir(&path).ok()?.flatten() {
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path).ok()?;
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file() && entry.file_name() == name {
                return Some(path);
            }
        }
    }
    None
}

fn is_elf(path: &Path) -> bool {
    let mut magic = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut magic))
        .is_ok()
        && magic == *b"\x7fELF"
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("{}: {}", path.display(), e))
}

/// Creates a system user (no login) for a service, when it does not exist.
pub fn ensure_user(name: &str, home: &Path, log: &mut String) -> Result<(), String> {
    if name.is_empty() || name == "root" || appproc::run_user_ids(name).is_some() {
        return Ok(());
    }
    note(log, &format!("[LinxPanel] Creating system user {}", name));
    let home = home.to_string_lossy().to_string();
    let nologin = ["/usr/sbin/nologin", "/sbin/nologin"]
        .into_iter()
        .find(|p| Path::new(p).exists())
        .unwrap_or("/bin/false");
    let output = if shell::has_binary("useradd") {
        shell::run_privileged(
            "useradd",
            &[
                "--system",
                "--user-group",
                "--home-dir",
                &home,
                "--no-create-home",
                "--shell",
                nologin,
                name,
            ],
        )?
    } else {
        // BusyBox (Alpine).
        shell::run_privileged(
            "adduser",
            &["-S", "-D", "-H", "-h", &home, "-s", nologin, name],
        )?
    };
    if !output.ok() || appproc::run_user_ids(name).is_none() {
        return Err(format!(
            "Could not create user {}: {}",
            name,
            output.error_text()
        ));
    }
    Ok(())
}

/// A command run as `user` (inside `dir`, with HOME set), or as the panel's
/// own user when `user` is empty.
fn command_as(user: &str, home: &str, dir: &Path, args: &[String]) -> Command {
    let mut command = if user.is_empty() || user == "root" {
        let mut c = Command::new(&args[0]);
        c.args(&args[1..]);
        c
    } else {
        let mut c = Command::new("runuser");
        c.args(["-u", user, "--", "env", &format!("HOME={}", home)]);
        c.args(args);
        c
    };
    command.current_dir(dir);
    command
}

fn run_init(user: &str, home: &str, dir: &Path, step: &InitStep) -> Result<(), String> {
    let command = command_as(user, home, dir, &step.args);
    let output = if step.private {
        shell::run_command_private(command, STEP_TIMEOUT)?
    } else {
        shell::run_command(command, None, STEP_TIMEOUT)?
    };
    if output.ok() {
        Ok(())
    } else {
        Err(format!("{} failed: {}", step.label, output.error_text()))
    }
}

// ===== Configuration =====

#[derive(Debug, Clone, Deserialize)]
pub struct InstallOptions {
    /// Same order as the app's ports; 0 turns an optional port off.
    pub ports: Vec<u16>,
    /// The web interface listens on all addresses (else on 127.0.0.1 only,
    /// for a reverse-proxy domain).
    pub public: bool,
    pub domain: Option<String>,
    /// The host the browser used for the panel: used in URLs when there is no domain.
    pub access_host: String,
    pub run_user: String,
    pub root_dir: String,
    pub locale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InfoItem {
    /// i18n key suffix: url | user | password | token | config | ...
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub secret: bool,
}

fn info(key: &str, value: impl Into<String>, secret: bool) -> InfoItem {
    InfoItem {
        key: key.into(),
        value: value.into(),
        secret,
    }
}

#[derive(Debug, Clone)]
pub struct InitStep {
    pub label: &'static str,
    pub args: Vec<String>,
    /// Holds a secret as a plain argument: keep the command line out of the log.
    pub private: bool,
}

/// Everything an install writes and runs, decided up front (and testable).
#[derive(Debug, Clone)]
pub struct Setup {
    pub files: Vec<(PathBuf, String, u32)>,
    pub init: Vec<InitStep>,
    /// Arguments the managed app runs the binary with.
    pub args: String,
    pub info: Vec<InfoItem>,
    /// System packages the program needs.
    pub packages: Vec<&'static str>,
}

/// URL of the web interface as the user will open it.
pub fn web_url(port: u16, options: &InstallOptions) -> String {
    match options.domain.as_deref().filter(|d| !d.is_empty()) {
        Some(domain) => format!("http://{}/", domain),
        None => format!("http://{}:{}/", url_host(&options.access_host), port),
    }
}

fn url_host(host: &str) -> String {
    let host = host.trim();
    let host = if host.is_empty() { "127.0.0.1" } else { host };
    if host.contains(':') && !host.starts_with('[') {
        format!("[{}]", host)
    } else {
        host.to_string()
    }
}

/// Only characters that are safe inside the generated TOML/INI/YAML values.
fn plain_host(host: &str) -> String {
    host.chars()
        .filter(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(*c))
        .collect()
}

pub fn plan(def: &ServiceDef, dir: &Path, options: &InstallOptions) -> Result<Setup, String> {
    let port = |i: usize| options.ports.get(i).copied().unwrap_or(0);
    let listen = if options.public {
        "0.0.0.0"
    } else {
        "127.0.0.1"
    };
    let bin = def.binary_path(dir).to_string_lossy().to_string();
    let d = dir.to_string_lossy().to_string();
    let url = web_url(port(0), options);
    Ok(match def.key {
        "filebrowser" => {
            let db = format!("{}/filebrowser.db", d);
            let root = options.root_dir.trim();
            if !root.starts_with('/') {
                return Err("The directory to manage must be an absolute path".into());
            }
            let password = crate::db::generate_password(16);
            let s = |v: &str| v.to_string();
            Setup {
                files: Vec::new(),
                init: vec![
                    InitStep {
                        label: "filebrowser config init",
                        args: vec![
                            bin.clone(),
                            s("-d"),
                            db.clone(),
                            s("config"),
                            s("init"),
                            s("--address"),
                            s(listen),
                            s("--port"),
                            port(0).to_string(),
                            s("--root"),
                            s(root),
                            s("--log"),
                            s("stdout"),
                            s("--locale"),
                            s(if options.locale.starts_with("zh") {
                                "zh-cn"
                            } else {
                                "en"
                            }),
                        ],
                        private: false,
                    },
                    InitStep {
                        label: "filebrowser users add",
                        args: vec![
                            bin.clone(),
                            s("-d"),
                            db.clone(),
                            s("users"),
                            s("add"),
                            s("admin"),
                            password.clone(),
                            s("--perm.admin"),
                        ],
                        private: true,
                    },
                ],
                args: format!("-d {}", db),
                info: vec![
                    info("url", url, false),
                    info("user", "admin", false),
                    info("password", password, true),
                    info("rootDir", root, false),
                ],
                packages: Vec::new(),
            }
        }
        "frps" => {
            let token = crate::db::generate_password(24);
            let password = crate::db::generate_password(16);
            let config = format!(
                "# frps configuration, written by LinxPanel. Docs: https://gofrp.org\n\
                 bindAddr = \"0.0.0.0\"\n\
                 bindPort = {bind}\n\n\
                 auth.method = \"token\"\n\
                 auth.token = \"{token}\"\n\n\
                 # Dashboard\n\
                 webServer.addr = \"{listen}\"\n\
                 webServer.port = {web}\n\
                 webServer.user = \"admin\"\n\
                 webServer.password = \"{password}\"\n\n\
                 log.to = \"console\"\n\
                 log.level = \"info\"\n\n\
                 # HTTP(S) virtual hosts for frpc \"http\" proxies (ports must be free):\n\
                 # vhostHTTPPort = 8080\n\
                 # vhostHTTPSPort = 8443\n",
                bind = port(1),
                web = port(0),
            );
            let config_path = dir.join("frps.toml");
            let server = plain_host(
                options
                    .domain
                    .as_deref()
                    .filter(|d| !d.is_empty())
                    .unwrap_or(&options.access_host),
            );
            Setup {
                files: vec![(config_path.clone(), config, 0o600)],
                init: Vec::new(),
                args: format!("-c {}", config_path.display()),
                info: vec![
                    info("dashboard", url, false),
                    info("user", "admin", false),
                    info("password", password, true),
                    info("serverAddr", format!("{}:{}", server, port(1)), false),
                    info("token", token, true),
                    info("config", config_path.to_string_lossy(), false),
                ],
                packages: Vec::new(),
            }
        }
        "ddns-go" => {
            let password = crate::db::generate_password(16);
            let config_path = dir.join("config.yaml");
            // ddns-go hashes a plain password on start and saves the file.
            let config = format!(
                "user:\n    username: admin\n    password: \"{password}\"\n\
                 notallowwanaccess: {local_only}\nlang: {lang}\n",
                local_only = !options.public && options.domain.as_deref().unwrap_or("").is_empty(),
                lang = if options.locale.starts_with("zh") {
                    "zh"
                } else {
                    "en"
                },
            );
            Setup {
                files: vec![(config_path.clone(), config, 0o600)],
                init: Vec::new(),
                args: format!(
                    "-l {}:{} -f 300 -c {}",
                    listen,
                    port(0),
                    config_path.display()
                ),
                info: vec![
                    info("url", url, false),
                    info("user", "admin", false),
                    info("password", password, true),
                    info("config", config_path.to_string_lossy(), false),
                ],
                packages: Vec::new(),
            }
        }
        "gitea" => {
            let ini = dir.join("custom/conf/app.ini");
            let ini_s = ini.to_string_lossy().to_string();
            let user = run_user(def, options);
            let host = plain_host(
                options
                    .domain
                    .as_deref()
                    .filter(|d| !d.is_empty())
                    .unwrap_or(&options.access_host),
            );
            let host = if host.is_empty() {
                "localhost".to_string()
            } else {
                host
            };
            let ssh = if port(1) > 0 {
                format!(
                    "DISABLE_SSH = false\nSTART_SSH_SERVER = true\nSSH_DOMAIN = {host}\n\
                     SSH_PORT = {p}\nSSH_LISTEN_PORT = {p}\nBUILTIN_SSH_SERVER_USER = {user}\n",
                    p = port(1)
                )
            } else {
                "DISABLE_SSH = true\n".to_string()
            };
            let config = format!(
                "; Gitea configuration, written by LinxPanel.\n\
                 ; Reference: https://docs.gitea.com/administration/config-cheat-sheet\n\
                 APP_NAME = Gitea\nRUN_USER = {user}\nRUN_MODE = prod\nWORK_PATH = {d}\n\n\
                 [server]\nPROTOCOL = http\nHTTP_ADDR = {listen}\nHTTP_PORT = {web}\n\
                 DOMAIN = {host}\nROOT_URL = {url}\nAPP_DATA_PATH = {d}/data\n\
                 LFS_START_SERVER = true\nOFFLINE_MODE = true\n{ssh}\n\
                 [database]\nDB_TYPE = sqlite3\nPATH = {d}/data/gitea.db\n\n\
                 [repository]\nROOT = {d}/data/repositories\n\n\
                 [lfs]\nPATH = {d}/data/lfs\n\n\
                 [security]\nINSTALL_LOCK = true\nSECRET_KEY = {secret}\n\n\
                 [service]\nDISABLE_REGISTRATION = true\n\n\
                 [session]\nPROVIDER = file\n\n\
                 [log]\nMODE = console\nLEVEL = info\nROOT_PATH = {d}/log\n",
                web = port(0),
                secret = crate::db::generate_password(64),
            );
            let password = crate::db::generate_password(16);
            let base = |extra: &[&str]| {
                let mut args = vec![
                    bin.clone(),
                    "-w".into(),
                    d.clone(),
                    "-c".into(),
                    ini_s.clone(),
                ];
                args.extend(extra.iter().map(|s| s.to_string()));
                args
            };
            let mut create = base(&[
                "admin",
                "user",
                "create",
                "--username",
                "gitadmin",
                "--email",
                "gitadmin@example.com",
                "--admin",
                "--must-change-password=false",
            ]);
            // "--password=..." is masked in the task log.
            create.push(format!("--password={}", password));
            let mut items = vec![
                info("url", url, false),
                info("user", "gitadmin", false),
                info("password", password, true),
            ];
            if port(1) > 0 {
                items.push(info(
                    "sshClone",
                    format!("ssh://{}@{}:{}/<owner>/<repo>.git", user, host, port(1)),
                    false,
                ));
            }
            items.push(info("config", ini_s.clone(), false));
            Setup {
                files: vec![(ini, config, 0o640)],
                init: vec![
                    InitStep {
                        label: "gitea migrate",
                        args: base(&["migrate"]),
                        private: false,
                    },
                    InitStep {
                        label: "gitea admin user create",
                        args: create,
                        private: false,
                    },
                ],
                args: format!("-w {} -c {} web", d, ini_s),
                info: items,
                packages: vec!["git"],
            }
        }
        _ => return Err("Unknown app".into()),
    })
}

/// The system user the service runs as.
pub fn run_user(def: &ServiceDef, options: &InstallOptions) -> String {
    let chosen = options.run_user.trim();
    if def.user_choice && !chosen.is_empty() {
        if chosen == "root" {
            String::new()
        } else {
            chosen.to_string()
        }
    } else {
        def.default_user.to_string()
    }
}

/// Lays out the install directory: binary, configuration, data owned by the
/// run user, then the program's own init commands. Blocking.
pub fn prepare(
    def: &ServiceDef,
    dir: &Path,
    url: &str,
    work: &Path,
    setup: &Setup,
    user: &str,
    log: &mut String,
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {}", dir.display(), e))?;
    fetch_binary(def, url, work, &def.binary_path(dir), log)?;
    for package in &setup.packages {
        if !shell::has_binary(package) {
            note(log, &format!("[LinxPanel] Installing {}", package));
            super::runtime::install_packages(&[package], log);
            if !shell::has_binary(package) {
                return Err(format!(
                    "{} is required but could not be installed",
                    package
                ));
            }
        }
    }
    // A dedicated user's home is the install directory; an existing user keeps its own.
    ensure_user(user, dir, log)?;
    let home = appproc::run_user_ids(user)
        .map(|(_, _, home)| home)
        .unwrap_or_else(|| dir.to_string_lossy().to_string());
    for (path, content, mode) in &setup.files {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, content).map_err(|e| format!("{}: {}", path.display(), e))?;
        set_mode(path, *mode)?;
    }
    if !user.is_empty() {
        appproc::chown_to(user, dir);
    }
    for step in &setup.init {
        note(log, &format!("[LinxPanel] {}", step.label));
        run_init(user, &home, dir, step)?;
    }
    // Init commands may have created files (databases) as root.
    if !user.is_empty() {
        appproc::chown_to(user, dir);
    }
    Ok(())
}

/// Whether nothing listens on `port` yet (all addresses).
pub fn port_is_free(port: u16) -> bool {
    std::net::TcpListener::bind(("0.0.0.0", port)).is_ok()
}

/// Whether something accepts connections on 127.0.0.1:`port` right now.
pub async fn port_open(port: u16) -> bool {
    let connect = tokio::net::TcpStream::connect(("127.0.0.1", port));
    matches!(
        tokio::time::timeout(Duration::from_millis(400), connect).await,
        Ok(Ok(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> InstallOptions {
        InstallOptions {
            ports: vec![3000, 2222],
            public: true,
            domain: None,
            access_host: "203.0.113.5".into(),
            run_user: String::new(),
            root_dir: "/www/wwwroot".into(),
            locale: "zh-CN".into(),
        }
    }

    #[test]
    fn tags_are_normalized_and_checked() {
        assert_eq!(normalize_tag("1.27.3").unwrap(), "v1.27.3");
        assert_eq!(normalize_tag(" v0.71.1 ").unwrap(), "v0.71.1");
        assert_eq!(normalize_tag("v2.0.0-rc.1").unwrap(), "v2.0.0-rc.1");
        for bad in ["", "v", "latest", "1.0/../x", "1.0 x", "-1"] {
            assert!(normalize_tag(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn asset_names_follow_each_projects_release_layout() {
        if std::env::consts::ARCH != "x86_64" {
            return;
        }
        let name = |key: &str, v: &str, xz: bool| find(key).unwrap().asset(v, xz).unwrap().0;
        assert_eq!(
            name("filebrowser", "2.63.23", true),
            "linux-amd64-filebrowser.tar.gz"
        );
        assert_eq!(
            name("frps", "0.71.1", true),
            "frp_0.71.1_linux_amd64.tar.gz"
        );
        assert_eq!(
            name("ddns-go", "6.17.7", true),
            "ddns-go_6.17.7_linux_x86_64.tar.gz"
        );
        assert_eq!(name("gitea", "1.27.3", true), "gitea-1.27.3-linux-amd64.xz");
        assert_eq!(name("gitea", "1.27.3", false), "gitea-1.27.3-linux-amd64");
        assert_eq!(
            download_url("https://ghfast.top/", "fatedier/frp", "v0.71.1", "frp_0.71.1_linux_amd64.tar.gz"),
            "https://ghfast.top/https://github.com/fatedier/frp/releases/download/v0.71.1/frp_0.71.1_linux_amd64.tar.gz"
        );
    }

    #[test]
    fn asset_kind_from_url() {
        assert_eq!(
            AssetKind::of_url("https://x/a.tar.gz?x=1"),
            AssetKind::TarGz
        );
        assert_eq!(
            AssetKind::of_url("https://x/gitea-1-linux-amd64.xz"),
            AssetKind::Xz
        );
        assert_eq!(
            AssetKind::of_url("https://x/gitea-1-linux-amd64"),
            AssetKind::Raw
        );
    }

    #[test]
    fn gitea_config_is_complete() {
        let def = find("gitea").unwrap();
        let setup = plan(def, Path::new("/www/server/apps/gitea"), &options()).unwrap();
        let ini = &setup.files[0].1;
        for line in [
            "RUN_USER = git",
            "HTTP_ADDR = 0.0.0.0",
            "HTTP_PORT = 3000",
            "ROOT_URL = http://203.0.113.5:3000/",
            "INSTALL_LOCK = true",
            "DB_TYPE = sqlite3",
            "SSH_PORT = 2222",
            "START_SSH_SERVER = true",
            "DISABLE_REGISTRATION = true",
        ] {
            assert!(ini.contains(line), "missing {line}");
        }
        assert!(ini.contains("SECRET_KEY = ") && !ini.contains("SECRET_KEY = \n"));
        assert_eq!(
            setup.args,
            "-w /www/server/apps/gitea -c /www/server/apps/gitea/custom/conf/app.ini web"
        );
        let create = &setup.init[1].args;
        assert!(create.iter().any(|a| a.starts_with("--password=")));
        assert_eq!(setup.packages, vec!["git"]);

        // Domain, no SSH, local only.
        let mut o = options();
        o.domain = Some("git.example.com".into());
        o.public = false;
        o.ports = vec![3000, 0];
        let ini = plan(def, Path::new("/x"), &o).unwrap().files[0].1.clone();
        assert!(ini.contains("ROOT_URL = http://git.example.com/"));
        assert!(ini.contains("HTTP_ADDR = 127.0.0.1"));
        assert!(ini.contains("DISABLE_SSH = true"));
    }

    #[test]
    fn frps_and_ddns_configs() {
        let mut o = options();
        o.ports = vec![7500, 7000];
        o.public = false;
        let setup = plan(
            find("frps").unwrap(),
            Path::new("/www/server/apps/frps"),
            &o,
        )
        .unwrap();
        let toml = &setup.files[0].1;
        assert!(toml.contains("bindPort = 7000"));
        assert!(toml.contains("webServer.port = 7500"));
        assert!(toml.contains("webServer.addr = \"127.0.0.1\""));
        let token = setup.info.iter().find(|i| i.key == "token").unwrap();
        assert!(toml.contains(&format!("auth.token = \"{}\"", token.value)));
        assert_eq!(setup.args, "-c /www/server/apps/frps/frps.toml");
        assert_eq!(setup.files[0].2, 0o600);

        o.ports = vec![9876];
        let setup = plan(find("ddns-go").unwrap(), Path::new("/d"), &o).unwrap();
        let yaml = &setup.files[0].1;
        assert!(yaml.starts_with("user:\n    username: admin\n    password: \""));
        assert!(yaml.contains("notallowwanaccess: true"));
        assert!(yaml.contains("lang: zh"));
        assert_eq!(setup.args, "-l 127.0.0.1:9876 -f 300 -c /d/config.yaml");
    }

    #[test]
    fn filebrowser_plan_keeps_password_out_of_the_log() {
        let mut o = options();
        o.ports = vec![8090];
        o.run_user = "www-data".into();
        let def = find("filebrowser").unwrap();
        let setup = plan(def, Path::new("/fb"), &o).unwrap();
        assert!(setup.init[1].private);
        assert!(setup.init[0]
            .args
            .windows(2)
            .any(|w| w[0] == "--root" && w[1] == "/www/wwwroot"));
        assert_eq!(setup.args, "-d /fb/filebrowser.db");
        assert_eq!(run_user(def, &o), "www-data");
        o.root_dir = "relative".into();
        assert!(plan(def, Path::new("/fb"), &o).is_err());
    }

    #[test]
    fn hosts_in_urls() {
        let mut o = options();
        assert_eq!(web_url(3000, &o), "http://203.0.113.5:3000/");
        o.access_host = "2001:db8::1".into();
        assert_eq!(web_url(3000, &o), "http://[2001:db8::1]:3000/");
        o.domain = Some("x.example.com".into());
        assert_eq!(web_url(3000, &o), "http://x.example.com/");
        assert_eq!(plain_host("a\"b\nc.example.com"), "abc.example.com");
    }

    #[test]
    fn elf_check_rejects_html() {
        let dir = std::env::temp_dir().join(format!("lp-svc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a"), "<html>rate limited</html>").unwrap();
        std::fs::write(dir.join("b"), b"\x7fELF\x02\x01").unwrap();
        assert!(!is_elf(&dir.join("a")));
        assert!(is_elf(&dir.join("b")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
