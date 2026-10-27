//! Language runtime detection and installation.
//!
//! The panel hosts PHP / Node.js / Python / Java / Go / plain-binary applications,
//! so it needs to know which runtimes exist on the host, what version they are,
//! and how to install the missing ones through the distro package manager.

use serde::{Deserialize, Serialize};

use super::shell;

/// Static description of a supported runtime and how to find or install it.
struct RuntimeSpec {
    kind: &'static str,
    display_name: &'static str,
    /// Probed in order; the first one found wins.
    binaries: &'static [&'static str],
    version_args: &'static [&'static str],
    /// Package names per manager. Empty means "not installable this way".
    apt: &'static [&'static str],
    dnf: &'static [&'static str],
    apk: &'static [&'static str],
    brew: &'static [&'static str],
    /// Extra tooling reported alongside the runtime (package manager, app server...).
    components: &'static [ComponentSpec],
}

struct ComponentSpec {
    name: &'static str,
    binary: &'static str,
    version_args: &'static [&'static str],
}

const RUNTIMES: &[RuntimeSpec] = &[
    RuntimeSpec {
        kind: "php",
        display_name: "PHP",
        binaries: &["php"],
        version_args: &["--version"],
        apt: &["php-fpm", "php-cli"],
        dnf: &["php-fpm", "php-cli"],
        apk: &["php", "php-fpm"],
        brew: &["php"],
        components: &[
            ComponentSpec {
                name: "php-fpm",
                binary: "php-fpm",
                version_args: &["--version"],
            },
            ComponentSpec {
                name: "composer",
                binary: "composer",
                version_args: &["--version"],
            },
        ],
    },
    RuntimeSpec {
        kind: "node",
        display_name: "Node.js",
        binaries: &["node"],
        version_args: &["--version"],
        apt: &["nodejs", "npm"],
        dnf: &["nodejs", "npm"],
        apk: &["nodejs", "npm"],
        brew: &["node"],
        components: &[
            ComponentSpec {
                name: "npm",
                binary: "npm",
                version_args: &["--version"],
            },
            ComponentSpec {
                name: "pnpm",
                binary: "pnpm",
                version_args: &["--version"],
            },
            ComponentSpec {
                name: "yarn",
                binary: "yarn",
                version_args: &["--version"],
            },
        ],
    },
    RuntimeSpec {
        kind: "python",
        display_name: "Python",
        binaries: &["python3", "python"],
        version_args: &["--version"],
        apt: &["python3", "python3-pip", "python3-venv"],
        dnf: &["python3", "python3-pip"],
        apk: &["python3", "py3-pip"],
        brew: &["python"],
        components: &[
            ComponentSpec {
                name: "pip",
                binary: "pip3",
                version_args: &["--version"],
            },
            ComponentSpec {
                name: "uv",
                binary: "uv",
                version_args: &["--version"],
            },
        ],
    },
    RuntimeSpec {
        kind: "java",
        display_name: "Java (JDK)",
        binaries: &["java"],
        // `java -version` writes to stderr on older JDKs; the reader handles both.
        version_args: &["-version"],
        apt: &["default-jdk"],
        dnf: &["java-latest-openjdk-devel"],
        apk: &["openjdk17"],
        brew: &["openjdk"],
        components: &[
            ComponentSpec {
                name: "javac",
                binary: "javac",
                version_args: &["-version"],
            },
            ComponentSpec {
                name: "maven",
                binary: "mvn",
                version_args: &["--version"],
            },
            ComponentSpec {
                name: "gradle",
                binary: "gradle",
                version_args: &["--version"],
            },
        ],
    },
    RuntimeSpec {
        kind: "go",
        display_name: "Go",
        binaries: &["go"],
        version_args: &["version"],
        apt: &["golang-go"],
        dnf: &["golang"],
        apk: &["go"],
        brew: &["go"],
        components: &[],
    },
];

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeComponent {
    pub name: String,
    pub installed: bool,
    pub version: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeStatus {
    pub kind: String,
    pub display_name: String,
    pub installed: bool,
    pub version: String,
    pub path: String,
    pub components: Vec<RuntimeComponent>,
    /// Package manager that would be used to install it (apt/dnf/yum/apk/brew/none).
    pub package_manager: String,
    pub installable: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInstallResult {
    pub changed: bool,
    pub status: RuntimeStatus,
    pub log: String,
}

#[derive(Debug, Deserialize)]
pub struct RuntimeInstallRequest {
    pub kind: String,
}

/// Returns the detected package manager, preferring the distro's native one.
pub fn package_manager() -> &'static str {
    for candidate in ["apt-get", "dnf", "yum", "apk"] {
        if shell::has_binary(candidate) {
            return candidate;
        }
    }
    if shell::has_binary("brew") {
        return "brew";
    }
    "none"
}

fn spec_for(kind: &str) -> Option<&'static RuntimeSpec> {
    RUNTIMES.iter().find(|spec| spec.kind == kind)
}

