use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Method, Uri};
use axum::routing::any;
use axum::Router;
use std::sync::Arc;
use tokio::sync::RwLock;

use schema_gateway::cli::Cli;
use schema_gateway::config::Config;
use schema_gateway::handler::{handle_request, AppState};
use schema_gateway::schema::SchemaCache;

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

    let app_state = AppState {
        config,
        schema_cache: SchemaCache::new(),
        http_client: reqwest::Client::new(),
    };

    let shared_state = Arc::new(RwLock::new(app_state));

    // Create axum router that matches all requests
    let app = Router::new()
        .route("/*path", any(handler))
        .with_state(shared_state);

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
