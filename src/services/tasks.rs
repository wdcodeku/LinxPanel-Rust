//! Long operations (package installs, deployments) as background tasks.
//!
//! An endpoint starts a task and returns its id at once; the browser follows
//! the task's log while it runs and gets the endpoint's normal response when
//! it ends. Output of every command a task runs is streamed into its log as it
//! arrives (see `shell::run_command`), so an install never looks frozen.

use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Arc, Mutex};

use serde::Serialize;

/// Most tasks kept for the task list.
const MAX_TASKS: usize = 30;
/// Longest log kept per task (the tail is what matters).
const MAX_LOG: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Success,
    Failed,
}

#[derive(Debug)]
struct State {
    status: Status,
    /// Everything ever appended, minus what was trimmed from the front.
    log: String,
    /// Bytes trimmed from the front, so client offsets stay valid.
    trimmed: usize,
    finished_at: Option<i64>,
    message: String,
    data: Option<serde_json::Value>,
}

#[derive(Debug)]
pub struct Task {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub started_at: i64,
    state: Mutex<State>,
}

impl Task {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Appends output; safe from any thread.
    pub fn append(&self, text: &str) {
        let mut state = self.lock();
        state.log.push_str(text);
        if state.log.len() > MAX_LOG {
            let mut cut = state.log.len() - MAX_LOG;
            while !state.log.is_char_boundary(cut) {
                cut += 1;
            }
            state.log.drain(..cut);
            state.trimmed += cut;
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub status: Status,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub message: String,
}

/// A slice of a task's log from `offset`, plus its outcome once finished.
#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    #[serde(flatten)]
    pub summary: Summary,
    pub log: String,
    /// Pass back as `offset` to get only what is new.
    pub next_offset: usize,
    /// The endpoint's own `success` / `data` when finished.
    pub success: Option<bool>,
    pub data: Option<serde_json::Value>,
}

#[derive(Default)]
pub struct Tasks {
    list: Mutex<VecDeque<Arc<Task>>>,
}

tokio::task_local! {
    static CURRENT: Arc<Task>;
}

thread_local! {
    static THREAD_TASK: std::cell::RefCell<Option<Arc<Task>>> = const { std::cell::RefCell::new(None) };
}

/// The task the calling code runs for, if any (async or blocking context).
pub fn current() -> Option<Arc<Task>> {
    THREAD_TASK
        .with(|t| t.borrow().clone())
        .or_else(|| CURRENT.try_with(|t| t.clone()).ok())
}

/// Writes a line to the current task's log (no-op outside a task).
pub fn note(line: &str) {
    if let Some(task) = current() {
        task.append(line);
        if !line.ends_with('\n') {
            task.append("\n");
        }
    }
}

/// `tokio::task::spawn_blocking`, carrying the current task along so command
/// output from the blocking code is streamed into its log.
pub async fn blocking<F, R>(f: F) -> Result<R, tokio::task::JoinError>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let task = current();
    tokio::task::spawn_blocking(move || {
        THREAD_TASK.with(|t| *t.borrow_mut() = task);
        let result = f();
        THREAD_TASK.with(|t| *t.borrow_mut() = None);
        result
    })
    .await
}

#[derive(Debug, Clone, Serialize)]
pub struct Started {
    pub task_id: String,
}

impl Tasks {
    /// Runs `work` (an endpoint body producing its usual response) as a task.
    pub fn start<Fut, T>(self: &Arc<Self>, kind: &str, title: &str, work: Fut) -> Started
    where
        Fut: Future<Output = crate::models::ApiResponse<T>> + Send + 'static,
        T: Serialize + Send + 'static,
    {
        let task = Arc::new(Task {
            id: uuid::Uuid::new_v4().simple().to_string()[..12].to_string(),
            kind: kind.to_string(),
            title: title.to_string(),
            started_at: chrono::Utc::now().timestamp(),
            state: Mutex::new(State {
                status: Status::Running,
                log: String::new(),
                trimmed: 0,
                finished_at: None,
                message: String::new(),
                data: None,
            }),
        });
        {
            let mut list = self.list.lock().unwrap_or_else(|e| e.into_inner());
            list.push_front(task.clone());
            // Drop the oldest finished tasks beyond the cap.
            while list.len() > MAX_TASKS {
                match list
                    .iter()
                    .rposition(|t| t.lock().status != Status::Running)
                {
                    Some(index) => {
                        list.remove(index);
                    }
                    None => break,
                }
            }
        }
        let id = task.id.clone();
        let running = task.clone();
        tokio::spawn(async move {
            // A panic in the work must still end the task.
            let outcome = tokio::spawn(CURRENT.scope(running.clone(), work)).await;
            let mut state = running.lock();
            match outcome {
                Ok(response) => {
                    state.status = if response.success {
                        Status::Success
                    } else {
                        Status::Failed
                    };
                    state.message = response.message;
                    let data = response.data.and_then(|d| serde_json::to_value(d).ok());
                    // A full log in the response is more complete than the
                    // streamed one (it has the panel's own step lines too).
                    if let Some(full) = data.as_ref().and_then(|d| d["log"].as_str()) {
                        if full.len() >= state.log.len() / 2 {
                            state.trimmed += state.log.len();
                            state.log = String::new();
                            let full = full.to_string();
                            drop(state);
                            running.append(&full);
                            state = running.lock();
                        }
                    }
                    // Some endpoints report a failed operation inside their data.
                    if data.as_ref().and_then(|d| d["success"].as_bool()) == Some(false)
                        && state.status == Status::Success
                    {
                        state.status = Status::Failed;
                        if state.message.is_empty() || state.message == "ok" {
                            state.message = "Failed - see the log".into();
                        }
                    }
                    state.data = data;
                }
                Err(err) => {
                    state.status = Status::Failed;
                    state.message = format!("Task stopped unexpectedly: {}", err);
                }
            }
            state.finished_at = Some(chrono::Utc::now().timestamp());
        });
        Started { task_id: id }
    }

