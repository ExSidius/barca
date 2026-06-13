//! Router wiring. The router is the single API boundary — middleware (CORS,
//! auth) and a static-file fallback for a future UI would layer in here.

use crate::handlers;
use crate::state::AppState;
use axum::Router;
use axum::routing::{get, post};

/// Build the v1 API router over the given state.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        .route("/plan", get(handlers::plan))
        .route("/assets", get(handlers::assets))
        .route("/assets/{name}", get(handlers::asset_detail))
        .route("/run", post(handlers::run))
        .route("/run/{target}", post(handlers::run_target))
        .route("/get/{target}", post(handlers::get_target))
        .route("/status/{run_id}", get(handlers::status))
        .route("/events/{run_id}", get(handlers::events))
        .route("/logs/{run_id}", get(handlers::logs))
        .with_state(state)
}
