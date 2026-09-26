//! Crontab management for the panel user.
//!
//! The whole crontab is read, edited in memory and written back atomically via
//! `crontab -`, which is the only safe way to mutate it without racing other writers.

use serde::{Deserialize, Serialize};

use super::shell;

/// Marks the comment line the panel writes above each job it owns.
const NAME_PREFIX: &str = "#LinxPanel:";

#[derive(Debug, Clone, Serialize)]
pub struct CronJob {
    /// Stable within one listing; used to address a job for edit/delete.
    pub id: usize,
    pub name: String,
    pub schedule: String,
    pub command: String,
    pub enabled: bool,
    /// Raw line as it appears in the crontab, for display and round-tripping.
    pub raw: String,
}

#[derive(Debug, Deserialize)]
pub struct CronJobRequest {
    #[serde(default)]
    pub name: String,
    pub schedule: String,
    pub command: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct CronBackend {
    pub available: bool,
    pub message: String,
}

pub fn backend() -> CronBackend {
    if cfg!(windows) {
        return CronBackend {
            available: false,
            message: "Cron management is available on Linux and macOS only".into(),
        };
    }
    if !shell::has_binary("crontab") {
        return CronBackend {
            available: false,
            message: "crontab is not installed on this system".into(),
        };
    }
    CronBackend {
        available: true,
        message: String::new(),
    }
}

fn read_crontab() -> Result<Vec<String>, String> {
    let output = shell::run("crontab", &["-l"])?;
    if !output.ok() {
        let text = output.error_text();
        // An empty crontab is reported as an error by most implementations.
        if text.contains("no crontab") || text.contains("no crontab for") {
            return Ok(Vec::new());
        }
        return Err(text);
    }
    Ok(output.stdout.lines().map(|line| line.to_string()).collect())
}

fn write_crontab(lines: &[String]) -> Result<(), String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut body = lines.join("\n");
    if !body.ends_with('\n') {
        body.push('\n');
    }

    let mut child = Command::new("crontab")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("crontab: {}", err))?;

    child
        .stdin
        .as_mut()
        .ok_or_else(|| "Failed to open crontab stdin".to_string())?
        .write_all(body.as_bytes())
        .map_err(|err| format!("Failed to write crontab: {}", err))?;

    let output = child
        .wait_with_output()
        .map_err(|err| format!("crontab: {}", err))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "Failed to save crontab".into()
        } else {
            stderr
        });
    }
    Ok(())
}

pub fn list() -> Result<Vec<CronJob>, String> {
    let backend = backend();
    if !backend.available {
        return Err(backend.message);
    }
    Ok(parse_jobs(&read_crontab()?))
}

fn parse_jobs(lines: &[String]) -> Vec<CronJob> {
    let mut jobs = Vec::new();
    let mut pending_name: Option<String> = None;

    for line in lines {
        let trimmed = line.trim();

        if let Some(rest) = trimmed.strip_prefix(NAME_PREFIX) {
            pending_name = Some(rest.trim().to_string());
            continue;
        }
        if trimmed.is_empty() {
            pending_name = None;
            continue;
        }

        // A disabled job is stored commented out, so look through a single leading '#'.
        let (enabled, body) = if let Some(rest) = trimmed.strip_prefix('#') {
            (false, rest.trim())
        } else {
            (true, trimmed)
        };

        if let Some((schedule, command)) = split_schedule(body) {
            jobs.push(CronJob {
                id: jobs.len(),
                name: pending_name.take().unwrap_or_default(),
                schedule,
                command,
                enabled,
                raw: line.clone(),
            });
        } else {
            // Not a job line (env assignment, free comment); drop any name we were holding.
            pending_name = None;
        }
    }
    jobs
}

/// Splits `*/5 * * * * cmd` into schedule and command. Handles `@daily cmd` too.
fn split_schedule(line: &str) -> Option<(String, String)> {
    if line.starts_with('@') {
        let mut parts = line.splitn(2, char::is_whitespace);
        let schedule = parts.next()?.to_string();
        let command = parts.next()?.trim().to_string();
        if command.is_empty() {
            return None;
        }
        return Some((schedule, command));
    }

    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 6 {
        return None;
    }
    let schedule = fields[..5].join(" ");
    if !fields[..5].iter().all(|field| is_cron_field(field)) {
        return None;
    }
    // Recover the command with its original inner spacing.
    let command = line
        .splitn(6, char::is_whitespace)
        .last()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;
    Some((schedule, command))
}

