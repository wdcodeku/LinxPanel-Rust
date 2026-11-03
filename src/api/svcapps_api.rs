//! App store service programs: install (download, configure, run as a managed
//! app, optional firewall ports and reverse-proxy domain), upgrade in place
//! with rollback, and uninstall. Every long step runs as a background task.

use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

use crate::models::{ApiResponse, CreateSiteRequest};
use crate::services::{appproc, firewall, openresty, shell, svcapps, tasks, updater};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/appstore/services", get(installed))
        .route("/appstore/services/install", post(install))
        .route("/appstore/services/{id}/upgrade", post(upgrade))
        .route("/appstore/services/{id}/uninstall", post(uninstall))
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
struct ServiceRow {
    id: i64,
    app: String,
    version: String,
    dir: String,
    app_id: i64,
    site_id: Option<i64>,
    /// JSON array of ports, in the catalog's order.
    ports: String,
    /// JSON array of `svcapps::InfoItem`.
    info: String,
    created_at: String,
    updated_at: String,
}

#[derive(Serialize)]
struct ServiceView {
    #[serde(flatten)]
    row: ServiceRow,
    name: String,
    running: bool,
    /// The managed app was deleted from the Runtimes page.
    missing: bool,
    started_at: Option<i64>,
    last_error: String,
    domain: Option<String>,
}

async fn load_row(state: &AppState, id: i64) -> Result<ServiceRow, String> {
    sqlx::query_as::<_, ServiceRow>("SELECT * FROM service_installs WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Not installed".to_string())
}

