//! Runtime environments and managed application endpoints.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::models::ApiResponse;
use crate::services::appproc::{self, AppView, SaveAppRequest};
use crate::services::tasks;
use crate::services::runtime::{self, RuntimeInstallRequest, RuntimeInstallResult, RuntimeStatus};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/runtimes", get(list_runtimes))
        .route("/runtimes/install", post(install_runtime))
        .route("/apps", get(list_apps).post(create_app))
        .route("/apps/{id}", post(update_app).delete(delete_app))
        .route("/apps/{id}/start", post(start_app))
        .route("/apps/{id}/stop", post(stop_app))
        .route("/apps/{id}/restart", post(restart_app))
        .route("/apps/{id}/install-deps", post(install_deps))
        .route("/apps/from-sample", post(create_from_sample))
        .route("/apps/{id}/logs", get(app_logs).delete(clear_app_logs))
}

// ===== Runtimes =====

async fn list_runtimes() -> Json<ApiResponse<Vec<RuntimeStatus>>> {
    // Detection shells out to several binaries, so keep it off the async workers.
    match tokio::task::spawn_blocking(runtime::list).await {
        Ok(list) => Json(ApiResponse::ok(list)),
        Err(_) => Json(ApiResponse::err("Failed to detect runtimes")),
    }
}

/// Starts the install as a background task (its log streams to the browser).
async fn install_runtime(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RuntimeInstallRequest>,
) -> Json<ApiResponse<tasks::Started>> {
    let title = format!("Install {}", body.kind);
    Json(ApiResponse::ok(state.tasks.start(
        "runtime-install",
        &title,
        install_runtime_task(body),
    )))
}

async fn install_runtime_task(body: RuntimeInstallRequest) -> ApiResponse<RuntimeInstallResult> {
    match tasks::blocking(move || runtime::install(&body.kind)).await {
        Ok(Ok(result)) => ApiResponse::ok(result),
        Ok(Err(err)) => ApiResponse::err(&err),
        Err(_) => ApiResponse::err("Runtime install task failed"),
    }
}

// ===== Applications =====

