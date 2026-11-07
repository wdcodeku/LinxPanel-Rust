pub mod account_api;
pub mod appstore_api;
pub mod auth;
pub mod backup_api;
pub mod cert_api;
pub mod cron_api;
pub mod database_api;
pub mod defense_api;
pub mod domains;
pub mod files;
pub mod files_dl;
pub mod files_ops;
pub mod firewall_api;
pub mod notify_api;
pub mod panel_api;
pub mod processes;
pub mod realtime;
pub mod recycle_api;
pub mod runtime_api;
pub mod services_api;
pub mod settings;
pub mod sites;
pub mod svcapps_api;
pub mod system;
pub mod tasks_api;
pub mod update_api;

use crate::AppState;
use axum::{middleware as axum_mw, Router};
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    let public = Router::new()
        .merge(auth::public_routes())
        .merge(panel_api::public_routes())
        .merge(files_dl::public_routes());

    let protected = Router::new()
        .merge(system::routes())
        .merge(realtime::routes())
        .merge(processes::routes())
        .merge(runtime_api::routes())
        .merge(services_api::routes())
        .merge(cron_api::routes())
        .merge(cert_api::routes())
        .merge(firewall_api::routes())
        .merge(sites::routes())
        .merge(domains::routes())
        .merge(files::routes())
        .merge(files_ops::routes())
        .merge(files_dl::routes())
        .merge(recycle_api::routes())
        .merge(notify_api::routes())
        .merge(settings::routes())
        .merge(panel_api::routes())
        .merge(account_api::routes())
        .merge(database_api::routes())
        .merge(appstore_api::routes())
        .merge(svcapps_api::routes())
        .merge(backup_api::routes())
        .merge(defense_api::routes())
        .merge(update_api::routes())
        .merge(tasks_api::routes())
        // Inside auth: records who did what once the caller is known.
        .route_layer(axum_mw::from_fn(crate::services::audit::middleware))
        .route_layer(axum_mw::from_fn(crate::middleware::auth))
        .layer(axum::Extension(state.clone()));

    Router::new()
        .merge(public)
        .merge(protected)
        .with_state(state)
}
