use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get};
use axum::Router;
use schema_gateway::cli::Cli;
use schema_gateway::config::Config;
use schema_gateway::handler::{build_http_client, handle_request, AppState};
use schema_gateway::health;
use schema_gateway::metrics::Metrics;
use schema_gateway::openapi::OpenApiCache;
use schema_gateway::schema::SchemaCache;
use schema_gateway::watcher;
use std::sync::Arc;
use tokio::signal;
use tokio::sync::RwLock;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Parse CLI arguments
    let cli = Cli::parse_args();

    // Load config from file
    let config = match Config::from_file(&cli.config) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("Error loading config: {}", e);
            std::process::exit(1);
        }
    };

    // Validate config
    if let Err(e) = config.validate() {
        eprintln!("Invalid config: {}", e);
        std::process::exit(1);
    }

    // If validate-config mode, exit after validation
    if cli.validate_config {
        println!("Config valid: {}", cli.config.display());
        std::process::exit(0);
    }

    tracing::info!(
        "Starting Schema Gateway with config: {}",
        cli.config.display()
    );
    tracing::info!("Loaded {} route(s)", config.routes.len());

    // Initialize metrics
    let metrics = Arc::new(Metrics::new().unwrap_or_else(|e| {
        eprintln!("Failed to initialize metrics: {}", e);
        std::process::exit(1);
    }));

    // Preload schemas and OpenAPI operations for better performance
    tracing::info!("Preloading schemas and OpenAPI operations...");
    let mut schema_cache = SchemaCache::new();
    let mut openapi_cache = OpenApiCache::new();

    // Collect all schema paths from routes
    let schema_paths: Vec<_> = config
        .routes
        .iter()
        .filter_map(|route| route.schema.clone())
        .collect();

    // Preload all JSON schemas
    let schema_errors = schema_cache.preload_all(schema_paths.iter());
    if !schema_errors.is_empty() {
        tracing::warn!(
            "Failed to preload {} schema(s), will load on first use",
            schema_errors.len()
        );
        for (path, error) in &schema_errors {
            tracing::warn!("  {}: {}", path.display(), error);
        }
    } else {
        tracing::info!("Successfully preloaded all schemas");
    }

    // Preload all OpenAPI operations
    let openapi_errors = openapi_cache.preload_routes(&config.routes);
    if !openapi_errors.is_empty() {
        tracing::warn!(
            "Failed to preload {} OpenAPI operation(s), will load on first use",
            openapi_errors.len()
        );
        for (route, error) in &openapi_errors {
            tracing::warn!("  {}: {}", route, error);
        }
    } else {
        tracing::info!("Successfully preloaded all OpenAPI operations");
    }

    // Wrap caches in Arc<RwLock<>> for independent access
    let schema_cache = Arc::new(tokio::sync::RwLock::new(schema_cache));
    let openapi_cache = Arc::new(tokio::sync::RwLock::new(openapi_cache));

    let app_state = AppState {
        config,
        schema_cache: schema_cache.clone(),
        openapi_cache: openapi_cache.clone(),
        http_client: build_http_client(),
        metrics: metrics.clone(),
    };

    let shared_state = Arc::new(RwLock::new(app_state));

    let _watcher_handle = if cli.no_watch {
        None
    } else {
        tracing::info!("file watching enabled (use --no-watch to disable)");
        Some(watcher::start_watcher(
            cli.config.clone(),
            shared_state.clone(),
        ))
    };

    // Create axum router with metrics, health, and main handler routes
    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health::health))
        .route("/health/ready", get(health::readiness))
        .route("/health/live", get(health::liveness))
        .route("/*path", any(handler))
        .with_state(shared_state)
        // Add request IDs + request tracing for observability/adoption.
        //
        // Note: layers are applied inside-out (last-added runs first on requests),
        // so we add TraceLayer first, then Propagate, then SetRequestId last.
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<_>| {
                let request_id = request
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");

                tracing::info_span!(
                    "http_request",
                    request_id = %request_id,
                    method = %request.method(),
                    uri = %request.uri(),
                    version = ?request.version(),
                    // Filled in later from the handler (once routing is resolved)
                    route = tracing::field::Empty,
                    upstream = tracing::field::Empty,
                )
            }),
        )
        .layer(PropagateRequestIdLayer::new(
            axum::http::HeaderName::from_static("x-request-id"),
        ))
        .layer(SetRequestIdLayer::new(
            axum::http::HeaderName::from_static("x-request-id"),
            MakeRequestUuid,
        ))
        // Add security headers
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("x-content-type-options"),
            axum::http::HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("x-frame-options"),
            axum::http::HeaderValue::from_static("DENY"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            axum::http::HeaderName::from_static("x-xss-protection"),
            axum::http::HeaderValue::from_static("1; mode=block"),
        ));

    let addr = format!("127.0.0.1:{}", cli.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Failed to bind to {}: {}", addr, e);
            std::process::exit(1);
        });

    tracing::info!("Schema Gateway listening on http://{}", addr);
    println!("Schema Gateway listening on http://{}", addr);

    // Setup graceful shutdown
    let shutdown_signal = async {
        let ctrl_c = async {
            signal::ctrl_c()
                .await
                .expect("failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            signal::unix::signal(signal::unix::SignalKind::terminate())
                .expect("failed to install signal handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => {},
            _ = terminate => {},
        }

        tracing::info!("Shutdown signal received, starting graceful shutdown...");
    };

    // Start server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Server error: {}", e);
            std::process::exit(1);
        });

    tracing::info!("Schema Gateway shutdown complete");
}

async fn handler(
    State(state): State<Arc<RwLock<AppState>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> axum::response::Response {
    handle_request(State(state), method, uri, headers, body).await
}

async fn metrics_handler(State(state): State<Arc<RwLock<AppState>>>) -> Response {
    let state_guard = state.read().await;
    match state_guard.metrics.gather() {
        Ok(output) => {
            match Response::builder()
                .status(StatusCode::OK)
                .header("Content-Type", "text/plain; version=0.0.4")
                .body(axum::body::Body::from(output))
            {
                Ok(response) => response,
                Err(e) => {
                    tracing::error!("Failed to build metrics response: {}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to build response",
                    )
                        .into_response()
                }
            }
        }
        Err(e) => {
            tracing::error!("Failed to gather metrics: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Error gathering metrics: {}", e),
            )
                .into_response()
        }
    }
}
