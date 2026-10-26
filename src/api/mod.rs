pub mod account_api;
pub mod appstore_api;
pub mod auth;
pub mod backup_api;
pub mod cert_api;
pub mod cron_api;
pub mod database_api;
pub mod domains;
pub mod files;
pub mod firewall_api;
pub mod panel_api;
pub mod processes;
pub mod realtime;
pub mod runtime_api;
pub mod services_api;
pub mod settings;
pub mod sites;
pub mod system;

use crate::AppState;
use axum::{middleware as axum_mw, Router};
use std::sync::Arc;

pub fn routes(state: Arc<AppState>) -> Router {
    let public = Router::new()
        .merge(auth::public_routes())
        .merge(panel_api::public_routes());

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
        .merge(settings::routes())
        .merge(panel_api::routes())
        .merge(account_api::routes())
        .merge(database_api::routes())
        .merge(appstore_api::routes())
        .merge(backup_api::routes())
        .route_layer(axum_mw::from_fn(crate::middleware::auth))
        .layer(axum::Extension(state.clone()));

    Router::new()
        .merge(public)
        .merge(protected)
        .with_state(state)
}