fn is_cron_field(field: &str) -> bool {
    !field.is_empty()
        && field
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '*' | '/' | ',' | '-'))
}

pub fn validate(request: &CronJobRequest) -> Result<(), String> {
    let schedule = request.schedule.trim();
    let command = request.command.trim();
    if command.is_empty() {
        return Err("Command is required".into());
    }
    if command.contains('\n') || request.name.contains('\n') || schedule.contains('\n') {
        return Err("Newlines are not allowed".into());
    }
    if schedule.starts_with('@') {
        const SPECIALS: &[&str] = &[
            "@reboot", "@yearly", "@annually", "@monthly", "@weekly", "@daily", "@midnight",
            "@hourly",
        ];
        if !SPECIALS.contains(&schedule) {
            return Err("Invalid special schedule".into());
        }
        return Ok(());
    }
    let fields: Vec<&str> = schedule.split_whitespace().collect();
    if fields.len() != 5 || !fields.iter().all(|f| is_cron_field(f)) {
        return Err("Schedule must be 5 cron fields, e.g. */5 * * * *".into());
    }
    Ok(())
}

fn render(request: &CronJobRequest) -> Vec<String> {
    let mut lines = Vec::new();
    let name = request.name.trim();
    if !name.is_empty() {
        lines.push(format!("{} {}", NAME_PREFIX, name));
    }
    let body = format!("{} {}", request.schedule.trim(), request.command.trim());
    lines.push(if request.enabled {
        body
    } else {
        format!("#{}", body)
    });
    lines
}

pub fn create(request: &CronJobRequest) -> Result<(), String> {
    let backend = backend();
    if !backend.available {
        return Err(backend.message);
    }
    validate(request)?;

    let mut lines = read_crontab()?;
    if !lines.is_empty() && !lines.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.push(String::new());
    }
    lines.extend(render(request));
    write_crontab(&lines)
}

pub fn update(id: usize, request: &CronJobRequest) -> Result<(), String> {
    let backend = backend();
    if !backend.available {
        return Err(backend.message);
    }
    validate(request)?;

    let lines = read_crontab()?;
    let jobs = parse_jobs(&lines);
    let job = jobs.get(id).ok_or_else(|| "Cron job not found".to_string())?;

    let rebuilt = replace_job(&lines, job, render(request))?;
    write_crontab(&rebuilt)
}

pub fn delete(id: usize) -> Result<(), String> {
    let backend = backend();
    if !backend.available {
        return Err(backend.message);
    }
    let lines = read_crontab()?;
    let jobs = parse_jobs(&lines);
    let job = jobs.get(id).ok_or_else(|| "Cron job not found".to_string())?;

    let rebuilt = replace_job(&lines, job, Vec::new())?;
    write_crontab(&rebuilt)
}

pub fn toggle(id: usize, enabled: bool) -> Result<(), String> {
    let lines = read_crontab()?;
    let jobs = parse_jobs(&lines);
    let job = jobs.get(id).ok_or_else(|| "Cron job not found".to_string())?;
    if job.enabled == enabled {
        return Ok(());
    }
    update(
        id,
        &CronJobRequest {
            name: job.name.clone(),
            schedule: job.schedule.clone(),
            command: job.command.clone(),
            enabled,
        },
    )
}

/// Swaps the lines belonging to `job` (its optional name comment plus its body) for `replacement`.
fn replace_job(
    lines: &[String],
    job: &CronJob,
    replacement: Vec<String>,
) -> Result<Vec<String>, String> {
    let index = lines
        .iter()
        .position(|line| line == &job.raw)
        .ok_or_else(|| "Cron job changed on disk, please refresh".to_string())?;

    let mut start = index;
    if !job.name.is_empty() && index > 0 {
        let previous = lines[index - 1].trim();
        if previous
            .strip_prefix(NAME_PREFIX)
            .map(|rest| rest.trim() == job.name)
            .unwrap_or(false)
        {
            start = index - 1;
        }
    }

    let mut rebuilt: Vec<String> = Vec::with_capacity(lines.len());
    rebuilt.extend_from_slice(&lines[..start]);
    rebuilt.extend(replacement);
    rebuilt.extend_from_slice(&lines[index + 1..]);
    Ok(rebuilt)
}
