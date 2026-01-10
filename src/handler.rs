use axum::body::Body;
use axum::extract::State;
use axum::http::{header::CONTENT_TYPE, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use jsonschema::JSONSchema;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use url::form_urlencoded;

use crate::config::{Config, GlobalConfig, OpenApiOptions};
use crate::metrics::Metrics;
use crate::openapi::{OpenApiCache, OperationValidationPlan, ParameterLocation, ResponseKey};
use crate::proxy::forward_request;
use crate::schema::SchemaCache;
use crate::validation::validate;

pub struct AppState {
    pub config: Config,
    pub schema_cache: SchemaCache,
    pub openapi_cache: OpenApiCache,
    pub http_client: reqwest::Client,
    pub metrics: Arc<Metrics>,
}

/// Build a reqwest client suitable for the gateway.
/// We disable system proxy lookups to avoid platform-specific panics in tests.
pub fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("Failed to build HTTP client")
}

#[derive(Clone)]
struct RequestContext {
    method: Method,
    upstream_url: String,
    path: String,
    path_and_query: String,
    query: Option<String>,
    headers: HeaderMap,
    body_bytes: Vec<u8>,
    route_pattern: String,
}

/// Main request handler for the gateway
pub async fn handle_request(
    State(state): State<Arc<tokio::sync::RwLock<AppState>>>,
    method: Method,
    uri: axum::http::Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let start_time = Instant::now();
    let path = uri.path().to_string();
    let path_with_query = build_forward_path(&path, uri.query());
    let method_str = method.as_str().to_uppercase();

    // Read body
    let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes.to_vec(),
        Err(_) => {
            let status = StatusCode::BAD_REQUEST;
            let route_label = "unknown";
            let state_guard = state.read().await;
            state_guard.metrics.http_requests_total
                .with_label_values(&[&method_str, route_label, &status.as_u16().to_string()])
                .inc();
            state_guard.metrics.http_request_duration_seconds
                .with_label_values(&[&method_str, route_label])
                .observe(start_time.elapsed().as_secs_f64());
            drop(state_guard);
            return (status, "Failed to read request body").into_response();
        }
    };

    // Lock state for reading
    let state_guard = state.read().await;

    // Find matching route
    let route = match state_guard.config.find_route(&path, method.as_str()) {
        Some(r) => r,
        None => {
            tracing::debug!(method = %method, path = %path, "Route not found");
            let status = StatusCode::NOT_FOUND;
            state_guard.metrics.routes_not_found_total
                .with_label_values(&[&method_str])
                .inc();
            state_guard.metrics.http_requests_total
                .with_label_values(&[&method_str, "not_found", &status.as_u16().to_string()])
                .inc();
            state_guard.metrics.http_request_duration_seconds
                .with_label_values(&[&method_str, "not_found"])
                .observe(start_time.elapsed().as_secs_f64());
            drop(state_guard);
            return (status, "Route not found").into_response();
        }
    };

    // Get effective config for this route
    let effective_config = state_guard.config.get_effective_config(route);
    let upstream_url = route.upstream.clone();
    let schema_path = route.schema.clone();
    let openapi_options = route.openapi_options();
    let route_pattern = route.path.clone();

    drop(state_guard);

    let validation_target = if let Some(openapi) = openapi_options {
        Some(ValidationTarget::OpenApi(openapi))
    } else {
        schema_path.map(ValidationTarget::JsonSchema)
    };

    let ctx = RequestContext {
        method,
        upstream_url,
        path,
        path_and_query: path_with_query,
        query: uri.query().map(|q| q.to_string()),
        headers,
        body_bytes,
        route_pattern,
    };

    match validation_target {
        None => forward_without_validation(ctx, state).await,
        Some(ValidationTarget::JsonSchema(schema_path)) => {
            handle_json_schema_validation(ctx, schema_path, state, effective_config).await
        }
        Some(ValidationTarget::OpenApi(openapi)) => {
            handle_openapi_validation(ctx, openapi, state, effective_config).await
        }
    }
}

