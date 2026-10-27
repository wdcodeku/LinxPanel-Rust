//! Thin wrapper around external command execution.
//!
//! Every OS-integration module (systemd, cron, firewall, sockets) shells out, so the
//! timeout / privilege / error-shaping logic lives here once.

use std::process::{Command, Stdio};
use std::time::Duration;

pub struct CmdOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CmdOutput {
    pub fn ok(&self) -> bool {
        self.status == 0
    }

    /// stderr when present, otherwise stdout — what a user should be shown on failure.
    pub fn error_text(&self) -> String {
        let stderr = self.stderr.trim();
        if !stderr.is_empty() {
            return stderr.to_string();
        }
        let stdout = self.stdout.trim();
        if !stdout.is_empty() {
            return stdout.to_string();
        }
        format!("command exited with status {}", self.status)
    }
}

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(20);
/// Commands allowed this long are jobs (installs, dumps, archives), run at
/// normal priority rather than the panel's raised one.
const LONG_JOB: Duration = Duration::from_secs(120);

/// Runs a command, capturing output. Returns Err only when the binary cannot be spawned.
pub fn run(program: &str, args: &[&str]) -> Result<CmdOutput, String> {
    run_with_timeout(program, args, DEFAULT_TIMEOUT)
}

pub fn run_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<CmdOutput, String> {
    run_with_input(program, args, None, timeout)
}

/// Like `run_with_timeout`, feeding `input` to stdin. Used for SQL and secrets,
/// which must not appear in the process list as arguments.
pub fn run_with_input(
    program: &str,
    args: &[&str],
    input: Option<&str>,
    timeout: Duration,
) -> Result<CmdOutput, String> {
    let mut command = Command::new(program);
    command.args(args);
    run_command(command, input, timeout)
}

/// Runs a prepared `Command` (for a working directory or extra environment),
/// with the same output capture, stdin feeding and timeout as `run`.
pub fn run_command(
    mut command: Command,
    input: Option<&str>,
    timeout: Duration,
) -> Result<CmdOutput, String> {
    let program = command.get_program().to_string_lossy().to_string();
    let program = program.as_str();
    if timeout >= LONG_JOB {
        use std::os::unix::process::CommandExt;
        // SAFETY: the hook only makes a system call.
        unsafe {
            command.pre_exec(normal_priority);
        }
    }
    let mut child = command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("{}: {}", program, err))?;

    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        let input = input.to_string();
        // A writer thread, so a child that does not read stdin cannot deadlock us.
        std::thread::spawn(move || {
            use std::io::Write;
            let _ = stdin.write_all(input.as_bytes());
        });
    }

    // Inside a background task, show the command and stream its output live.
    // `which` lookups are internal plumbing, not steps worth showing.
    let task = super::tasks::current().filter(|_| program != "which");
    if let Some(task) = &task {
        task.append(&format!("$ {}\n", describe(&command)));
    }

    // Drain both pipes while waiting: a child that fills the ~64KB pipe buffer
    // (package installs, ACME clients) would otherwise block until the timeout.
    let stdout_reader = drain_pipe(child.stdout.take(), task.clone());
    let stderr_reader = drain_pipe(child.stderr.take(), task);

    // Wait on a helper thread so the result arrives the moment the child exits
    // (polling added up to a poll interval to every command, which every page
    // paid several times over, and much more on a busy server).
    let pid = child.id();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = done_tx.send(child.wait());
    });
    let status = match done_rx.recv_timeout(timeout) {
        Ok(Ok(status)) => status,
        Ok(Err(err)) => return Err(format!("{}: {}", program, err)),
        Err(_) => {
            // The child is not reaped until the waiter's wait() returns, so the
            // pid still belongs to it here.
            kill_pid(pid);
            let _ = done_rx.recv_timeout(Duration::from_secs(5));
            return Err(format!("{} timed out after {:?}", program, timeout));
        }
    };

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();

    Ok(CmdOutput {
        status: status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&stdout).to_string(),
        stderr: String::from_utf8_lossy(&stderr).to_string(),
    })
}

