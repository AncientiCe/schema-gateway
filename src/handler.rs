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
    pub http_client: reqwest::Client,
    pub metrics: Arc<Metrics>,
    // Separate caches to reduce lock contention - can be accessed independently
    pub schema_cache: Arc<tokio::sync::RwLock<SchemaCache>>,
    pub openapi_cache: Arc<tokio::sync::RwLock<OpenApiCache>>,
}

/// Accumulator for batching metrics updates to reduce lock contention
struct MetricsAccumulator {
    validation_attempts: Option<&'static str>,
    validation_success: Option<&'static str>,
    validation_failures: Vec<(&'static str, &'static str)>,
    schema_cache_hit: bool,
    schema_cache_miss: bool,
    upstream_requests: Option<String>,
    upstream_duration: Option<f64>,
    upstream_errors: Vec<&'static str>,
    http_requests: Option<(String, String, String)>,
    http_duration: Option<(String, String, f64)>,
    routes_not_found: Option<String>,
}

impl MetricsAccumulator {
    fn new() -> Self {
        Self {
            validation_attempts: None,
            validation_success: None,
            validation_failures: Vec::new(),
            schema_cache_hit: false,
            schema_cache_miss: false,
            upstream_requests: None,
            upstream_duration: None,
            upstream_errors: Vec::new(),
            http_requests: None,
            http_duration: None,
            routes_not_found: None,
        }
    }

    fn record_validation_attempt(&mut self, validation_type: &'static str) {
        self.validation_attempts = Some(validation_type);
    }

    fn record_validation_success(&mut self, validation_type: &'static str) {
        self.validation_success = Some(validation_type);
    }

    fn record_validation_failure(
        &mut self,
        validation_type: &'static str,
        error_type: &'static str,
    ) {
        self.validation_failures.push((validation_type, error_type));
    }

    fn record_schema_cache_hit(&mut self) {
        self.schema_cache_hit = true;
    }

    fn record_schema_cache_miss(&mut self) {
        self.schema_cache_miss = true;
    }

    fn record_upstream_request(&mut self, status: String, duration: f64) {
        self.upstream_requests = Some(status);
        self.upstream_duration = Some(duration);
    }

    fn record_upstream_error(&mut self, error_type: &'static str) {
        self.upstream_errors.push(error_type);
    }

    fn record_http_request(
        &mut self,
        method: String,
        route: String,
        status: String,
        duration: f64,
    ) {
        self.http_requests = Some((method.clone(), route.clone(), status));
        self.http_duration = Some((method, route, duration));
    }

    fn record_route_not_found(&mut self, method: String) {
        self.routes_not_found = Some(method);
    }

    /// Flush all accumulated metrics in a single lock acquisition
    async fn flush(&self, metrics: &Metrics) {
        if let Some(validation_type) = self.validation_attempts {
            metrics
                .validation_attempts_total
                .with_label_values(&[validation_type])
                .inc();
        }

        if let Some(validation_type) = self.validation_success {
            metrics
                .validation_success_total
                .with_label_values(&[validation_type])
                .inc();
        }

        for (validation_type, error_type) in &self.validation_failures {
            metrics
                .validation_failures_total
                .with_label_values(&[validation_type, error_type])
                .inc();
        }

        if self.schema_cache_hit {
            metrics.schema_cache_hits_total.inc();
        }

        if self.schema_cache_miss {
            metrics.schema_cache_misses_total.inc();
        }

        if let Some(ref status) = self.upstream_requests {
            metrics
                .upstream_requests_total
                .with_label_values(&[status])
                .inc();
        }

        if let Some(duration) = self.upstream_duration {
            metrics
                .upstream_request_duration_seconds
                .with_label_values(&[] as &[&str])
                .observe(duration);
        }

        for error_type in &self.upstream_errors {
            metrics
                .upstream_errors_total
                .with_label_values(&[error_type])
                .inc();
        }

        if let Some((ref method, ref route, ref status)) = self.http_requests {
            metrics
                .http_requests_total
                .with_label_values(&[method, route, status])
                .inc();
        }

        if let Some((ref method, ref route, duration)) = self.http_duration {
            metrics
                .http_request_duration_seconds
                .with_label_values(&[method, route])
                .observe(duration);
        }

        if let Some(ref method) = self.routes_not_found {
            metrics
                .routes_not_found_total
                .with_label_values(&[method])
                .inc();
        }
    }
}