enum ValidationTarget {
    JsonSchema(PathBuf),
    OpenApi(OpenApiOptions),
}

async fn forward_without_validation(
    ctx: RequestContext,
    state: Arc<tokio::sync::RwLock<AppState>>,
) -> Response {
    let start_time = Instant::now();
    let RequestContext {
        method,
        upstream_url,
        path_and_query,
        headers,
        body_bytes,
        route_pattern,
        ..
    } = ctx;
    let method_str = method.as_str().to_uppercase();
    let route_label = &route_pattern;

    // Record validation attempt (none)
    {
        let state_guard = state.read().await;
        state_guard.metrics.validation_attempts_total
            .with_label_values(&["none"])
            .inc();
        drop(state_guard);
    }

    // Forward request and record upstream metrics
    let upstream_start = Instant::now();
    let state_guard = state.read().await;
    let response = forward_request(
        &state_guard.http_client,
        method.clone(),
        &upstream_url,
        &path_and_query,
        headers,
        body_bytes,
    )
    .await;
    let upstream_duration = upstream_start.elapsed().as_secs_f64();
    let status = response.status();
    let status_code = status.as_u16().to_string();
    drop(state_guard);

        // Record upstream metrics
        {
            let state_guard = state.read().await;
            state_guard.metrics.upstream_requests_total
                .with_label_values(&[&status_code])
                .inc();
            state_guard.metrics.upstream_request_duration_seconds
                .with_label_values(&[])
                .observe(upstream_duration);
            drop(state_guard);
        }

        // Record final request metrics
        {
            let state_guard = state.read().await;
            state_guard.metrics.http_requests_total
                .with_label_values(&[&method_str, route_label, &status_code])
                .inc();
            state_guard.metrics.http_request_duration_seconds
                .with_label_values(&[&method_str, route_label])
                .observe(start_time.elapsed().as_secs_f64());
            drop(state_guard);
        }

        response
}