    pub fn list(&self) -> Vec<Summary> {
        self.list
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|t| summary(t, &t.lock()))
            .collect()
    }

    pub fn progress(&self, id: &str, offset: usize) -> Option<Progress> {
        let task = self
            .list
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|t| t.id == id)
            .cloned()?;
        let state = task.lock();
        let total = state.trimmed + state.log.len();
        // Offsets are absolute; anything before the kept tail is gone.
        let mut start = offset.saturating_sub(state.trimmed).min(state.log.len());
        while !state.log.is_char_boundary(start) {
            start -= 1;
        }
        let finished = state.status != Status::Running;
        Some(Progress {
            summary: summary(&task, &state),
            log: state.log[start..].to_string(),
            next_offset: total,
            success: finished.then_some(state.status == Status::Success),
            data: if finished { state.data.clone() } else { None },
        })
    }
}

fn summary(task: &Task, state: &State) -> Summary {
    Summary {
        id: task.id.clone(),
        kind: task.kind.clone(),
        title: task.title.clone(),
        status: state.status,
        started_at: task.started_at,
        finished_at: state.finished_at,
        message: state.message.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ApiResponse;

    #[tokio::test]
    async fn streams_output_and_returns_the_response() {
        let tasks = Arc::new(Tasks::default());
        let started = tasks.start("test", "Test", async {
            note("[step] one");
            // Blocking work keeps streaming into the same task.
            blocking(|| {
                crate::services::shell::run("sh", &["-c", "echo from-child; echo err-line >&2"])
            })
            .await
            .unwrap()
            .unwrap();
            ApiResponse::ok(serde_json::json!({ "answer": 42 }))
        });
        let mut offset = 0;
        let mut log = String::new();
        let done = loop {
            let progress = tasks.progress(&started.task_id, offset).unwrap();
            log.push_str(&progress.log);
            offset = progress.next_offset;
            if progress.success.is_some() {
                break progress;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        };
        assert!(log.contains("[step] one"), "{log}");
        assert!(
            log.contains("from-child") && log.contains("err-line"),
            "{log}"
        );
        assert_eq!(done.success, Some(true));
        assert_eq!(done.data.unwrap()["answer"], 42);
        assert_eq!(tasks.list()[0].status, Status::Success);
    }

    #[tokio::test]
    async fn a_failure_reported_in_the_data_fails_the_task() {
        let tasks = Arc::new(Tasks::default());
        let started = tasks.start("t", "install", async {
            ApiResponse::ok(serde_json::json!({ "success": false, "log": "E: no such package" }))
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let done = tasks.progress(&started.task_id, 0).unwrap();
        assert_eq!(done.success, Some(false));
        assert!(done.log.contains("no such package"));
    }

    #[tokio::test]
    async fn failures_and_panics_end_the_task() {
        let tasks = Arc::new(Tasks::default());
        let failed = tasks.start("t", "fails", async { ApiResponse::<()>::err("nope") });
        let panicked = tasks.start("t", "panics", async {
            if true {
                panic!("boom");
            }
            ApiResponse::<()>::ok_msg("unreachable")
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let failed = tasks.progress(&failed.task_id, 0).unwrap();
        assert_eq!(
            (failed.success, failed.summary.message.as_str()),
            (Some(false), "nope")
        );
        let panicked = tasks.progress(&panicked.task_id, 0).unwrap();
        assert_eq!(panicked.success, Some(false));
        assert!(panicked.summary.message.contains("unexpectedly"));
    }

    #[test]
    fn log_offsets_survive_trimming() {
        let task = Task {
            id: "x".into(),
            kind: "k".into(),
            title: "t".into(),
            started_at: 0,
            state: Mutex::new(State {
                status: Status::Running,
                log: String::new(),
                trimmed: 0,
                finished_at: None,
                message: String::new(),
                data: None,
            }),
        };
        let chunk = "é".repeat(400_000); // 800 KB of 2-byte chars
        for _ in 0..4 {
            task.append(&chunk);
        }
        let state = task.lock();
        assert!(state.log.len() <= MAX_LOG);
        assert_eq!(state.trimmed + state.log.len(), 4 * chunk.len());
    }
}