fn drain_pipe<R: std::io::Read + Send + 'static>(
    pipe: Option<R>,
    task: Option<std::sync::Arc<super::tasks::Task>>,
) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let Some(mut pipe) = pipe else {
            return buffer;
        };
        let mut chunk = [0u8; 8192];
        // Bytes of an incomplete UTF-8 sequence carried to the next read.
        let mut pending = 0usize;
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    buffer.extend_from_slice(&chunk[..n]);
                    if let Some(task) = &task {
                        let fresh = &buffer[buffer.len() - n - pending..];
                        let valid = match std::str::from_utf8(fresh) {
                            Ok(_) => fresh.len(),
                            Err(e) if e.error_len().is_none() => e.valid_up_to(),
                            Err(_) => fresh.len(),
                        };
                        task.append(&String::from_utf8_lossy(&fresh[..valid]));
                        pending = fresh.len() - valid;
                    }
                }
            }
        }
        buffer
    })
}

/// "program arg ..." for a task log, with anything that looks secret masked.
fn describe(command: &Command) -> String {
    let mut parts = vec![command.get_program().to_string_lossy().into_owned()];
    for arg in command.get_args() {
        let arg = arg.to_string_lossy();
        let lower = arg.to_ascii_lowercase();
        let secret = ["password", "passwd", "secret", "token", "identified by"]
            .iter()
            .any(|word| lower.contains(word));
        parts.push(if secret {
            "***".into()
        } else {
            arg.into_owned()
        });
    }
    let line = parts.join(" ");
    if line.len() > 400 {
        format!(
            "{}…",
            &line[..line
                .char_indices()
                .nth(400)
                .map(|(i, _)| i)
                .unwrap_or(line.len())]
        )
    } else {
        line
    }
}

/// Runs a command, elevating with sudo when the panel is not already root.
/// Only non-interactive sudo is attempted, so this never blocks on a password prompt.
pub fn run_privileged(program: &str, args: &[&str]) -> Result<CmdOutput, String> {
    run_privileged_with_timeout(program, args, DEFAULT_TIMEOUT)
}

/// `run_privileged` with an explicit timeout, for long operations like package installs.
pub fn run_privileged_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
) -> Result<CmdOutput, String> {
    if is_root() {
        return run_with_timeout(program, args, timeout);
    }
    let mut sudo_args = vec!["-n", program];
    sudo_args.extend_from_slice(args);
    let result = run_with_timeout("sudo", &sudo_args, timeout)?;
    if !result.ok() && result.stderr.contains("password is required") {
        return Err("This action requires root privileges or passwordless sudo".into());
    }
    Ok(result)
}

/// Like `run_privileged`, but falls back to running unprivileged when sudo is unavailable.
/// Used for read-only inspection where a partial result beats an error.
pub fn run_best_effort(program: &str, args: &[&str]) -> Result<CmdOutput, String> {
    if is_root() {
        return run(program, args);
    }
    let mut sudo_args = vec!["-n", program];
    sudo_args.extend_from_slice(args);
    if let Ok(output) = run("sudo", &sudo_args) {
        if output.ok() {
            return Ok(output);
        }
    }
    run(program, args)
}

pub fn is_root() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: geteuid is always safe to call and has no preconditions.
        unsafe { libc_geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(unix)]
extern "C" {
    #[link_name = "geteuid"]
    fn libc_geteuid() -> u32;
    #[link_name = "kill"]
    fn libc_kill(pid: i32, sig: i32) -> i32;
    #[link_name = "setpriority"]
    fn libc_setpriority(which: i32, who: u32, prio: i32) -> i32;
}

