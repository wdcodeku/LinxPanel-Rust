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
    let task = super::tasks::current();
    if let Some(task) = &task {
        task.append(&format!("$ {}\n", describe(&command)));
    }

    // Drain both pipes while waiting: a child that fills the ~64KB pipe buffer
    // (package installs, ACME clients) would otherwise block until the timeout.
    let stdout_reader = drain_pipe(child.stdout.take(), task.clone());
    let stderr_reader = drain_pipe(child.stderr.take(), task);

    // portable wait-with-timeout: poll try_wait rather than pull in an extra crate.
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("{} timed out after {:?}", program, timeout));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(err) => return Err(format!("{}: {}", program, err)),
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
        parts.push(if secret { "***".into() } else { arg.into_owned() });
    }
    let line = parts.join(" ");
    if line.len() > 400 {
        format!("{}…", &line[..line.char_indices().nth(400).map(|(i, _)| i).unwrap_or(line.len())])
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
}

/// Whether a program exists on PATH.
pub fn has_binary(program: &str) -> bool {
    let probe = if cfg!(windows) { "where" } else { "which" };
    run(probe, &[program])
        .map(|output| output.ok())
        .unwrap_or(false)
}
