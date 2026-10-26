//! One-click deployment of website programs: checks prerequisites, creates the
//! database, unpacks and configures the program, then creates the site. Any
//! failure rolls back what was created.

use axum::{extract::State, routing::get, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

use crate::models::{ApiResponse, CreateSiteRequest};
use crate::services::{appstore, database, openresty, php};
use crate::AppState;

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/appstore/apps", get(apps))
        .route("/appstore/deploy", post(deploy))
        .route("/appstore/installed", get(installed))
}

#[derive(Serialize)]
struct Prerequisites {
    openresty: bool,
    php: bool,
    mysql: bool,
}

#[derive(Serialize)]
struct Catalog {
    apps: &'static [appstore::AppDef],
    prerequisites: Prerequisites,
}

async fn prerequisites(state: &AppState) -> Prerequisites {
    let admin = database::load_mysql_admin(&state.db).await;
    let openresty = openresty::get_status(state)
        .await
        .map(|status| status.installed)
        .unwrap_or(false);
    let (php, mysql) = tokio::task::spawn_blocking(move || {
        (
            php::detect().is_some(),
            database::status(database::Engine::Mysql, admin.as_ref()).manageable,
        )
    })
    .await
    .unwrap_or((false, false));
    Prerequisites {
        openresty,
        php,
        mysql,
    }
}

