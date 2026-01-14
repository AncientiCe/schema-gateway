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
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
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

    let app_state = AppState {
        config,
        schema_cache: SchemaCache::new(),
        openapi_cache: OpenApiCache::new(),
        http_client: build_http_client(),
        metrics: metrics.clone(),
    };

    let shared_state = Arc::new(RwLock::new(app_state));

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

    axum::serve(listener, app).await.unwrap_or_else(|e| {
        eprintln!("Server error: {}", e);
        std::process::exit(1);
    });
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