/// Build a reqwest client suitable for the gateway.
/// Optimized for low latency with connection pooling, keep-alive, and HTTP/2 support.
/// We disable system proxy lookups to avoid platform-specific panics in tests.
pub fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        // Connection pooling: reuse connections for better performance
        .pool_max_idle_per_host(10)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        // HTTP/2 support: enabled by default via ALPN, but configure keep-alive
        .http2_keep_alive_interval(Some(std::time::Duration::from_secs(30)))
        .http2_keep_alive_timeout(std::time::Duration::from_secs(10))
        .http2_keep_alive_while_idle(true)
        // Timeouts to prevent hanging requests
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(30))
        // Keep-alive: reuse TCP connections (TCP-level keep-alive)
        .tcp_keepalive(std::time::Duration::from_secs(60))
        .build()
        .expect("Failed to build HTTP client")
}

struct RequestContext {
    method: Method,
    upstream_url: String,
    path: String,
    path_and_query: String,
    query: Option<String>,
    headers: HeaderMap,
    body_bytes: Arc<[u8]>,
    route_pattern: String,
    parsed_json: tokio::sync::Mutex<Option<Value>>,
    parsed_params: tokio::sync::Mutex<Option<ParsedParams>>,
}

#[derive(Clone)]
struct ParsedParams {
    path_params: HashMap<String, String>,
    query_params: HashMap<String, Vec<String>>,
    header_params: HashMap<String, String>,
    cookie_params: HashMap<String, String>,
}

impl RequestContext {
    /// Parse JSON body lazily, caching the result
    async fn parse_json(&self) -> Result<Value, serde_json::Error> {
        let mut parsed = self.parsed_json.lock().await;
        if let Some(ref value) = *parsed {
            return Ok(value.clone());
        }

        let value = serde_json::from_slice::<Value>(&self.body_bytes)?;
        *parsed = Some(value.clone());
        Ok(value)
    }