/// Reads a version string from a binary. Some tools print to stderr, so both are read.
fn probe_version(binary: &str, args: &[&str]) -> Option<(String, String)> {
    if !shell::has_binary(binary) {
        return None;
    }
    let output = shell::run(binary, args).ok()?;
    let text = if output.stdout.trim().is_empty() {
        output.stderr
    } else {
        output.stdout
    };
    let first_line = text.lines().next().unwrap_or("").trim().to_string();
    let path = shell::run(if cfg!(windows) { "where" } else { "which" }, &[binary])
        .ok()
        .and_then(|out| out.stdout.lines().next().map(|s| s.trim().to_string()))
        .unwrap_or_default();
    Some((extract_version(&first_line), path))
}

/// Pulls `8.3.2` out of lines like `PHP 8.3.2 (cli) (built: ...)` or `go version go1.22.0 linux/amd64`.
fn extract_version(line: &str) -> String {
    let mut current = String::new();
    let mut best = String::new();
    for ch in line.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            current.push(ch);
        } else {
            if current.contains('.') && current.len() > best.len() {
                best = current.clone();
            }
            current.clear();
        }
    }
    if current.contains('.') && current.len() > best.len() {
        best = current;
    }
    if best.is_empty() {
        line.to_string()
    } else {
        best.trim_matches('.').to_string()
    }
}

fn detect_one(spec: &RuntimeSpec) -> RuntimeStatus {
    let manager = package_manager();
    let mut installed = false;
    let mut version = String::new();
    let mut path = String::new();

    for binary in spec.binaries {
        if let Some((found_version, found_path)) = probe_version(binary, spec.version_args) {
            installed = true;
            version = found_version;
            path = found_path;
            break;
        }
    }

    let components = spec
        .components
        .iter()
        .map(
            |component| match probe_version(component.binary, component.version_args) {
                Some((version, path)) => RuntimeComponent {
                    name: component.name.to_string(),
                    installed: true,
                    version,
                    path,
                },
                None => RuntimeComponent {
                    name: component.name.to_string(),
                    installed: false,
                    version: String::new(),
                    path: String::new(),
                },
            },
        )
        .collect();

    let packages = packages_for(spec, manager);
    let installable = !installed && !packages.is_empty() && manager != "none";
    let message = if installed {
        String::new()
    } else if manager == "none" {
        "No supported package manager found (apt/dnf/yum/apk/brew)".into()
    } else if packages.is_empty() {
        format!("No install recipe for {} on this system", spec.display_name)
    } else {
        String::new()
    };

    RuntimeStatus {
        kind: spec.kind.to_string(),
        display_name: spec.display_name.to_string(),
        installed,
        version,
        path,
        components,
        package_manager: manager.to_string(),
        installable,
        message,
    }
}

fn packages_for(spec: &RuntimeSpec, manager: &str) -> Vec<&'static str> {
    match manager {
        "apt-get" => spec.apt.to_vec(),
        "dnf" | "yum" => spec.dnf.to_vec(),
        "apk" => spec.apk.to_vec(),
        "brew" => spec.brew.to_vec(),
        _ => Vec::new(),
    }
}

pub fn list() -> Vec<RuntimeStatus> {
    RUNTIMES.iter().map(detect_one).collect()
}

/// Extensions common PHP apps (WordPress, Laravel, Typecho...) need. Installed one
/// at a time after PHP itself, so a name missing from one distro's repos does not
/// fail the whole install.
fn php_extensions(manager: &str) -> &'static [&'static str] {
    match manager {
        "apt-get" => &[
            "php-mysql",
            "php-pgsql",
            "php-sqlite3",
            "php-curl",
            "php-gd",
            "php-mbstring",
            "php-xml",
            "php-zip",
            "php-intl",
            "php-bcmath",
            "php-opcache",
            "php-redis",
        ],
        "dnf" | "yum" => &[
            "php-mysqlnd",
            "php-pgsql",
            "php-pdo",
            "php-gd",
            "php-mbstring",
            "php-xml",
            "php-intl",
            "php-bcmath",
            "php-opcache",
            "php-pecl-zip",
            "php-pecl-redis5",
        ],
        "apk" => &[
            "php-mysqli",
            "php-pdo_mysql",
            "php-pdo_pgsql",
            "php-pdo_sqlite",
            "php-curl",
            "php-gd",
            "php-mbstring",
            "php-xml",
            "php-dom",
            "php-zip",
            "php-intl",
            "php-bcmath",
            "php-opcache",
            "php-session",
            "php-tokenizer",
            "php-fileinfo",
        ],
        _ => &[],
    }
}