async fn handle_json_schema_validation(
    ctx: RequestContext,
    schema_path: PathBuf,
    state: Arc<tokio::sync::RwLock<AppState>>,
    effective_config: GlobalConfig,
) -> Response {
    let start_time = Instant::now();
    let method_str = ctx.method.as_str().to_uppercase();
    let route_label = &ctx.route_pattern;

    // Record validation attempt
    {
        let state_guard = state.read().await;
        state_guard.metrics.validation_attempts_total
            .with_label_values(&["json_schema"])
            .inc();
        drop(state_guard);
    }

    if ctx.body_bytes.is_empty() {
        return forward_without_validation(ctx, state).await;
    }

    let ctx_for_parse = ctx.clone();
    let json_value = match parse_json_body_or_handle_error(
        ctx_for_parse,
        &effective_config,
        state.clone(),
    )
    .await
    {
        Ok(value) => value,
        Err(response) => {
            // Record validation failure for JSON parse error
            let state_guard = state.read().await;
            state_guard.metrics.validation_failures_total
                .with_label_values(&["json_schema", "invalid_json"])
                .inc();
            drop(state_guard);
            return response;
        }
    };

    let schema = {
        let mut state_guard = state.write().await;
        let was_cached = state_guard.schema_cache.cache.contains_key(&schema_path);
        let schema_result = state_guard.schema_cache.load(&schema_path);
        drop(state_guard);
        
        // Record cache hit/miss
        {
            let state_guard = state.read().await;
            if was_cached {
                state_guard.metrics.schema_cache_hits_total.inc();
            } else {
                state_guard.metrics.schema_cache_misses_total.inc();
            }
            drop(state_guard);
        }

        match schema_result {
            Ok(schema) => schema,
            Err(e) => {
                let error_msg = format!("{}", e);
                tracing::warn!(
                    method = %ctx.method,
                    path = %ctx.path,
                    schema_path = %schema_path.display(),
                    error = %e,
                    "Failed to load schema"
                );
                // Record validation failure
                {
                    let state_guard = state.read().await;
                    state_guard.metrics.validation_failures_total
                        .with_label_values(&["json_schema", "schema_load_error"])
                        .inc();
                    drop(state_guard);
                }
                return handle_error(
                    &error_msg,
                    &effective_config,
                    ctx,
                    state,
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
                .await;
            }
        }
    };

    let validation_result = validate(&schema, &json_value);

    if validation_result.valid {
        // Record validation success
        {
            let state_guard = state.read().await;
            state_guard.metrics.validation_success_total
                .with_label_values(&["json_schema"])
                .inc();
            drop(state_guard);
        }

        tracing::debug!(
            method = %ctx.method,
            path = %ctx.path,
            upstream = %ctx.upstream_url,
            "Request validated successfully"
        );

        let mut forwarding_headers = ctx.headers;
        if effective_config.add_validation_header {
            if let Ok(header_value) = "true".parse() {
                forwarding_headers.insert("X-Schema-Validated", header_value);
            }
        }

        // Forward request and record upstream metrics
        let upstream_start = Instant::now();
        let state_guard = state.read().await;
        let response = forward_request(
            &state_guard.http_client,
            ctx.method.clone(),
            &ctx.upstream_url,
            &ctx.path_and_query,
            forwarding_headers,
            ctx.body_bytes.clone(),
        )
        .await;
        let upstream_duration = upstream_start.elapsed().as_secs_f64();
        let status = response.status();
        let status_code = status.as_u16().to_string();
        drop(state_guard);

        // Record upstream metrics
        {
            let state_guard = state.read().await;
            state_guard.metrics.upstream_requests_total
                .with_label_values(&[&status_code])
                .inc();
            state_guard.metrics.upstream_request_duration_seconds
                .with_label_values(&[])
                .observe(upstream_duration);
            drop(state_guard);
        }

        // Record final request metrics
        {
            let state_guard = state.read().await;
            state_guard.metrics.http_requests_total
                .with_label_values(&[&method_str, route_label, &status_code])
                .inc();
            state_guard.metrics.http_request_duration_seconds
                .with_label_values(&[&method_str, route_label])
                .observe(start_time.elapsed().as_secs_f64());
            drop(state_guard);
        }

        response
    } else {
        // Record validation failure
        {
            let state_guard = state.read().await;
            state_guard.metrics.validation_failures_total
                .with_label_values(&["json_schema", "validation_failed"])
                .inc();
            drop(state_guard);
        }

        let error_msg = format!("Validation failed: {}", validation_result.errors.join(", "));
        tracing::warn!(
            method = %ctx.method,
            path = %ctx.path,
            errors = ?validation_result.errors,
            "Validation failed"
        );
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

async fn handle_openapi_validation(
    ctx: RequestContext,
    openapi: OpenApiOptions,
    state: Arc<tokio::sync::RwLock<AppState>>,
    effective_config: GlobalConfig,
) -> Response {
    let start_time = Instant::now();
    let method_str = ctx.method.as_str().to_uppercase();
    let route_label = &ctx.route_pattern;

    // Record validation attempt
    {
        let state_guard = state.read().await;
        state_guard.metrics.validation_attempts_total
            .with_label_values(&["openapi"])
            .inc();
        drop(state_guard);
    }

    let plan = {
        let mut state_guard = state.write().await;
        let result = state_guard.openapi_cache.load_operation(
            &openapi.spec,
            &ctx.path,
            &ctx.method,
            openapi.operation_id.as_deref(),
        );
        drop(state_guard);
        match result {
            Ok(plan) => plan,
            Err(e) => {
                let error_msg = format!("{}", e);
                tracing::warn!(
                    method = %ctx.method,
                    path = %ctx.path,
                    spec = %openapi.spec.display(),
                    error = %e,
                    "Failed to load OpenAPI schema"
                );
                // Record validation failure
                {
                    let state_guard = state.read().await;
                    state_guard.metrics.validation_failures_total
                        .with_label_values(&["openapi", "schema_load_error"])
                        .inc();
                    drop(state_guard);
                }
                return handle_error(
                    &error_msg,
                    &effective_config,
                    ctx,
                    state,
                    StatusCode::INTERNAL_SERVER_ERROR,
                )
                .await;
            }
        }
    };

    if let Err(response) =
        validate_openapi_parameters(&plan, &ctx, &effective_config, state.clone()).await
    {
        // Record validation failure for parameter validation
        {
            let state_guard = state.read().await;
            state_guard.metrics.validation_failures_total
                .with_label_values(&["openapi", "parameter_validation_failed"])
                .inc();
            drop(state_guard);
        }
        return response;
    }

    if plan.schema.is_none() && ctx.body_bytes.is_empty() {
        return forward_without_validation(ctx, state).await;
    }

    if plan.body_required && ctx.body_bytes.is_empty() {
        let error_msg = format!(
            "OpenAPI request body required for {} {}",
            plan.method, plan.path_template
        );
        // Record validation failure
        {
            let state_guard = state.read().await;
            state_guard.metrics.validation_failures_total
                .with_label_values(&["openapi", "missing_body"])
                .inc();
            drop(state_guard);
        }
        return handle_error(
            &error_msg,
            &effective_config,
            ctx,
            state,
            StatusCode::BAD_REQUEST,
        )
        .await;
    }

    let schema = match plan.schema.clone() {
        Some(schema) => schema,
        None => {
            return forward_without_validation(ctx, state).await;
        }
    };

    let ctx_for_parse = ctx.clone();
    let json_value = match parse_json_body_or_handle_error(
        ctx_for_parse,
        &effective_config,
        state.clone(),
    )
    .await
    {
        Ok(value) => value,
        Err(response) => {
            // Record validation failure for JSON parse error
            {
                let state_guard = state.read().await;
                state_guard.metrics.validation_failures_total
                    .with_label_values(&["openapi", "invalid_json"])
                    .inc();
                drop(state_guard);
            }
            return response;
        }
    };

    let validation_result = validate(&schema, &json_value);

    if validation_result.valid {
        // Record validation success
        {
            let state_guard = state.read().await;
            state_guard.metrics.validation_success_total
                .with_label_values(&["openapi"])
                .inc();
            drop(state_guard);
        }

        tracing::debug!(
            method = %ctx.method,
            path = %ctx.path,
            upstream = %ctx.upstream_url,
            spec = %openapi.spec.display(),
            "OpenAPI validation passed"
        );

        let mut forwarding_headers = ctx.headers.clone();
        if effective_config.add_validation_header {
            if let Ok(header_value) = "openapi".parse() {
                forwarding_headers.insert("X-Schema-Validated", header_value);
            }
        }

        // Forward request and record upstream metrics
        let upstream_start = Instant::now();
        let state_guard = state.read().await;
        let response = forward_request(
            &state_guard.http_client,
            ctx.method.clone(),
            &ctx.upstream_url,
            &ctx.path_and_query,
            forwarding_headers,
            ctx.body_bytes.clone(),
        )
        .await;
        let upstream_duration = upstream_start.elapsed().as_secs_f64();
        drop(state_guard);

        // Record upstream metrics before response validation
        {
            let state_guard = state.read().await;
            let status = response.status();
            let status_code = status.as_u16().to_string();
            state_guard.metrics.upstream_requests_total
                .with_label_values(&[&status_code])
                .inc();
            state_guard.metrics.upstream_request_duration_seconds
                .with_label_values(&[])
                .observe(upstream_duration);
            drop(state_guard);
        }

        let response = validate_openapi_response(response, &plan, &ctx, &effective_config).await;
        
        // Record final request metrics
        {
            let state_guard = state.read().await;
            let status = response.status();
            let status_code = status.as_u16().to_string();
            state_guard.metrics.http_requests_total
                .with_label_values(&[&method_str, route_label, &status_code])
                .inc();
            state_guard.metrics.http_request_duration_seconds
                .with_label_values(&[&method_str, route_label])
                .observe(start_time.elapsed().as_secs_f64());
            drop(state_guard);
        }

        response
    } else {
        // Record validation failure
        {
            let state_guard = state.read().await;
            state_guard.metrics.validation_failures_total
                .with_label_values(&["openapi", "validation_failed"])
                .inc();
            drop(state_guard);
        }

        let error_msg = format!("Validation failed: {}", validation_result.errors.join(", "));
        tracing::warn!(
            method = %ctx.method,
            path = %ctx.path,
            errors = ?validation_result.errors,
            "OpenAPI validation failed"
        );
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
    let method_str = ctx.method.as_str().to_uppercase();
    let route_label = &ctx.route_pattern;
    let status_code = error_status.as_u16().to_string();

    if effective_config.forward_on_error {
        // Forward to upstream with error header
        tracing::warn!(
            method = %ctx.method,
            path = %ctx.path,
            upstream = %ctx.upstream_url,
            error = %error_msg,
            "Forwarding request to upstream despite error (forward_on_error: true)"
        );

        // Add error header to request if configured
        let mut forwarding_headers = ctx.headers;
        if effective_config.add_error_header {
            if let Ok(header_value) = error_msg.parse() {
                forwarding_headers.insert("X-Gateway-Error", header_value);
            }
        }

        // Forward request and record upstream metrics
        let upstream_start = Instant::now();
        let state_guard = state.read().await;
        let response = forward_request(
            &state_guard.http_client,
            ctx.method.clone(),
            &ctx.upstream_url,
            &ctx.path_and_query,
            forwarding_headers,
            ctx.body_bytes.clone(),
        )
        .await;
        let upstream_duration = upstream_start.elapsed().as_secs_f64();
        let response_status = response.status();
        let response_status_code = response_status.as_u16().to_string();
        drop(state_guard);

        // Record upstream metrics
        {
            let state_guard = state.read().await;
            state_guard.metrics.upstream_requests_total
                .with_label_values(&[&response_status_code])
                .inc();
            state_guard.metrics.upstream_request_duration_seconds
                .with_label_values(&[])
                .observe(upstream_duration);
            // Record upstream errors if status indicates error
            if response_status.is_server_error() || response_status.is_client_error() {
                let error_type = if response_status.is_server_error() {
                    "server_error"
                } else {
                    "client_error"
                };
                state_guard.metrics.upstream_errors_total
                    .with_label_values(&[error_type])
                    .inc();
            }
            drop(state_guard);
        }

        // Record final request metrics
        {
            let state_guard = state.read().await;
            state_guard.metrics.http_requests_total
                .with_label_values(&[&method_str, route_label, &response_status_code])
                .inc();
            drop(state_guard);
        }

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
        
        // Record final request metrics
        {
            let state_guard = state.read().await;
            state_guard.metrics.http_requests_total
                .with_label_values(&[&method_str, route_label, &status_code])
                .inc();
            drop(state_guard);
        }

        let error_body = serde_json::json!({
            "error": error_msg
        });
        let body_str = serde_json::to_string(&error_body)
            .unwrap_or_else(|_| format!(r#"{{"error":"{}"}}"#, error_msg));
        (error_status, body_str).into_response()
    }
}

async fn parse_json_body_or_handle_error(
    ctx: RequestContext,
    effective_config: &GlobalConfig,
    state: Arc<tokio::sync::RwLock<AppState>>,
) -> Result<Value, Response> {
    match serde_json::from_slice::<Value>(&ctx.body_bytes) {
        Ok(v) => Ok(v),
        Err(e) => {
            let error_msg = format!("Invalid JSON: {}", e);
            tracing::warn!(
                method = %ctx.method,
                path = %ctx.path,
                error = %e,
                "Failed to parse JSON body"
            );
            Err(handle_error(
                &error_msg,
                effective_config,
                ctx,
                state,
                StatusCode::BAD_REQUEST,
            )
            .await)
        }
    }
}

async fn validate_openapi_parameters(
    plan: &OperationValidationPlan,
    ctx: &RequestContext,
    effective_config: &GlobalConfig,
    state: Arc<tokio::sync::RwLock<AppState>>,
) -> Result<(), Response> {
    let path_params = extract_path_params(&ctx.path, &plan.path_template);
    let path_params = match path_params {
        Some(params) => params,
        None => {
            let error_msg = format!(
                "Route '{}' no longer matches OpenAPI template '{}'",
                ctx.path, plan.path_template
            );
            return Err(handle_error(
                &error_msg,
                effective_config,
                ctx.clone(),
                state,
                StatusCode::BAD_REQUEST,
            )
            .await);
        }
    };

    let query_params = parse_query_params(ctx.query.as_deref());
    let header_params = build_header_lookup(&ctx.headers);
    let cookie_params = parse_cookie_header(&ctx.headers);

    for param in &plan.parameters {
        let raw_value = match param.location {
            ParameterLocation::Path => path_params.get(&param.name).cloned(),
            ParameterLocation::Query => query_params
                .get(&param.name)
                .and_then(|vals| vals.first().cloned()),
            ParameterLocation::Header => {
                header_params.get(&param.name.to_ascii_lowercase()).cloned()
            }
            ParameterLocation::Cookie => cookie_params.get(&param.name).cloned(),
        };

        let Some(raw_value) = raw_value else {
            if param.required {
                let error_msg = format!(
                    "Missing required {} parameter '{}'",
                    parameter_location_label(param.location),
                    param.name
                );
                return Err(handle_error(
                    &error_msg,
                    effective_config,
                    ctx.clone(),
                    state,
                    StatusCode::BAD_REQUEST,
                )
                .await);
            }
            continue;
        };

        let Some(schema) = &param.schema else {
            continue;
        };

        let coerced_value = match param.coerce_value(&raw_value) {
            Ok(value) => value,
            Err(message) => {
                return Err(handle_error(
                    &message,
                    effective_config,
                    ctx.clone(),
                    state,
                    StatusCode::BAD_REQUEST,
                )
                .await);
            }
        };

        let validation_error = schema.validate(&coerced_value).err();
        if let Some(mut errors) = validation_error {
            let first_error = errors
                .next()
                .map(|e| e.to_string())
                .unwrap_or_else(|| "Parameter validation failed".to_string());
            drop(errors);
            let error_msg = format!("Parameter '{}' invalid: {}", param.name, first_error);
            return Err(handle_error(
                &error_msg,
                effective_config,
                ctx.clone(),
                state,
                StatusCode::BAD_REQUEST,
            )
            .await);
        }
    }

    Ok(())
}

fn extract_path_params(path: &str, template: &str) -> Option<HashMap<String, String>> {
    let actual_segments: Vec<&str> = path.trim_matches('/').split('/').collect();
    let template_segments: Vec<&str> = template.trim_matches('/').split('/').collect();

    if actual_segments.len() != template_segments.len() {
        return None;
    }

    let mut params = HashMap::new();
    for (actual, pattern) in actual_segments.iter().zip(template_segments.iter()) {
        if pattern.starts_with('{') && pattern.ends_with('}') {
            let name = pattern.trim_start_matches('{').trim_end_matches('}');
            params.insert(name.to_string(), (*actual).to_string());
        } else if pattern != actual {
            return None;
        }
    }

    Some(params)
}

fn parse_query_params(query: Option<&str>) -> HashMap<String, Vec<String>> {
    let mut map = HashMap::new();
    if let Some(q) = query {
        for (key, value) in form_urlencoded::parse(q.as_bytes()) {
            map.entry(key.into_owned())
                .or_insert_with(Vec::new)
                .push(value.into_owned());
        }
    }
    map
}

fn build_header_lookup(headers: &HeaderMap) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for (name, value) in headers.iter() {
        if let Ok(val_str) = value.to_str() {
            map.entry(name.as_str().to_ascii_lowercase())
                .or_insert_with(|| val_str.to_string());
        }
    }
    map
}

fn parse_cookie_header(headers: &HeaderMap) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(cookie_header) = headers.get("cookie") {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for pair in cookie_str.split(';') {
                let mut parts = pair.trim().splitn(2, '=');
                if let Some(name) = parts.next() {
                    if let Some(value) = parts.next() {
                        map.insert(name.trim().to_string(), value.trim().to_string());
                    }
                }
            }
        }
    }
    map
}

fn parameter_location_label(location: ParameterLocation) -> &'static str {
    match location {
        ParameterLocation::Path => "path",
        ParameterLocation::Query => "query",
        ParameterLocation::Header => "header",
        ParameterLocation::Cookie => "cookie",
    }
}

async fn validate_openapi_response(
    response: Response,
    plan: &OperationValidationPlan,
    ctx: &RequestContext,
    effective_config: &GlobalConfig,
) -> Response {
    if plan.response_schemas.is_empty() {
        return response;
    }

    let schema = match select_response_schema(&plan.response_schemas, response.status()) {
        Some(schema) => schema,
        None => return response,
    };

    if !has_json_content_type(response.headers()) {
        return response;
    }

    let (parts, body) = response.into_parts();
    let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(_) => {
            let error_msg = "Failed to read upstream response body".to_string();
            tracing::warn!(
                method = %ctx.method,
                path = %ctx.path,
                error = %error_msg,
                "Unable to validate OpenAPI response"
            );
            return (
                StatusCode::BAD_GATEWAY,
                serde_json::json!({ "error": error_msg }).to_string(),
            )
                .into_response();
        }
    };

    if body_bytes.is_empty() {
        return Response::from_parts(parts, Body::from(body_bytes));
    }

    let mut rebuilt = Response::from_parts(parts, Body::from(body_bytes.clone()));

    match serde_json::from_slice::<Value>(&body_bytes) {
        Ok(json) => match schema.validate(&json) {
            Ok(_) => rebuilt,
            Err(errors) => {
                let messages: Vec<String> = errors.map(|e| e.to_string()).collect();
                let error_msg = format!(
                    "OpenAPI response validation failed: {}",
                    messages.join(", ")
                );
                tracing::warn!(
                    method = %ctx.method,
                    path = %ctx.path,
                    error = %error_msg,
                    "Response validation failed"
                );
                if effective_config.forward_on_error {
                    add_error_header(&mut rebuilt, effective_config, &error_msg);
                    rebuilt
                } else {
                    (
                        StatusCode::BAD_GATEWAY,
                        serde_json::json!({ "error": error_msg }).to_string(),
                    )
                        .into_response()
                }
            }
        },
        Err(e) => {
            let error_msg = format!("Invalid JSON in upstream response: {}", e);
            tracing::warn!(
                method = %ctx.method,
                path = %ctx.path,
                error = %error_msg,
                "Response JSON parse failed"
            );
            if effective_config.forward_on_error {
                add_error_header(&mut rebuilt, effective_config, &error_msg);
                rebuilt
            } else {
                (
                    StatusCode::BAD_GATEWAY,
                    serde_json::json!({ "error": error_msg }).to_string(),
                )
                    .into_response()
            }
        }
    }
}

fn select_response_schema(
    map: &HashMap<ResponseKey, Arc<JSONSchema>>,
    status: StatusCode,
) -> Option<Arc<JSONSchema>> {
    map.get(&ResponseKey::Status(status.as_u16()))
        .cloned()
        .or_else(|| map.get(&ResponseKey::Default).cloned())
}

fn has_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_ascii_lowercase().contains("json"))
        .unwrap_or(false)
}

fn add_error_header(response: &mut Response, config: &GlobalConfig, message: &str) {
    if !config.add_error_header {
        return;
    }
    if let Ok(value) = HeaderValue::from_str(message) {
        response.headers_mut().insert("X-Gateway-Error", value);
    }
}

fn build_forward_path(path: &str, query: Option<&str>) -> String {
    match query {
        Some(q) if !q.is_empty() => format!("{}?{}", path, q),
        _ => path.to_string(),
    }
}
