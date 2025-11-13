use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::Value;
use std::sync::Arc;

use crate::config::{Config, GlobalConfig};
use crate::proxy::forward_request;
use crate::schema::SchemaCache;
use crate::validation::validate;

pub struct AppState {
    pub config: Config,
    pub schema_cache: SchemaCache,
    pub http_client: reqwest::Client,
}

struct RequestContext {
    method: Method,
    upstream_url: String,
    path: String,
    headers: HeaderMap,
    body_bytes: Vec<u8>,
}

/// Main request handler for the gateway
pub async fn handle_request(
    State(state): State<Arc<tokio::sync::RwLock<AppState>>>,
    method: Method,
    uri: axum::http::Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let path = uri.path();

    // Read body
    let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes.to_vec(),
        Err(_) => {
            return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
        }
    };

    // Lock state for reading
    let state_guard = state.read().await;

    // Find matching route
    let route = match state_guard.config.find_route(path, method.as_str()) {
        Some(r) => r,
        None => {
            tracing::debug!(method = %method, path = %path, "Route not found");
            drop(state_guard);
            return (StatusCode::NOT_FOUND, "Route not found").into_response();
        }
    };

    // Get effective config for this route
    let effective_config = state_guard.config.get_effective_config(route);
    let upstream_url = route.upstream.clone();
    let schema_path = route.schema.clone();

    drop(state_guard);

    // If no schema configured, forward without validation
    let schema_path = match schema_path {
        Some(path) => path,
        None => {
            // No schema, just forward
            let state_guard = state.read().await;
            let response = forward_request(
                &state_guard.http_client,
                method,
                &upstream_url,
                path,
                headers,
                body_bytes,
            )
            .await;
            drop(state_guard);
            return response;
        }
    };

    // Parse JSON body if present
    let json_value = if body_bytes.is_empty() {
        // Empty body, skip validation and forward
        let state_guard = state.read().await;
        let response = forward_request(
            &state_guard.http_client,
            method,
            &upstream_url,
            path,
            headers,
            body_bytes,
        )
        .await;
        drop(state_guard);
        return response;
    } else {
        match serde_json::from_slice::<Value>(&body_bytes) {
            Ok(v) => v,
            Err(e) => {
                // Invalid JSON
                let error_msg = format!("Invalid JSON: {}", e);
                tracing::warn!(
                    method = %method,
                    path = %path,
                    error = %e,
                    "Failed to parse JSON body"
                );
                let ctx = RequestContext {
                    method,
                    upstream_url,
                    path: path.to_string(),
                    headers,
                    body_bytes,
                };
                return handle_error(
                    &error_msg,
                    &effective_config,
                    ctx,
                    state,
                    StatusCode::BAD_REQUEST,
                )
                .await;
            }
        }
    };

    // Load schema
    let mut state_guard = state.write().await;
    let schema_result = state_guard.schema_cache.load(&schema_path);
    drop(state_guard);

    let schema = match schema_result {
        Ok(s) => s,
        Err(e) => {
            // Schema loading error
            let error_msg = format!("{}", e);
            tracing::warn!(
                method = %method,
                path = %path,
                schema_path = %schema_path.display(),
                error = %e,
                "Failed to load schema"
            );
            let ctx = RequestContext {
                method,
                upstream_url,
                path: path.to_string(),
                headers,
                body_bytes,
            };
            return handle_error(
                &error_msg,
                &effective_config,
                ctx,
                state,
                StatusCode::INTERNAL_SERVER_ERROR,
            )
            .await;
        }
    };

    // Validate
    let validation_result = validate(&schema, &json_value);

    if validation_result.valid {
        // Validation passed, forward with validation header
        tracing::debug!(
            method = %method,
            path = %path,
            upstream = %upstream_url,
            "Request validated successfully"
        );
        let state_guard = state.read().await;
        let mut response = forward_request(
            &state_guard.http_client,
            method,
            &upstream_url,
            path,
            headers,
            body_bytes,
        )
        .await;

        // Add validation header if configured
        if effective_config.add_validation_header {
            if let Ok(header_value) = "true".parse() {
                response
                    .headers_mut()
                    .insert("X-Schema-Validated", header_value);
            }
        }

        drop(state_guard);
        response
    } else {
        // Validation failed
        let error_msg = format!("Validation failed: {}", validation_result.errors.join(", "));
        tracing::warn!(
            method = %method,
            path = %path,
            errors = ?validation_result.errors,
            "Validation failed"
        );
        let ctx = RequestContext {
            method,
            upstream_url,
            path: path.to_string(),
            headers,
            body_bytes,
        };
        handle_error(
            &error_msg,
            &effective_config,
            ctx,
            state,
            StatusCode::BAD_REQUEST,
        )
        .await
    }
}

/// Handle errors according to forward_on_error configuration
async fn handle_error(
    error_msg: &str,
    effective_config: &GlobalConfig,
    ctx: RequestContext,
    state: Arc<tokio::sync::RwLock<AppState>>,
    error_status: StatusCode,
) -> Response {
    if effective_config.forward_on_error {
        // Forward to upstream with error header
        tracing::warn!(
            method = %ctx.method,
            path = %ctx.path,
            upstream = %ctx.upstream_url,
            error = %error_msg,
            "Forwarding request to upstream despite error (forward_on_error: true)"
        );
        let state_guard = state.read().await;
        let mut response = forward_request(
            &state_guard.http_client,
            ctx.method,
            &ctx.upstream_url,
            &ctx.path,
            ctx.headers,
            ctx.body_bytes,
        )
        .await;

        // Add error header if configured
        if effective_config.add_error_header {
            if let Ok(header_value) = error_msg.parse() {
                response
                    .headers_mut()
                    .insert("X-Gateway-Error", header_value);
            }
        }

        drop(state_guard);
        response
    } else {
        // Return error response without forwarding
        tracing::warn!(
            method = %ctx.method,
            path = %ctx.path,
            error = %error_msg,
            status = %error_status,
            "Rejecting request due to error (forward_on_error: false)"
        );
        let error_body = serde_json::json!({
            "error": error_msg
        });
        let body_str = serde_json::to_string(&error_body)
            .unwrap_or_else(|_| format!(r#"{{"error":"{}"}}"#, error_msg));
        (error_status, body_str).into_response()
    }
}