/// After PHP is present: add extensions, open the FPM socket to nginx and start it.
fn finish_php_install(manager: &str, log: &mut String) {
    for package in php_extensions(manager) {
        let args: Vec<String> = match manager {
            "apt-get" => vec!["install".into(), "-y".into(), package.to_string()],
            "dnf" | "yum" => vec!["install".into(), "-y".into(), package.to_string()],
            "apk" => vec!["add".into(), package.to_string()],
            _ => continue,
        };
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let ok = run_package_manager(manager, &arg_refs, std::time::Duration::from_secs(300))
            .map(|o| o.ok())
            .unwrap_or(false);
        log.push_str(&format!(
            "[LinxPanel] extension {}: {}\n",
            package,
            if ok { "ok" } else { "not available, skipped" }
        ));
    }
    match super::php::prepare() {
        Ok((pool, text)) => {
            log.push_str(&text);
            log.push_str(&format!(
                "[LinxPanel] PHP sites will use fastcgi_pass {}\n",
                pool.fastcgi_addr()
            ));
        }
        Err(err) => log.push_str(&format!("[LinxPanel] PHP-FPM setup failed: {}\n", err)),
    }
}

pub fn install(kind: &str) -> Result<RuntimeInstallResult, String> {
    let spec = spec_for(kind).ok_or_else(|| format!("Unknown runtime: {}", kind))?;
    let before = detect_one(spec);
    // Installing PHP again repairs it: missing extensions, FPM socket, FPM not running.
    if before.installed && kind == "php" {
        let mut log =
            String::from("[LinxPanel] PHP is installed; checking extensions and PHP-FPM\n");
        finish_php_install(package_manager(), &mut log);
        return Ok(RuntimeInstallResult {
            changed: false,
            status: detect_one(spec),
            log,
        });
    }
    if before.installed {
        return Ok(RuntimeInstallResult {
            changed: false,
            log: format!(
                "{} is already installed ({})",
                spec.display_name, before.version
            ),
            status: before,
        });
    }

    let manager = package_manager();
    let packages = packages_for(spec, manager);
    if packages.is_empty() || manager == "none" {
        return Err(if manager == "none" {
            "No supported package manager found (apt/dnf/yum/apk/brew)".to_string()
        } else {
            format!("No install recipe for {} on this system", spec.display_name)
        });
    }

    let mut log = String::new();
    log.push_str(&format!(
        "[LinxPanel] Installing {} via {}\n",
        spec.display_name, manager
    ));

    // brew refuses to run as root and manages its own privileges.
    let elevate = manager != "brew";

    for (program, args) in install_commands(manager, &packages) {
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        log.push_str(&format!("\n$ {} {}\n", program, arg_refs.join(" ")));

        let output = if elevate {
            shell::run_privileged_with_timeout(
                program,
                &arg_refs,
                std::time::Duration::from_secs(900),
            )
        } else {
            shell::run_with_timeout(program, &arg_refs, std::time::Duration::from_secs(900))
        };

        match output {
            Ok(result) => {
                log.push_str(result.stdout.trim_end());
                if !result.stderr.trim().is_empty() {
                    log.push('\n');
                    log.push_str(result.stderr.trim_end());
                }
                log.push('\n');
                if !result.ok() {
                    let status = detect_one(spec);
                    log.push_str(&format!(
                        "\n[LinxPanel] Command failed with status {}\n",
                        result.status
                    ));
                    return Ok(RuntimeInstallResult {
                        changed: status.installed,
                        status,
                        log,
                    });
                }
            }
            Err(err) => {
                log.push_str(&format!("{}\n", err));
                let status = detect_one(spec);
                return Ok(RuntimeInstallResult {
                    changed: status.installed,
                    status,
                    log,
                });
            }
        }
    }

    if kind == "php" && detect_one(spec).installed {
        finish_php_install(manager, &mut log);
    }

    let after = detect_one(spec);
    log.push_str(&format!(
        "\n[LinxPanel] Done. {} is {}\n",
        spec.display_name,
        if after.installed {
            format!("installed ({})", after.version)
        } else {
            "still not detected".to_string()
        }
    ));

    Ok(RuntimeInstallResult {
        changed: after.installed && !before.installed,
        status: after,
        log,
    })
}

