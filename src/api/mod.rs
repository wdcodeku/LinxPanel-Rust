pub mod auth;
pub mod sites;
pub mod domains;
pub mod files;
pub mod settings;
pub mod system;

use std::sync::Arc;
use axum::{Router, middleware as axum_mw};
use crate::AppState;

pub fn routes(state: Arc<AppState>) -> Router {
    let public = Router::new()
        .merge(auth::public_routes());

    let protected = Router::new()
        .merge(system::routes())
        .merge(sites::routes())
        .merge(domains::routes())
        .merge(files::routes())
        .merge(settings::routes())
        .route_layer(axum_mw::from_fn(crate::middleware::auth))
        .layer(axum::Extension(state.clone()));

    Router::new()
        .merge(public)
        .merge(protected)
        .with_state(state)
}