/// Puts a child back to normal CPU priority. The panel itself runs at a
/// raised priority (Nice=-5) so it stays responsive on a saturated server;
/// the apps it manages and its long jobs (installs, dumps) must not inherit
/// that and compete with it. Called between fork and exec: it only makes a
/// system call.
pub fn normal_priority() -> std::io::Result<()> {
    #[cfg(unix)]
    {
        // SAFETY: setpriority is async-signal-safe; PRIO_PROCESS = 0, who 0 = self.
        unsafe {
            libc_setpriority(0, 0, 0);
        }
    }
    Ok(())
}

fn kill_pid(pid: u32) {
    #[cfg(unix)]
    {
        // SAFETY: kill has no memory preconditions; SIGKILL = 9.
        unsafe {
            libc_kill(pid as i32, 9);
        }
    }
}

/// Where `program` lives on PATH (or the path itself when it contains a
/// slash), looked up in-process: `which` cost a process per check.
pub fn which(program: &str) -> Option<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let executable = |path: &std::path::Path| {
        std::fs::metadata(path)
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    };
    if program.is_empty() {
        return None;
    }
    if program.contains('/') {
        let path = std::path::PathBuf::from(program);
        return executable(&path).then_some(path);
    }
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path_var)
        // The panel may run with a minimal PATH; system tools live in sbin.
        .chain(
            [
                "/usr/local/sbin",
                "/usr/local/bin",
                "/usr/sbin",
                "/usr/bin",
                "/sbin",
                "/bin",
            ]
            .map(Into::into),
        )
        .map(|dir: std::path::PathBuf| dir.join(program))
        .find(|candidate| executable(candidate))
}

/// Whether a program exists on PATH.
pub fn has_binary(program: &str) -> bool {
    which(program).is_some()
}

/// The output of `program args` (stdout, or stderr when stdout is empty),
/// cached per binary until the binary file changes (an upgrade or a new
/// install), so version probes cost nothing after the first page load.
pub fn cached_version_output(program: &str, args: &[&str]) -> Option<String> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    type Key = (std::path::PathBuf, String);
    type Stamp = (Option<std::time::SystemTime>, u64);
    static CACHE: OnceLock<Mutex<HashMap<Key, (Stamp, String)>>> = OnceLock::new();

    let path = which(program)?;
    // Resolve symlinks (alternatives): the target changes on upgrades.
    let real = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
    let meta = std::fs::metadata(&real).ok()?;
    let stamp: Stamp = (meta.modified().ok(), meta.len());
    let key: Key = (real, args.join(" "));
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((cached_stamp, text)) = cache.lock().ok()?.get(&key) {
        if *cached_stamp == stamp {
            return Some(text.clone());
        }
    }
    let output = run_with_timeout(&path.to_string_lossy(), args, Duration::from_secs(15)).ok()?;
    let text = if output.stdout.trim().is_empty() {
        output.stderr
    } else {
        output.stdout
    };
    if let Ok(mut map) = cache.lock() {
        map.insert(key, (stamp, text.clone()));
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_binaries_in_process() {
        assert!(which("sh").is_some());
        assert!(which("/bin/sh").is_some());
        assert!(which("definitely-not-a-binary-xyz").is_none());
        assert!(!has_binary(""));
    }

    #[test]
    fn commands_return_without_poll_delay() {
        let start = std::time::Instant::now();
        for _ in 0..10 {
            assert!(run("true", &[]).unwrap().ok());
        }
        // Ten trivial commands used to cost at least 10 x 20 ms.
        assert!(
            start.elapsed() < Duration::from_millis(150),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn timeouts_kill_the_child() {
        let start = std::time::Instant::now();
        let result = run_with_timeout("sleep", &["5"], Duration::from_millis(200));
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn version_output_is_cached() {
        let first = cached_version_output("sh", &["-c", "echo $$"]).unwrap();
        let second = cached_version_output("sh", &["-c", "echo $$"]).unwrap();
        assert_eq!(first, second);
    }
}