async fn installed(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<ServiceView>>> {
    let rows =
        match sqlx::query_as::<_, ServiceRow>("SELECT * FROM service_installs ORDER BY id DESC")
            .fetch_all(&state.db)
            .await
        {
            Ok(rows) => rows,
            Err(err) => return Json(ApiResponse::err(&err.to_string())),
        };
    let mut views = Vec::with_capacity(rows.len());
    for row in rows {
        let missing = appproc::get(&state.db, row.app_id).await.is_err();
        let app_state = state.supervisor.state_of(row.app_id).await;
        let domain = match row.site_id {
            Some(site_id) => sqlx::query_as::<_, (String,)>("SELECT name FROM sites WHERE id = ?")
                .bind(site_id)
                .fetch_optional(&state.db)
                .await
                .ok()
                .flatten()
                .map(|(name,)| name),
            None => None,
        };
        views.push(ServiceView {
            name: svcapps::find(&row.app)
                .map(|d| d.name)
                .unwrap_or(&row.app)
                .to_string(),
            running: app_state.running,
            missing,
            started_at: app_state.started_at,
            last_error: app_state.last_error,
            domain,
            row,
        });
    }
    Json(ApiResponse::ok(views))
}

#[derive(Debug, Deserialize)]
struct InstallRequest {
    app: String,
    /// Empty: the latest release.
    #[serde(default)]
    version: String,
    #[serde(default)]
    ports: Vec<u16>,
    #[serde(default)]
    public: bool,
    #[serde(default)]
    domain: String,
    #[serde(default)]
    open_firewall: bool,
    #[serde(default)]
    run_user: String,
    #[serde(default)]
    root_dir: String,
    /// Overrides the GitHub download.
    #[serde(default)]
    source_url: String,
    #[serde(default)]
    access_host: String,
    #[serde(default)]
    locale: String,
}

#[derive(Serialize)]
struct InstallResult {
    success: bool,
    log: String,
    id: Option<i64>,
    version: String,
    info: Vec<svcapps::InfoItem>,
}

fn failed(log: String) -> Json<ApiResponse<InstallResult>> {
    Json(ApiResponse::ok(InstallResult {
        success: false,
        log,
        id: None,
        version: String::new(),
        info: Vec::new(),
    }))
}

fn note(log: &mut String, line: &str) {
    tasks::note(line);
    log.push_str(line);
    log.push('\n');
}

async fn install(
    State(state): State<Arc<AppState>>,
    Json(req): Json<InstallRequest>,
) -> Json<ApiResponse<tasks::Started>> {
    let Some(def) = svcapps::find(&req.app) else {
        return Json(ApiResponse::err("Unknown app"));
    };
    let title = format!("Install {}", def.name);
    let task_state = state.clone();
    Json(ApiResponse::ok(state.tasks.start(
        "app-service",
        &title,
        async move { install_task(task_state, req).await.0 },
    )))
}

/// The version to install: the requested one, else the latest release.
async fn resolve_tag(
    def: &'static svcapps::ServiceDef,
    requested: &str,
    prefix: String,
) -> Result<String, String> {
    if !requested.trim().is_empty() {
        return svcapps::normalize_tag(requested);
    }
    tasks::blocking(move || svcapps::latest_tag(&prefix, def.repo))
        .await
        .map_err(|e| e.to_string())?
}

fn download_for(
    def: &svcapps::ServiceDef,
    tag: &str,
    prefix: &str,
    source_url: &str,
) -> Result<String, String> {
    let url = match source_url.trim() {
        "" => {
            let (asset, _) = def.asset(tag.trim_start_matches('v'), shell::has_binary("xz"))?;
            svcapps::download_url(prefix, def.repo, tag, &asset)
        }
        url => url.to_string(),
    };
    crate::services::appstore::validate_url(&url)?;
    Ok(url)
}

async fn validate_ports(
    state: &AppState,
    def: &svcapps::ServiceDef,
    ports: &[u16],
    exclude_app: Option<i64>,
) -> Result<(), String> {
    if ports.len() != def.ports.len() {
        return Err("Port count does not match the app".into());
    }
    let mut seen = std::collections::HashSet::new();
    for (port, spec) in ports.iter().zip(def.ports) {
        if *port == 0 {
            if spec.optional {
                continue;
            }
            return Err("Port is required".into());
        }
        if *port < 1024 {
            return Err(format!("Port {} is privileged; use 1024 or above", port));
        }
        if !seen.insert(*port) {
            return Err(format!("Port {} is used twice", port));
        }
        let taken: Option<(i64,)> = sqlx::query_as("SELECT id FROM apps WHERE port = ? LIMIT 1")
            .bind(*port as i64)
            .fetch_optional(&state.db)
            .await
            .map_err(|e| e.to_string())?;
        if taken.is_some_and(|(id,)| Some(id) != exclude_app) {
            return Err(format!("Port {} belongs to another application", port));
        }
        if !svcapps::port_is_free(*port) {
            return Err(format!("Port {} is already in use", port));
        }
    }
    Ok(())
}

async fn install_task(
    state: Arc<AppState>,
    req: InstallRequest,
) -> Json<ApiResponse<InstallResult>> {
    let Some(def) = svcapps::find(&req.app) else {
        return Json(ApiResponse::err("Unknown app"));
    };
    let mut log = format!("[LinxPanel] Installing {}\n", def.name);

    // --- validate ---
    let existing: Option<(i64,)> = sqlx::query_as("SELECT id FROM service_installs WHERE app = ?")
        .bind(def.key)
        .fetch_optional(&state.db)
        .await
        .unwrap_or(None);
    if existing.is_some() {
        return Json(ApiResponse::err(&format!(
            "{} is already installed",
            def.name
        )));
    }
    let dir = def.dir();
    if let Err(err) = validate_ports(&state, def, &req.ports, None).await {
        return Json(ApiResponse::err(&err));
    }
    let domain = match req.domain.trim() {
        "" => None,
        raw => match crate::api::sites::normalize_domain(raw) {
            Some(domain) => {
                match crate::api::sites::find_conflicting_domain(&state.db, &domain).await {
                    Ok(Some(conflict)) => {
                        return Json(ApiResponse::err(&format!(
                            "Domain already exists: {}",
                            conflict
                        )))
                    }
                    Ok(None) => {}
                    Err(err) => return Json(ApiResponse::err(&err.to_string())),
                }
                let ready = openresty::get_status(&state)
                    .await
                    .map(|s| s.installed)
                    .unwrap_or(false);
                if !ready {
                    return Json(ApiResponse::err(
                        "A domain needs OpenResty: install it on the Sites page, or leave the domain empty",
                    ));
                }
                Some(domain)
            }
            None => return Json(ApiResponse::err("Invalid domain format")),
        },
    };
    let options = svcapps::InstallOptions {
        ports: req.ports.clone(),
        // Without a domain the port is the only way in.
        public: req.public || domain.is_none(),
        domain: domain.clone(),
        access_host: req.access_host.clone(),
        run_user: req.run_user.clone(),
        root_dir: req.root_dir.clone(),
        locale: req.locale.clone(),
    };
    let user = svcapps::run_user(def, &options);
    if def.user_choice && !user.is_empty() && appproc::run_user_ids(&user).is_none() {
        return Json(ApiResponse::err(&format!("No such system user: {}", user)));
    }
    if def.root_dir && !std::path::Path::new(options.root_dir.trim()).is_dir() {
        return Json(ApiResponse::err(&format!(
            "Directory does not exist: {}",
            options.root_dir.trim()
        )));
    }
    let setup = match svcapps::plan(def, &dir, &options) {
        Ok(setup) => setup,
        Err(err) => return Json(ApiResponse::err(&err)),
    };

    // --- version and download ---
    let prefix = updater::load_config(&state.db)
        .await
        .github_prefix()
        .to_string();
    let tag = if req.version.trim().is_empty() && !req.source_url.trim().is_empty() {
        // A custom download without a version: record it as such.
        "custom".to_string()
    } else {
        note(&mut log, "[LinxPanel] Looking up the release");
        match resolve_tag(def, &req.version, prefix.clone()).await {
            Ok(tag) => tag,
            Err(err) => {
                note(&mut log, &format!("[LinxPanel] {}", err));
                return failed(log);
            }
        }
    };
    note(&mut log, &format!("[LinxPanel] Version {}", tag));
    let url = match download_for(def, &tag, &prefix, &req.source_url) {
        Ok(url) => url,
        Err(err) => return Json(ApiResponse::err(&err)),
    };

    // Data kept by an earlier uninstall: set aside (never deleted) for a clean install.
    if dir.exists() && !crate::services::appstore::dir_is_empty(&dir) {
        let aside = PathBuf::from(format!(
            "{}.old-{}",
            dir.display(),
            chrono::Local::now().format("%Y%m%d-%H%M%S")
        ));
        if let Err(err) = std::fs::rename(&dir, &aside) {
            return Json(ApiResponse::err(&format!("{}: {}", dir.display(), err)));
        }
        note(
            &mut log,
            &format!("[LinxPanel] Kept the previous data as {}", aside.display()),
        );
    }

    // --- files ---
    let work = state
        .data_dir
        .join("tmp")
        .join(format!("svc-{}", uuid::Uuid::new_v4()));
    let prepared = {
        let (dir, work, setup, user) = (dir.clone(), work.clone(), setup.clone(), user.clone());
        tasks::blocking(move || {
            let mut step_log = String::new();
            let result = svcapps::prepare(def, &dir, &url, &work, &setup, &user, &mut step_log);
            (step_log, result)
        })
        .await
    };
    let _ = std::fs::remove_dir_all(&work);
    let prepare_error = match prepared {
        Ok((text, result)) => {
            log.push_str(&text);
            result.err()
        }
        Err(err) => Some(err.to_string()),
    };
    if let Some(err) = prepare_error {
        note(&mut log, &format!("[LinxPanel] Failed: {}", err));
        // The directory was empty or missing before; leave nothing behind.
        let _ = std::fs::remove_dir_all(&dir);
        return failed(log);
    }

    // --- managed app ---
    note(&mut log, "[LinxPanel] Registering the service");
    let request = appproc::SaveAppRequest {
        name: def.name.to_string(),
        runtime: "binary".into(),
        work_dir: dir.to_string_lossy().to_string(),
        entry: def.binary_path(&dir).to_string_lossy().to_string(),
        args: setup.args.clone(),
        env: String::new(),
        port: req.ports[0] as i64,
        autostart: true,
        run_user: user.clone(),
        chown_work_dir: false,
    };
    let app_id = match appproc::create(&state.db, &request).await {
        Ok(id) => id,
        Err(err) => {
            note(&mut log, &format!("[LinxPanel] Failed: {}", err));
            let _ = std::fs::remove_dir_all(&dir);
            return failed(log);
        }
    };
    let rollback = |log: &mut String, reason: String| {
        let state = state.clone();
        let dir = dir.clone();
        note(
            log,
            &format!("[LinxPanel] Failed: {}\n[LinxPanel] Rolling back", reason),
        );
        async move {
            let _ = appproc::stop(state.supervisor.clone(), app_id).await;
            let _ = appproc::delete(&state.db, app_id).await;
            let _ = std::fs::remove_dir_all(&dir);
        }
    };
    if let Err(err) = start_and_wait(&state, app_id, req.ports[0]).await {
        let tail = appproc::read_log(&state.data_dir, app_id, 30);
        if !tail.trim().is_empty() {
            note(&mut log, "[LinxPanel] Service output:");
            note(&mut log, tail.trim_end());
        }
        rollback(&mut log, err).await;
        return failed(log);
    }
    note(
        &mut log,
        &format!(
            "[LinxPanel] {} is running on port {}",
            def.name, req.ports[0]
        ),
    );

    // --- firewall ---
    if req.open_firewall {
        let to_open: Vec<u16> = req
            .ports
            .iter()
            .zip(def.ports)
            .filter(|(port, spec)| **port > 0 && (spec.external || options.public))
            .map(|(port, _)| *port)
            .collect();
        let opened = tasks::blocking(move || open_ports(&to_open))
            .await
            .unwrap_or_default();
        log.push_str(&opened);
    }

    // --- site ---
    let mut site_id = None;
    if let Some(domain) = &domain {
        note(&mut log, &format!("[LinxPanel] Creating site {}", domain));
        match crate::api::sites::create_site_record(
            &state,
            CreateSiteRequest {
                name: domain.clone(),
                domains: vec![domain.clone()],
                doc_root: None,
                site_type: Some("binary".into()),
                proxy_pass: None,
                index_files: None,
                stopped_page_html: None,
                app_id: Some(app_id),
                rewrite_rules: None,
            },
        )
        .await
        {
            Ok(site) => site_id = Some(site.id),
            Err(err) => {
                rollback(&mut log, format!("site: {}", err)).await;
                return failed(log);
            }
        }
    }

    let info = serde_json::to_string(&setup.info).unwrap_or_else(|_| "[]".into());
    let ports = serde_json::to_string(&req.ports).unwrap_or_else(|_| "[]".into());
    let inserted = sqlx::query(
        "INSERT INTO service_installs (app, version, dir, app_id, site_id, ports, info)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(def.key)
    .bind(&tag)
    .bind(dir.to_string_lossy().to_string())
    .bind(app_id)
    .bind(site_id)
    .bind(&ports)
    .bind(&info)
    .execute(&state.db)
    .await;
    let id = match inserted {
        Ok(result) => result.last_insert_rowid(),
        Err(err) => return Json(ApiResponse::err(&err.to_string())),
    };
    note(
        &mut log,
        &format!(
            "\n[LinxPanel] Done. {} {} is installed in {}",
            def.name,
            tag,
            dir.display()
        ),
    );
    Json(ApiResponse::ok(InstallResult {
        success: true,
        log,
        id: Some(id),
        version: tag,
        info: setup.info,
    }))
}

/// Starts the managed app and waits until it accepts connections. A crash
/// (the supervisor restarting it) fails at once instead of after the timeout.
async fn start_and_wait(state: &Arc<AppState>, app_id: i64, port: u16) -> Result<(), String> {
    appproc::start(
        state.supervisor.clone(),
        state.db.clone(),
        state.data_dir.clone(),
        app_id,
    )
    .await?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if svcapps::port_open(port).await {
            return Ok(());
        }
        let app_state = state.supervisor.state_of(app_id).await;
        if app_state.restarts > 0 || (!app_state.running && !app_state.last_error.is_empty()) {
            return Err(format!("the service exited: {}", app_state.last_error));
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!(
                "the service did not open port {} within 30 seconds",
                port
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

/// Opens TCP ports when a firewall is active. Never fails the install.
fn open_ports(ports: &[u16]) -> String {
    let mut log = String::new();
    let status = firewall::status();
    if !status.available || !status.enabled {
        log.push_str("[LinxPanel] No active firewall; ports need no opening here (check your cloud security group)\n");
        tasks::note(log.trim_end());
        return log;
    }
    for port in ports {
        let request = firewall::FirewallRuleRequest {
            port: port.to_string(),
            protocol: "tcp".into(),
            source: String::new(),
            action: "allow".into(),
        };
        let line = match firewall::add_rule(&request) {
            Ok(_) => format!("[LinxPanel] Firewall: opened {}/tcp", port),
            Err(err) => format!("[LinxPanel] Firewall: could not open {}/tcp: {}", port, err),
        };
        tasks::note(&line);
        log.push_str(&line);
        log.push('\n');
    }
    log
}

#[derive(Debug, Deserialize, Default)]
struct UpgradeRequest {
    #[serde(default)]
    version: String,
    #[serde(default)]
    source_url: String,
}

async fn upgrade(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    body: Option<Json<UpgradeRequest>>,
) -> Json<ApiResponse<tasks::Started>> {
    let req = body.map(|Json(b)| b).unwrap_or_default();
    let row = match load_row(&state, id).await {
        Ok(row) => row,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    let name = svcapps::find(&row.app).map(|d| d.name).unwrap_or("app");
    let title = format!("Upgrade {}", name);
    let task_state = state.clone();
    Json(ApiResponse::ok(state.tasks.start(
        "app-service",
        &title,
        async move { upgrade_task(task_state, row, req).await.0 },
    )))
}

async fn upgrade_task(
    state: Arc<AppState>,
    row: ServiceRow,
    req: UpgradeRequest,
) -> Json<ApiResponse<InstallResult>> {
    let Some(def) = svcapps::find(&row.app) else {
        return Json(ApiResponse::err("Unknown app"));
    };
    let mut log = String::new();
    let prefix = updater::load_config(&state.db)
        .await
        .github_prefix()
        .to_string();
    let tag = if req.version.trim().is_empty() && !req.source_url.trim().is_empty() {
        "custom".to_string()
    } else {
        note(&mut log, "[LinxPanel] Looking up the release");
        match resolve_tag(def, &req.version, prefix.clone()).await {
            Ok(tag) => tag,
            Err(err) => {
                note(&mut log, &format!("[LinxPanel] {}", err));
                return failed(log);
            }
        }
    };
    if tag == row.version && req.source_url.trim().is_empty() {
        note(
            &mut log,
            &format!(
                "[LinxPanel] {} {} is already the latest version",
                def.name, tag
            ),
        );
        return Json(ApiResponse::ok(InstallResult {
            success: true,
            log,
            id: Some(row.id),
            version: tag,
            info: Vec::new(),
        }));
    }
    note(
        &mut log,
        &format!(
            "[LinxPanel] Upgrading {} {} -> {}",
            def.name, row.version, tag
        ),
    );
    let url = match download_for(def, &tag, &prefix, &req.source_url) {
        Ok(url) => url,
        Err(err) => return Json(ApiResponse::err(&err)),
    };

    let dir = PathBuf::from(&row.dir);
    let binary = def.binary_path(&dir);
    let fresh = dir.join(format!(".{}.new", def.key));
    let backup = dir.join(format!(".{}.old", def.key));
    let work = state
        .data_dir
        .join("tmp")
        .join(format!("svc-{}", uuid::Uuid::new_v4()));
    let fetched = {
        let (work, fresh) = (work.clone(), fresh.clone());
        tasks::blocking(move || {
            let mut step_log = String::new();
            let result = svcapps::fetch_binary(def, &url, &work, &fresh, &mut step_log);
            (step_log, result)
        })
        .await
    };
    let _ = std::fs::remove_dir_all(&work);
    match fetched {
        Ok((text, Ok(()))) => log.push_str(&text),
        Ok((text, Err(err))) => {
            log.push_str(&text);
            note(&mut log, &format!("[LinxPanel] Failed: {}", err));
            let _ = std::fs::remove_file(&fresh);
            return failed(log);
        }
        Err(err) => return Json(ApiResponse::err(&err.to_string())),
    }

    // Swap the binary while the service is stopped; put the old one back if
    // the new one does not come up.
    let port = serde_json::from_str::<Vec<u16>>(&row.ports)
        .ok()
        .and_then(|p| p.first().copied())
        .unwrap_or(0);
    let record = appproc::get(&state.db, row.app_id).await.ok();
    note(&mut log, "[LinxPanel] Stopping the service");
    let _ = appproc::stop(state.supervisor.clone(), row.app_id).await;
    let swap = std::fs::rename(&binary, &backup).and_then(|_| std::fs::rename(&fresh, &binary));
    if let Err(err) = swap {
        let _ = std::fs::rename(&backup, &binary);
        let _ = start_and_wait(&state, row.app_id, port).await;
        note(
            &mut log,
            &format!("[LinxPanel] Failed to replace the program: {}", err),
        );
        return failed(log);
    }
    if let Some(user) = record
        .as_ref()
        .map(|r| r.run_user.clone())
        .filter(|u| !u.is_empty())
    {
        appproc::chown_to(&user, &binary);
    }
    note(&mut log, "[LinxPanel] Starting the new version");
    if let Err(err) = start_and_wait(&state, row.app_id, port).await {
        note(
            &mut log,
            &format!(
                "[LinxPanel] The new version did not start ({}); restoring {}",
                err, row.version
            ),
        );
        let tail = appproc::read_log(&state.data_dir, row.app_id, 20);
        if !tail.trim().is_empty() {
            note(&mut log, tail.trim_end());
        }
        let _ = appproc::stop(state.supervisor.clone(), row.app_id).await;
        let _ = std::fs::rename(&backup, &binary);
        let restored = start_and_wait(&state, row.app_id, port).await;
        note(
            &mut log,
            &match restored {
                Ok(()) => format!("[LinxPanel] {} {} is running again", def.name, row.version),
                Err(err) => format!("[LinxPanel] The old version did not start either: {}", err),
            },
        );
        return failed(log);
    }
    let _ = std::fs::remove_file(&backup);
    let _ = sqlx::query(
        "UPDATE service_installs SET version = ?, updated_at = datetime('now') WHERE id = ?",
    )
    .bind(&tag)
    .bind(row.id)
    .execute(&state.db)
    .await;
    note(
        &mut log,
        &format!("\n[LinxPanel] Done. {} is now {}", def.name, tag),
    );
    Json(ApiResponse::ok(InstallResult {
        success: true,
        log,
        id: Some(row.id),
        version: tag,
        info: Vec::new(),
    }))
}

#[derive(Debug, Deserialize, Default)]
struct UninstallRequest {
    /// Also delete the install directory (configuration and data).
    #[serde(default)]
    delete_data: bool,
}

async fn uninstall(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    body: Option<Json<UninstallRequest>>,
) -> Json<ApiResponse<()>> {
    let req = body.map(|Json(b)| b).unwrap_or_default();
    let row = match load_row(&state, id).await {
        Ok(row) => row,
        Err(err) => return Json(ApiResponse::err(&err)),
    };
    let _ = appproc::stop(state.supervisor.clone(), row.app_id).await;
    let _ = appproc::delete(&state.db, row.app_id).await;
    if let Some(site_id) = row.site_id {
        let _ = sqlx::query("DELETE FROM domains WHERE site_id = ?")
            .bind(site_id)
            .execute(&state.db)
            .await;
        let deleted = sqlx::query("DELETE FROM sites WHERE id = ?")
            .bind(site_id)
            .execute(&state.db)
            .await
            .map(|r| r.rows_affected() > 0)
            .unwrap_or(false);
        if deleted {
            if let Err(err) = openresty::remove_site(&state, site_id).await {
                tracing::warn!("Removing site {} of {}: {}", site_id, row.app, err);
            }
        }
    }
    if req.delete_data {
        let dir = PathBuf::from(&row.dir);
        // Only ever an app directory under the apps root.
        if dir.starts_with(svcapps::APPS_ROOT) && dir != std::path::Path::new(svcapps::APPS_ROOT) {
            if let Err(err) = tokio::fs::remove_dir_all(&dir).await {
                if err.kind() != std::io::ErrorKind::NotFound {
                    return Json(ApiResponse::err(&format!("{}: {}", dir.display(), err)));
                }
            }
        }
    }
    match sqlx::query("DELETE FROM service_installs WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await
    {
        Ok(_) => Json(ApiResponse::ok_msg("Uninstalled")),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}