    /// Get parsed parameters lazily, caching the result
    async fn get_parsed_params(&self, path_template: &str) -> Option<ParsedParams> {
        let mut parsed = self.parsed_params.lock().await;
        if let Some(ref params) = *parsed {
            // Return a clone only when needed (avoid unnecessary allocation if we can return reference)
            return Some(ParsedParams {
                path_params: params.path_params.clone(),
                query_params: params.query_params.clone(),
                header_params: params.header_params.clone(),
                cookie_params: params.cookie_params.clone(),
            });
        }

        let path_params = extract_path_params(&self.path, path_template)?;
        let query_params = parse_query_params(self.query.as_deref());
        let header_params = build_header_lookup(&self.headers);
        let cookie_params = parse_cookie_header(&self.headers);

        // Store and return without extra clones
        let params = ParsedParams {
            path_params,
            query_params,
            header_params,
            cookie_params,
        };

        *parsed = Some(params.clone());
        Some(params)
    }
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
    let body_bytes: Arc<[u8]> = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes.to_vec().into(),
        Err(_) => {
            let status = StatusCode::BAD_REQUEST;
            let route_label = "unknown";
            let mut metrics_acc = MetricsAccumulator::new();
            metrics_acc.record_http_request(
                method_str.clone(),
                route_label.to_string(),
                status.as_u16().to_string(),
                start_time.elapsed().as_secs_f64(),
            );
            let state_guard = state.read().await;
            metrics_acc.flush(&state_guard.metrics).await;
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
            let mut metrics_acc = MetricsAccumulator::new();
            metrics_acc.record_route_not_found(method_str.clone());
            metrics_acc.record_http_request(
                method_str.clone(),
                "not_found".to_string(),
                status.as_u16().to_string(),
                start_time.elapsed().as_secs_f64(),
            );
            metrics_acc.flush(&state_guard.metrics).await;
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

    // Enrich the per-request span (created by TraceLayer) with routing info.
    // This makes all subsequent logs automatically include route/upstream.
    tracing::Span::current().record("route", route_pattern.as_str());
    tracing::Span::current().record("upstream", upstream_url.as_str());

    drop(state_guard);

    let validation_target = if let Some(openapi) = openapi_options {
        Some(ValidationTarget::OpenApi(openapi))
    } else {
        schema_path.map(ValidationTarget::JsonSchema)
    };

    let ctx = Arc::new(RequestContext {
        method,
        upstream_url,
        path,
        path_and_query: path_with_query,
        query: uri.query().map(|q| q.to_string()),
        headers,
        body_bytes,
        route_pattern,
        parsed_json: tokio::sync::Mutex::new(None),
        parsed_params: tokio::sync::Mutex::new(None),
    });

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
    ctx: Arc<RequestContext>,
    state: Arc<tokio::sync::RwLock<AppState>>,
) -> Response {
    let start_time = Instant::now();
    let method_str = ctx.method.as_str().to_uppercase();
    let route_label = &ctx.route_pattern;

    let mut metrics_acc = MetricsAccumulator::new();
    metrics_acc.record_validation_attempt("none");

    // Forward request and record upstream metrics
    let upstream_start = Instant::now();
    let state_guard = state.read().await;
    let response = forward_request(
        &state_guard.http_client,
        ctx.method.clone(),
        &ctx.upstream_url,
        &ctx.path_and_query,
        ctx.headers.clone(),
        ctx.body_bytes.to_vec(),
    )
    .await;
    let upstream_duration = upstream_start.elapsed().as_secs_f64();
    let status = response.status();
    let status_code = status.as_u16().to_string();
    drop(state_guard);

    metrics_acc.record_upstream_request(status_code.clone(), upstream_duration);
    metrics_acc.record_http_request(
        method_str.clone(),
        route_label.clone(),
        status_code.clone(),
        start_time.elapsed().as_secs_f64(),
    );

    // Flush all metrics in a single lock acquisition
    let state_guard = state.read().await;
    metrics_acc.flush(&state_guard.metrics).await;
    drop(state_guard);

    response
}

async fn handle_json_schema_validation(
    ctx: Arc<RequestContext>,
    schema_path: PathBuf,
    state: Arc<tokio::sync::RwLock<AppState>>,
    effective_config: GlobalConfig,
) -> Response {
    let start_time = Instant::now();
    let method_str = ctx.method.as_str().to_uppercase();
    let route_label = ctx.route_pattern.clone();

    let mut metrics_acc = MetricsAccumulator::new();
    metrics_acc.record_validation_attempt("json_schema");

    // Early exit if body is empty
    if ctx.body_bytes.is_empty() {
        metrics_acc.record_http_request(
            method_str.clone(),
            route_label.clone(),
            "200".to_string(),
            start_time.elapsed().as_secs_f64(),
        );
        let state_guard = state.read().await;
        metrics_acc.flush(&state_guard.metrics).await;
        drop(state_guard);
        return forward_without_validation(ctx, state).await;
    }

    // Early exit: check content-type before parsing JSON (avoid parsing non-JSON bodies)
    if !has_json_content_type(&ctx.headers) {
        // Not JSON content-type, forward without validation
        metrics_acc.record_http_request(
            method_str.clone(),
            route_label.clone(),
            "200".to_string(),
            start_time.elapsed().as_secs_f64(),
        );
        let state_guard = state.read().await;
        metrics_acc.flush(&state_guard.metrics).await;
        drop(state_guard);
        return forward_without_validation(ctx, state).await;
    }

    // Parse JSON using lazy parsing
    let json_value = match ctx.parse_json().await {
        Ok(value) => value,
        Err(e) => {
            let error_msg = format!("Invalid JSON: {}", e);
            tracing::warn!(
                method = %ctx.method,
                path = %ctx.path,
                error = %e,
                "Failed to parse JSON body"
            );
            metrics_acc.record_validation_failure("json_schema", "invalid_json");
            metrics_acc.record_http_request(
                method_str.clone(),
                route_label.clone(),
                "400".to_string(),
                start_time.elapsed().as_secs_f64(),
            );
            let state_guard = state.read().await;
            metrics_acc.flush(&state_guard.metrics).await;
            drop(state_guard);
            return handle_error(
                &error_msg,
                &effective_config,
                ctx,
                state,
                StatusCode::BAD_REQUEST,
            )
            .await;
        }
    };

    // Access schema cache independently (reduces lock contention)
    let schema = {
        let state_guard = state.read().await;
        let schema_cache = state_guard.schema_cache.clone();
        drop(state_guard);

        // Try read lock first for cache lookup
        let cache_guard = schema_cache.read().await;
        if let Some(schema) = cache_guard.get(&schema_path) {
            metrics_acc.record_schema_cache_hit();
            drop(cache_guard);
            schema
        } else {
            drop(cache_guard);
            // Cache miss - need write lock to insert
            metrics_acc.record_schema_cache_miss();
            let mut cache_guard = schema_cache.write().await;
            match cache_guard.load(&schema_path) {
                Ok(schema) => {
                    drop(cache_guard);
                    schema
                }
                Err(e) => {
                    drop(cache_guard);
                    let error_msg = format!("{}", e);
                    tracing::warn!(
                        method = %ctx.method,
                        path = %ctx.path,
                        schema_path = %schema_path.display(),
                        error = %e,
                        "Failed to load schema"
                    );
                    metrics_acc.record_validation_failure("json_schema", "schema_load_error");
                    metrics_acc.record_http_request(
                        method_str.clone(),
                        route_label.clone(),
                        "500".to_string(),
                        start_time.elapsed().as_secs_f64(),
                    );
                    let state_guard = state.read().await;
                    metrics_acc.flush(&state_guard.metrics).await;
                    drop(state_guard);
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
        }
    };

    let validation_result = validate(&schema, &json_value);

    if validation_result.valid {
        metrics_acc.record_validation_success("json_schema");

        tracing::debug!(
            method = %ctx.method,
            path = %ctx.path,
            upstream = %ctx.upstream_url,
            "Request validated successfully"
        );

        let mut forwarding_headers = ctx.headers.clone();
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
            ctx.body_bytes.to_vec(),
        )
        .await;
        let upstream_duration = upstream_start.elapsed().as_secs_f64();
        let status = response.status();
        let status_code = status.as_u16().to_string();
        drop(state_guard);

        metrics_acc.record_upstream_request(status_code.clone(), upstream_duration);
        metrics_acc.record_http_request(
            method_str.clone(),
            route_label.clone(),
            status_code.clone(),
            start_time.elapsed().as_secs_f64(),
        );

        // Flush all metrics in a single lock acquisition
        let state_guard = state.read().await;
        metrics_acc.flush(&state_guard.metrics).await;
        drop(state_guard);

        response
    } else {
        let error_msg = format!("Validation failed: {}", validation_result.errors.join(", "));
        tracing::warn!(
            method = %ctx.method,
            path = %ctx.path,
            errors = ?validation_result.errors,
            "Validation failed"
        );
        metrics_acc.record_validation_failure("json_schema", "validation_failed");
        metrics_acc.record_http_request(
            method_str.clone(),
            route_label.clone(),
            "400".to_string(),
            start_time.elapsed().as_secs_f64(),
        );
        let state_guard = state.read().await;
        metrics_acc.flush(&state_guard.metrics).await;
        drop(state_guard);
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
    ctx: Arc<RequestContext>,
    openapi: OpenApiOptions,
    state: Arc<tokio::sync::RwLock<AppState>>,
    effective_config: GlobalConfig,
) -> Response {
    let start_time = Instant::now();
    let method_str = ctx.method.as_str().to_uppercase();
    let route_label = ctx.route_pattern.clone();

    let mut metrics_acc = MetricsAccumulator::new();
    metrics_acc.record_validation_attempt("openapi");

    // Access OpenAPI cache independently (reduces lock contention)
    let plan = {
        let state_guard = state.read().await;
        let openapi_cache = state_guard.openapi_cache.clone();
        drop(state_guard);

        // OpenAPI cache needs write lock for loading operations
        let mut cache_guard = openapi_cache.write().await;
        match cache_guard.load_operation(
            &openapi.spec,
            &ctx.path,
            &ctx.method,
            openapi.operation_id.as_deref(),
        ) {
            Ok(plan) => {
                drop(cache_guard);
                plan
            }
            Err(e) => {
                drop(cache_guard);
                let error_msg = format!("{}", e);
                tracing::warn!(
                    method = %ctx.method,
                    path = %ctx.path,
                    spec = %openapi.spec.display(),
                    error = %e,
                    "Failed to load OpenAPI schema"
                );
                metrics_acc.record_validation_failure("openapi", "schema_load_error");
                metrics_acc.record_http_request(
                    method_str.clone(),
                    route_label.clone(),
                    "500".to_string(),
                    start_time.elapsed().as_secs_f64(),
                );
                let state_guard = state.read().await;
                metrics_acc.flush(&state_guard.metrics).await;
                drop(state_guard);
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

    // Early exit if no parameters to validate
    if !plan.parameters.is_empty() {
        if let Err(response) =
            validate_openapi_parameters(&plan, &ctx, &effective_config, state.clone()).await
        {
            metrics_acc.record_validation_failure("openapi", "parameter_validation_failed");
            metrics_acc.record_http_request(
                method_str.clone(),
                route_label.clone(),
                "400".to_string(),
                start_time.elapsed().as_secs_f64(),
            );
            let state_guard = state.read().await;
            metrics_acc.flush(&state_guard.metrics).await;
            drop(state_guard);
            return response;
        }
    }

    // Early exit if no schema and empty body
    if plan.schema.is_none() && ctx.body_bytes.is_empty() {
        metrics_acc.record_http_request(
            method_str.clone(),
            route_label.clone(),
            "200".to_string(),
            start_time.elapsed().as_secs_f64(),
        );
        let state_guard = state.read().await;
        metrics_acc.flush(&state_guard.metrics).await;
        drop(state_guard);
        return forward_without_validation(ctx, state).await;
    }

    if plan.body_required && ctx.body_bytes.is_empty() {
        let error_msg = format!(
            "OpenAPI request body required for {} {}",
            plan.method, plan.path_template
        );
        metrics_acc.record_validation_failure("openapi", "missing_body");
        metrics_acc.record_http_request(
            method_str.clone(),
            route_label.clone(),
            "400".to_string(),
            start_time.elapsed().as_secs_f64(),
        );
        let state_guard = state.read().await;
        metrics_acc.flush(&state_guard.metrics).await;
        drop(state_guard);
        return handle_error(
            &error_msg,
            &effective_config,
            ctx,
            state,
            StatusCode::BAD_REQUEST,
        )
        .await;
    }

    // Use reference instead of clone
    let schema = match plan.schema.as_ref() {
        Some(schema) => Arc::clone(schema),
        None => {
            metrics_acc.record_http_request(
                method_str.clone(),
                route_label.clone(),
                "200".to_string(),
                start_time.elapsed().as_secs_f64(),
            );
            let state_guard = state.read().await;
            metrics_acc.flush(&state_guard.metrics).await;
            drop(state_guard);
            return forward_without_validation(ctx, state).await;
        }
    };

    // Early exit: check content-type before parsing JSON (avoid parsing non-JSON bodies)
    if !has_json_content_type(&ctx.headers) {
        // Not JSON content-type, forward without validation
        metrics_acc.record_http_request(
            method_str.clone(),
            route_label.clone(),
            "200".to_string(),
            start_time.elapsed().as_secs_f64(),
        );
        let state_guard = state.read().await;
        metrics_acc.flush(&state_guard.metrics).await;
        drop(state_guard);
        return forward_without_validation(ctx, state).await;
    }

    // Parse JSON using lazy parsing
    let json_value = match ctx.parse_json().await {
        Ok(value) => value,
        Err(e) => {
            let error_msg = format!("Invalid JSON: {}", e);
            tracing::warn!(
                method = %ctx.method,
                path = %ctx.path,
                error = %e,
                "Failed to parse JSON body"
            );
            metrics_acc.record_validation_failure("openapi", "invalid_json");
            metrics_acc.record_http_request(
                method_str.clone(),
                route_label.clone(),
                "400".to_string(),
                start_time.elapsed().as_secs_f64(),
            );
            let state_guard = state.read().await;
            metrics_acc.flush(&state_guard.metrics).await;
            drop(state_guard);
            return handle_error(
                &error_msg,
                &effective_config,
                ctx,
                state,
                StatusCode::BAD_REQUEST,
            )
            .await;
        }
    };

    let validation_result = validate(&schema, &json_value);

    if validation_result.valid {
        metrics_acc.record_validation_success("openapi");

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
            ctx.body_bytes.to_vec(),
        )
        .await;
        let upstream_duration = upstream_start.elapsed().as_secs_f64();
        drop(state_guard);

        let status = response.status();
        let status_code = status.as_u16().to_string();
        metrics_acc.record_upstream_request(status_code.clone(), upstream_duration);

        let response = validate_openapi_response(response, &plan, &ctx, &effective_config).await;

        let final_status = response.status();
        let final_status_code = final_status.as_u16().to_string();
        metrics_acc.record_http_request(
            method_str.clone(),
            route_label.clone(),
            final_status_code.clone(),
            start_time.elapsed().as_secs_f64(),
        );

        // Flush all metrics in a single lock acquisition
        let state_guard = state.read().await;
        metrics_acc.flush(&state_guard.metrics).await;
        drop(state_guard);

        response
    } else {
        let error_msg = format!("Validation failed: {}", validation_result.errors.join(", "));
        tracing::warn!(
            method = %ctx.method,
            path = %ctx.path,
            errors = ?validation_result.errors,
            "OpenAPI validation failed"
        );
        metrics_acc.record_validation_failure("openapi", "validation_failed");
        metrics_acc.record_http_request(
            method_str.clone(),
            route_label.clone(),
            "400".to_string(),
            start_time.elapsed().as_secs_f64(),
        );
        let state_guard = state.read().await;
        metrics_acc.flush(&state_guard.metrics).await;
        drop(state_guard);
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
    ctx: Arc<RequestContext>,
    state: Arc<tokio::sync::RwLock<AppState>>,
    error_status: StatusCode,
) -> Response {
    let method_str = ctx.method.as_str().to_uppercase();
    let route_label = ctx.route_pattern.clone();
    let status_code = error_status.as_u16().to_string();

    let mut metrics_acc = MetricsAccumulator::new();

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
        let mut forwarding_headers = ctx.headers.clone();
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
            ctx.body_bytes.to_vec(),
        )
        .await;
        let upstream_duration = upstream_start.elapsed().as_secs_f64();
        let response_status = response.status();
        let response_status_code = response_status.as_u16().to_string();
        drop(state_guard);

        metrics_acc.record_upstream_request(response_status_code.clone(), upstream_duration);
        // Record upstream errors if status indicates error
        if response_status.is_server_error() || response_status.is_client_error() {
            let error_type = if response_status.is_server_error() {
                "server_error"
            } else {
                "client_error"
            };
            metrics_acc.record_upstream_error(error_type);
        }

        metrics_acc.record_http_request(
            method_str.clone(),
            route_label.clone(),
            response_status_code.clone(),
            upstream_start.elapsed().as_secs_f64(),
        );

        // Flush all metrics in a single lock acquisition
        let state_guard = state.read().await;
        metrics_acc.flush(&state_guard.metrics).await;
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

        metrics_acc.record_http_request(
            method_str.clone(),
            route_label.clone(),
            status_code.clone(),
            Instant::now().elapsed().as_secs_f64(),
        );

        // Flush metrics
        let state_guard = state.read().await;
        metrics_acc.flush(&state_guard.metrics).await;
        drop(state_guard);

        let error_body = serde_json::json!({
            "error": error_msg
        });
        let body_str = serde_json::to_string(&error_body)
            .unwrap_or_else(|_| format!(r#"{{"error":"{}"}}"#, error_msg));
        (error_status, body_str).into_response()
    }
}

async fn validate_openapi_parameters(
    plan: &OperationValidationPlan,
    ctx: &Arc<RequestContext>,
    effective_config: &GlobalConfig,
    state: Arc<tokio::sync::RwLock<AppState>>,
) -> Result<(), Response> {
    // Use lazy parameter parsing
    let parsed_params = match ctx.get_parsed_params(&plan.path_template).await {
        Some(params) => params,
        None => {
            let error_msg = format!(
                "Route '{}' no longer matches OpenAPI template '{}'",
                ctx.path, plan.path_template
            );
            return Err(handle_error(
                &error_msg,
                effective_config,
                Arc::clone(ctx),
                state,
                StatusCode::BAD_REQUEST,
            )
            .await);
        }
    };

    for param in &plan.parameters {
        let raw_value = match param.location {
            ParameterLocation::Path => parsed_params.path_params.get(&param.name).cloned(),
            ParameterLocation::Query => parsed_params
                .query_params
                .get(&param.name)
                .and_then(|vals| vals.first().cloned()),
            ParameterLocation::Header => parsed_params
                .header_params
                .get(&param.name.to_ascii_lowercase())
                .cloned(),
            ParameterLocation::Cookie => parsed_params.cookie_params.get(&param.name).cloned(),
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
                    Arc::clone(ctx),
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
                    Arc::clone(ctx),
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
                Arc::clone(ctx),
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
    ctx: &Arc<RequestContext>,
    effective_config: &GlobalConfig,
) -> Response {
    // Early exit if no response schemas defined
    if plan.response_schemas.is_empty() {
        return response;
    }

    // Early exit if not JSON content type (check before reading body)
    if !has_json_content_type(response.headers()) {
        return response;
    }

    let schema = match select_response_schema(&plan.response_schemas, response.status()) {
        Some(schema) => schema,
        None => return response,
    };

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

    // Early exit if body is empty
    if body_bytes.is_empty() {
        return Response::from_parts(parts, Body::from(body_bytes));
    }

    // Parse JSON before rebuilding response to avoid cloning
    let json_result = serde_json::from_slice::<Value>(&body_bytes);
    let mut rebuilt = Response::from_parts(parts, Body::from(body_bytes));

    match json_result {
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
        .map(Arc::clone)
        .or_else(|| map.get(&ResponseKey::Default).map(Arc::clone))
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