async fn list_apps(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<AppView>>> {
    let records = match appproc::list(&state.db).await {
        Ok(records) => records,
        Err(err) => return Json(ApiResponse::err(&err)),
    };

    let mut views = Vec::with_capacity(records.len());
    for record in records {
        let runtime_state = state.supervisor.state_of(record.id).await;
        views.push(AppView {
            record,
            state: runtime_state,
        });
    }
    Json(ApiResponse::ok(views))
}

async fn create_app(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SaveAppRequest>,
) -> Json<ApiResponse<i64>> {
    match appproc::create(&state.db, &body).await {
        Ok(id) => Json(ApiResponse::ok(id)),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn update_app(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<SaveAppRequest>,
) -> Json<ApiResponse<()>> {
    match appproc::update(&state.db, id, &body).await {
        Ok(()) => Json(ApiResponse::ok_msg("Application updated")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn delete_app(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<()>> {
    // Stop first so deleting never orphans a running process.
    let _ = appproc::stop(state.supervisor.clone(), id).await;
    match appproc::delete(&state.db, id).await {
        Ok(()) => Json(ApiResponse::ok_msg("Application deleted")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn start_app(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<()>> {
    match appproc::start(
        state.supervisor.clone(),
        state.db.clone(),
        state.data_dir.clone(),
        id,
    )
    .await
    {
        Ok(()) => Json(ApiResponse::ok_msg("Application started")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn stop_app(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<()>> {
    match appproc::stop(state.supervisor.clone(), id).await {
        Ok(()) => Json(ApiResponse::ok_msg("Application stopped")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

async fn restart_app(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<()>> {
    match appproc::restart(
        state.supervisor.clone(),
        state.db.clone(),
        state.data_dir.clone(),
        id,
    )
    .await
    {
        Ok(()) => Json(ApiResponse::ok_msg("Application restarted")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}

#[derive(Debug, Deserialize, Default)]
struct InstallDepsRequest {
    /// Node: also run the `build` script when package.json has one.
    #[serde(default)]
    build: bool,
}

#[derive(Serialize)]
struct InstallDepsResult {
    success: bool,
    log: String,
}

/// pip / npm / pnpm / yarn / composer install in the app's working directory.
async fn install_deps(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    body: Option<Json<InstallDepsRequest>>,
) -> Json<ApiResponse<tasks::Started>> {
    let record = match appproc::get(&state.db, id).await {
        Ok(record) => record,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    let build = body.map(|Json(b)| b.build).unwrap_or(false);
    let title = format!("Install dependencies: {}", record.name);
    Json(ApiResponse::ok(state.tasks.start("app-deps", &title, async move {
        match tasks::blocking(move || appproc::install_dependencies(&record, build)).await {
            Ok(Ok(log)) => ApiResponse::ok(InstallDepsResult { success: true, log }),
            Ok(Err(log)) => ApiResponse::ok(InstallDepsResult {
                success: false,
                log,
            }),
            Err(err) => ApiResponse::err(&err.to_string()),
        }
    })))
}

#[derive(Debug, Deserialize)]
struct SampleRequest {
    name: String,
    runtime: String,
    work_dir: String,
    port: u16,
    #[serde(default)]
    run_user: String,
}

#[derive(Serialize)]
struct SampleResult {
    success: bool,
    log: String,
    id: Option<i64>,
    port: u16,
}

/// Writes a sample project, registers it, builds it when needed, starts it
/// and checks it answers - all as one background task.
async fn create_from_sample(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SampleRequest>,
) -> Json<ApiResponse<tasks::Started>> {
    if req.name.trim().is_empty() {
        return Json(ApiResponse::err("Application name is required"));
    }
    if req.port < 1024 {
        return Json(ApiResponse::err("Choose a port of 1024 or above"));
    }
    let dir = std::path::PathBuf::from(req.work_dir.trim());
    if !dir.is_absolute() {
        return Json(ApiResponse::err("Working directory must be an absolute path"));
    }
    let title = format!("Sample app: {}", req.name.trim());
    let task_state = state.clone();
    Json(ApiResponse::ok(state.tasks.start("app-sample", &title, async move {
        sample_task(task_state, req, dir).await
    })))
}

async fn sample_task(state: Arc<AppState>, req: SampleRequest, dir: std::path::PathBuf) -> ApiResponse<SampleResult> {
    let mut log = String::new();
    let note = |log: &mut String, line: &str| {
        tasks::note(line);
        log.push_str(line);
        log.push('\n');
    };
    let fail = |log: String| ApiResponse::ok(SampleResult { success: false, log, id: None, port: req.port });
    if crate::services::svcapps::port_open(req.port).await {
        return ApiResponse::err(&format!("Port {} is already in use", req.port));
    }
    let dotnet_major = if req.runtime == "dotnet" {
        tasks::blocking(|| {
            crate::services::shell::cached_version_output("dotnet", &["--version"])
                .and_then(|v| v.trim().split('.').next()?.parse::<u32>().ok())
        })
        .await
        .ok()
        .flatten()
    } else {
        None
    };
    let sample = match crate::services::samples::sample(&req.runtime, &req.name, req.port, dotnet_major) {
        Ok(sample) => sample,
        Err(err) => return ApiResponse::err(&err),
    };
    note(&mut log, &format!("[LinxPanel] Writing the sample into {}", dir.display()));
    {
        let (dir, files) = (dir.clone(), sample.files.clone());
        let written = tasks::blocking(move || {
            crate::services::samples::write(&dir, &crate::services::samples::Sample { files, entry: String::new(), build: false })
        })
        .await;
        match written {
            Ok(Ok(())) => {}
            Ok(Err(err)) => return ApiResponse::err(&err),
            Err(err) => return ApiResponse::err(&err.to_string()),
        }
    }
    let request = appproc::SaveAppRequest {
        name: req.name.trim().to_string(),
        runtime: req.runtime.clone(),
        work_dir: dir.to_string_lossy().to_string(),
        entry: sample.entry.clone(),
        args: String::new(),
        env: String::new(),
        port: req.port as i64,
        autostart: true,
        run_user: req.run_user.clone(),
        chown_work_dir: !req.run_user.trim().is_empty(),
    };
    let id = match appproc::create(&state.db, &request).await {
        Ok(id) => id,
        Err(err) => {
            note(&mut log, &format!("[LinxPanel] {}", err));
            return fail(log);
        }
    };
    if sample.build {
        note(&mut log, "[LinxPanel] Building / installing dependencies");
        let record = match appproc::get(&state.db, id).await {
            Ok(record) => record,
            Err(err) => return ApiResponse::err(&err),
        };
        match tasks::blocking(move || appproc::install_dependencies(&record, true)).await {
            Ok(Ok(text)) => log.push_str(&text),
            Ok(Err(text)) => {
                log.push_str(&text);
                note(&mut log, "[LinxPanel] The build failed; the app was created and can be rebuilt from the Runtimes page");
                return ApiResponse::ok(SampleResult { success: false, log, id: Some(id), port: req.port });
            }
            Err(err) => return ApiResponse::err(&err.to_string()),
        }
    }
    note(&mut log, "[LinxPanel] Starting");
    if let Err(err) = appproc::start(state.supervisor.clone(), state.db.clone(), state.data_dir.clone(), id).await {
        note(&mut log, &format!("[LinxPanel] {}", err));
        return ApiResponse::ok(SampleResult { success: false, log, id: Some(id), port: req.port });
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        if crate::services::svcapps::port_open(req.port).await {
            note(&mut log, &format!("[LinxPanel] Done: the sample answers on port {}", req.port));
            return ApiResponse::ok(SampleResult { success: true, log, id: Some(id), port: req.port });
        }
        let st = state.supervisor.state_of(id).await;
        if st.restarts > 0 || std::time::Instant::now() >= deadline {
            let tail = appproc::read_log(&state.data_dir, id, 20);
            note(&mut log, &format!("[LinxPanel] The app did not come up: {}", st.last_error));
            if !tail.trim().is_empty() {
                note(&mut log, tail.trim_end());
            }
            return ApiResponse::ok(SampleResult { success: false, log, id: Some(id), port: req.port });
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

#[derive(Debug, Deserialize)]
struct LogQuery {
    lines: Option<usize>,
}

async fn app_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Query(query): Query<LogQuery>,
) -> Json<ApiResponse<String>> {
    let lines = query.lines.unwrap_or(300);
    let data_dir = state.data_dir.clone();
    match tokio::task::spawn_blocking(move || appproc::read_log(&data_dir, id, lines)).await {
        Ok(text) => Json(ApiResponse::ok(text)),
        Err(_) => Json(ApiResponse::err("Failed to read application log")),
    }
}

async fn clear_app_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Json<ApiResponse<()>> {
    match appproc::clear_log(&state.data_dir, id) {
        Ok(()) => Json(ApiResponse::ok_msg("Log cleared")),
        Err(err) => Json(ApiResponse::err(&err)),
    }
}