/// Installs distro packages with the detected package manager, appending the
/// commands and their output to `log`. Returns whether every command succeeded.
/// Runs a package-manager command unattended: apt gets no debconf, needrestart
/// or conffile prompts (nothing could answer them from the panel), and a lock
/// held by another package operation - automatic updates hold it for minutes
/// after boot - is waited for instead of failing the install at once.
pub fn run_package_manager(
    manager: &str,
    args: &[&str],
    timeout: std::time::Duration,
) -> Result<shell::CmdOutput, String> {
    const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(600);
    let mut full: Vec<&str> = Vec::new();
    let program = if manager == "apt-get" {
        // Through env, because sudo drops the caller's environment.
        full.extend([
            "DEBIAN_FRONTEND=noninteractive",
            "NEEDRESTART_MODE=a",
            "NEEDRESTART_SUSPEND=1",
            "apt-get",
            "-o",
            "DPkg::Lock::Timeout=600",
            "-o",
            "Dpkg::Options::=--force-confdef",
            "-o",
            "Dpkg::Options::=--force-confold",
        ]);
        "env"
    } else {
        manager
    };
    full.extend_from_slice(args);
    // Older apt (Ubuntu 18.04, Debian 10) ignores Lock::Timeout: retry here.
    let deadline = std::time::Instant::now() + LOCK_WAIT;
    loop {
        let output = shell::run_privileged_with_timeout(program, &full, timeout)?;
        let text = format!("{}{}", output.stdout, output.stderr);
        let locked = !output.ok()
            && (text.contains("Could not get lock")
                || text.contains("Unable to acquire the dpkg frontend lock")
                || text.contains("Unable to lock directory"));
        if !locked || std::time::Instant::now() >= deadline {
            return Ok(output);
        }
        std::thread::sleep(std::time::Duration::from_secs(10));
    }
}

pub fn install_packages(packages: &[&str], log: &mut String) -> bool {
    let manager = package_manager();
    let commands = install_commands(manager, packages);
    if commands.is_empty() {
        log.push_str("[LinxPanel] No supported package manager found (apt/dnf/yum/apk)\n");
        return false;
    }
    for (program, args) in commands {
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        log.push_str(&format!("\n$ {} {}\n", program, arg_refs.join(" ")));
        match run_package_manager(program, &arg_refs, std::time::Duration::from_secs(900)) {
            Ok(result) => {
                log.push_str(result.stdout.trim_end());
                if !result.stderr.trim().is_empty() {
                    log.push('\n');
                    log.push_str(result.stderr.trim_end());
                }
                log.push('\n');
                        if !result.ok() {
                    super::tasks::note(&format!("[LinxPanel] {} failed", program));
                    return false;
                }
            }
            Err(err) => {
                log.push_str(&format!("{}\n", err));
                return false;
            }
        }
    }
    true
}

/// Package-manager specific command sequence (refresh index first where it matters).
fn install_commands(manager: &str, packages: &[&str]) -> Vec<(&'static str, Vec<String>)> {
    let owned: Vec<String> = packages.iter().map(|p| p.to_string()).collect();
    match manager {
        "apt-get" => vec![
            ("apt-get", vec!["update".to_string(), "-y".to_string()]),
            (
                "apt-get",
                [vec!["install".to_string(), "-y".to_string()], owned].concat(),
            ),
        ],
        "dnf" => vec![(
            "dnf",
            [vec!["install".to_string(), "-y".to_string()], owned].concat(),
        )],
        "yum" => vec![(
            "yum",
            [vec!["install".to_string(), "-y".to_string()], owned].concat(),
        )],
        "apk" => vec![
            ("apk", vec!["update".to_string()]),
            ("apk", [vec!["add".to_string()], owned].concat()),
        ],
        "brew" => vec![("brew", [vec!["install".to_string()], owned].concat())],
        _ => Vec::new(),
    }
}

/// Resolves the interpreter a managed app should be launched with.
/// Returns the absolute path when the runtime is present.
pub fn interpreter_path(kind: &str) -> Option<String> {
    let spec = spec_for(kind)?;
    for binary in spec.binaries {
        if let Some((_, path)) = probe_version(binary, spec.version_args) {
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod package_tests {
    /// Manual: installs a tiny package while another process holds the dpkg
    /// lock (`cargo test waits_for_dpkg_lock -- --ignored`, as root on apt).
    #[test]
    #[ignore]
    fn waits_for_dpkg_lock() {
        let mut log = String::new();
        assert!(super::install_packages(&["sl"], &mut log), "{log}");
    }
}