async fn apps(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Catalog>> {
    Json(ApiResponse::ok(Catalog {
        apps: appstore::CATALOG,
        prerequisites: prerequisites(&state).await,
    }))
}

#[derive(Deserialize)]
struct DeployRequest {
    app: String,
    domains: Vec<String>,
    /// Defaults to /www/wwwroot/<first domain>.
    #[serde(default)]
    doc_root: String,
    /// Database name / user; derived from the domain when empty.
    #[serde(default)]
    db_name: String,
    #[serde(default)]
    db_password: String,
    /// Overrides the official download (e.g. a mirror where GitHub is slow).
    #[serde(default)]
    source_url: String,
    /// "zh" picks localized packages where available.
    #[serde(default)]
    locale: String,
}

#[derive(Serialize)]
struct DeployResult {
    success: bool,
    log: String,
    site_id: Option<i64>,
    url: String,
    database: Option<database::CreatedDatabase>,
}

fn fail(log: String) -> Json<ApiResponse<DeployResult>> {
    Json(ApiResponse::ok(DeployResult {
        success: false,
        log,
        site_id: None,
        url: String::new(),
        database: None,
    }))
}

async fn deploy(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DeployRequest>,
) -> Json<ApiResponse<DeployResult>> {
    let Some(app) = appstore::find(&req.app) else {
        return Json(ApiResponse::err("Unknown app"));
    };
    let mut log = format!("[LinxPanel] Deploying {}\n", app.name);

    // --- validate ---
    let domains: Vec<String> = req
        .domains
        .iter()
        .filter_map(|d| crate::api::sites::normalize_domain(d))
        .collect();
    let Some(primary) = domains.first().cloned() else {
        return Json(ApiResponse::err("At least one domain is required"));
    };
    for domain in &domains {
        match crate::api::sites::find_conflicting_domain(&state.db, domain).await {
            Ok(Some(conflict)) => {
                return Json(ApiResponse::err(&format!(
                    "Domain already exists: {}",
                    conflict
                )))
            }
            Ok(None) => {}
            Err(err) => return Json(ApiResponse::err(&err.to_string())),
        }
    }
    let doc_root = match req.doc_root.trim() {
        "" => PathBuf::from("/www/wwwroot").join(&primary),
        path => PathBuf::from(path),
    };
    if !doc_root.is_absolute() {
        return Json(ApiResponse::err("Site directory must be an absolute path"));
    }
    if !appstore::dir_is_empty(&doc_root) {
        return Json(ApiResponse::err(&format!(
            "{} is not empty; choose an empty or new directory",
            doc_root.display()
        )));
    }
    let url = match req.source_url.trim() {
        "" => app.default_url(&req.locale).to_string(),
        url => url.to_string(),
    };
    if let Err(err) = appstore::validate_url(&url) {
        return Json(ApiResponse::err(&err));
    }

    // --- prerequisites ---
    let pre = prerequisites(&state).await;
    let mut missing = Vec::new();
    if !pre.openresty {
        missing.push("OpenResty (Sites)");
    }
    if !pre.php {
        missing.push("PHP (Runtimes)");
    }
    if app.needs_mysql && !pre.mysql {
        missing.push("MySQL (Databases)");
    }
    if !missing.is_empty() {
        return Json(ApiResponse::err(&format!(
            "Install these first: {}",
            missing.join(", ")
        )));
    }
    // FPM must run and its socket must be reachable by nginx before the site goes live.
    let pool = match tokio::task::spawn_blocking(php::prepare).await {
        Ok(Ok((pool, text))) => {
            log.push_str(&text);
            pool
        }
        Ok(Err(err)) => return Json(ApiResponse::err(&err)),
        Err(err) => return Json(ApiResponse::err(&err.to_string())),
    };

    // --- database ---
    let mut created_db = None;
    if app.needs_mysql {
        let name = match req.db_name.trim() {
            "" => appstore::db_name_for(&primary),
            name => name.to_string(),
        };
        log.push_str(&format!("[LinxPanel] Creating MySQL database {}\n", name));
        let request = database::CreateDatabaseRequest {
            engine: "mysql".into(),
            name: name.clone(),
            username: name,
            password: req.db_password.trim().to_string(),
            host: "localhost".into(),
            note: format!("{} · {}", app.name, primary),
        };
        match database::create(
            &state.db,
            request,
            database::load_mysql_admin(&state.db).await,
        )
        .await
        {
            Ok(db) => created_db = Some(db),
            Err(err) => {
                log.push_str(&format!("[LinxPanel] {}\n", err));
                return fail(log);
            }
        }
    }

    // --- files ---
    let work = state
        .data_dir
        .join("tmp")
        .join(format!("deploy-{}", uuid::Uuid::new_v4()));
    let files_result = {
        let (app, url, doc_root, work_dir, db, user) = (
            app,
            url.clone(),
            doc_root.clone(),
            work.clone(),
            created_db.clone(),
            pool.user.clone(),
        );
        let result = tokio::task::spawn_blocking(move || {
            let mut step_log = String::new();
            let outcome = (|| {
                std::fs::create_dir_all(&work_dir).map_err(|e| e.to_string())?;
                let archive = work_dir.join("package");
                step_log.push_str(&format!("[LinxPanel] Downloading {}\n", url));
                appstore::download(&url, &archive, &mut step_log)?;
                step_log.push_str(&format!(
                    "[LinxPanel] Unpacking to {}\n",
                    doc_root.display()
                ));
                appstore::unpack(app, &archive, &doc_root, &work_dir, &mut step_log)?;
                if app.key == "wordpress" {
                    let db = db.as_ref().ok_or("database missing")?;
                    appstore::configure_wordpress(&doc_root, &db.name, &db.username, &db.password)?;
                    step_log.push_str("[LinxPanel] Wrote wp-config.php\n");
                }
                appstore::set_owner(&doc_root, &user, &mut step_log)?;
                Ok::<_, String>(())
            })();
            // Keep the step log on failure too: it shows which command broke.
            (step_log, outcome)
        })
        .await;
        let _ = std::fs::remove_dir_all(&work);
        result
    };
    let files_error = match files_result {
        Ok((text, outcome)) => {
            log.push_str(&text);
            outcome.err()
        }
        Err(err) => Some(err.to_string()),
    };

    // --- site ---
    let site = match files_error {
        Some(err) => Err(err),
        None => {
            log.push_str("[LinxPanel] Creating the site\n");
            crate::api::sites::create_site_record(
                &state,
                CreateSiteRequest {
                    name: primary.clone(),
                    domains: domains.clone(),
                    doc_root: Some(doc_root.to_string_lossy().to_string()),
                    site_type: Some("php".into()),
                    proxy_pass: None,
                    index_files: Some("index.php index.html".into()),
                    stopped_page_html: None,
                    app_id: None,
                    rewrite_rules: None,
                },
            )
            .await
        }
    };

    let site = match site {
        Ok(site) => site,
        Err(err) => {
            log.push_str(&format!(
                "[LinxPanel] Failed: {}\n[LinxPanel] Rolling back\n",
                err
            ));
            if let Some(db) = &created_db {
                let admin = database::load_mysql_admin(&state.db).await;
                let _ = database::delete(&state.db, db.engine, &db.name, admin).await;
            }
            // The directory was empty (or new) before; leave it that way.
            if let Ok(entries) = std::fs::read_dir(&doc_root) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let _ = if path.is_dir() {
                        std::fs::remove_dir_all(&path)
                    } else {
                        std::fs::remove_file(&path)
                    };
                }
            }
            return fail(log);
        }
    };

    let _ = sqlx::query(
        "INSERT INTO app_installs (app, site_id, domain, doc_root, db_name) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(app.key)
    .bind(site.id)
    .bind(&primary)
    .bind(doc_root.to_string_lossy().to_string())
    .bind(created_db.as_ref().map(|db| db.name.clone()))
    .execute(&state.db)
    .await;

    let url = format!("http://{}/", primary);
    log.push_str(&format!(
        "\n[LinxPanel] Done. Open {} to finish setup\n",
        url
    ));
    Json(ApiResponse::ok(DeployResult {
        success: true,
        log,
        site_id: Some(site.id),
        url,
        database: created_db,
    }))
}

#[derive(Serialize, sqlx::FromRow)]
struct InstalledApp {
    id: i64,
    app: String,
    site_id: i64,
    domain: String,
    doc_root: String,
    db_name: Option<String>,
    created_at: String,
}

async fn installed(State(state): State<Arc<AppState>>) -> Json<ApiResponse<Vec<InstalledApp>>> {
    // Only installs whose site still exists.
    match sqlx::query_as::<_, InstalledApp>(
        "SELECT a.id, a.app, a.site_id, a.domain, a.doc_root, a.db_name, a.created_at
         FROM app_installs a JOIN sites s ON s.id = a.site_id ORDER BY a.id DESC",
    )
    .fetch_all(&state.db)
    .await
    {
        Ok(rows) => Json(ApiResponse::ok(rows)),
        Err(err) => Json(ApiResponse::err(&err.to_string())),
    }
}
