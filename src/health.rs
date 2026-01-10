use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::handler::AppState;

/// Basic health check endpoint
/// Returns 200 OK if the server is running
pub async fn health() -> Response {
    (StatusCode::OK, "OK").into_response()
}

/// Liveness probe endpoint
/// Returns 200 OK if the server process is alive
pub async fn liveness() -> Response {
    (StatusCode::OK, "Alive").into_response()
}

/// Readiness probe endpoint
/// Returns 200 OK if the server is ready to accept requests
/// Checks that config is loaded and routes are available
pub async fn readiness(
    State(state): State<Arc<RwLock<AppState>>>,
) -> Response {
    let state_guard = state.read().await;
    
    // Check if we have at least one route configured
    if state_guard.config.routes.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "No routes configured",
        )
            .into_response();
    }

    (StatusCode::OK, "Ready").into_response()
}
