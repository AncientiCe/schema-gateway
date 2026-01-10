use axum::response::IntoResponse;
use reqwest::Client;
use schema_gateway::config::Config;
use schema_gateway::handler::{build_http_client, AppState};
use schema_gateway::metrics::Metrics;
use schema_gateway::openapi::OpenApiCache;
use schema_gateway::schema::SchemaCache;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

fn write_temp_config(contents: &str) -> PathBuf {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("config.yml");
    fs::write(&path, contents).expect("write temp config");
    let _ = Box::leak(Box::new(dir));
    path
}

async fn create_test_server(config_content: &str) -> (MockServer, u16) {
    // Start mock server first
    let mock_server = MockServer::start().await;
    let mock_uri = mock_server.uri();
    
    // Replace placeholder with mock server URI
    let config_content = config_content.replace("http://localhost:3000", mock_uri.as_str());
    
    let config_path = write_temp_config(&config_content);
    let config = Config::from_file(&config_path).expect("load config");

    let metrics = Arc::new(Metrics::new().expect("create metrics"));
    let app_state = AppState {
        config,
        schema_cache: SchemaCache::new(),
        openapi_cache: OpenApiCache::new(),
        http_client: build_http_client(),
        metrics: metrics.clone(),
    };

    let shared_state = Arc::new(RwLock::new(app_state));

    // Start axum server
    let app = axum::Router::new()
        .route("/metrics", axum::routing::get(metrics_handler))
        .route("/health", axum::routing::get(health_handler))
        .route("/health/ready", axum::routing::get(readiness_handler))
        .route("/health/live", axum::routing::get(liveness_handler))
        .route("/*path", axum::routing::any(handler))
        .with_state(shared_state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind to port");
    let addr = listener.local_addr().expect("get local addr");
    let port = addr.port();

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    // Wait a bit for server to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    (mock_server, port)
}

async fn metrics_handler(
    axum::extract::State(state): axum::extract::State<
        Arc<tokio::sync::RwLock<schema_gateway::handler::AppState>>,
    >,
) -> axum::response::Response {
    let state_guard = state.read().await;
    match state_guard.metrics.gather() {
        Ok(output) => axum::response::Response::builder()
            .status(axum::http::StatusCode::OK)
            .header("Content-Type", "text/plain; version=0.0.4")
            .body(axum::body::Body::from(output))
            .unwrap(),
        Err(e) => axum::response::Response::builder()
            .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
            .body(axum::body::Body::from(format!("Error: {}", e)))
            .unwrap(),
    }
}

async fn health_handler() -> &'static str {
    "OK"
}

async fn readiness_handler(
    axum::extract::State(state): axum::extract::State<
        Arc<tokio::sync::RwLock<schema_gateway::handler::AppState>>,
    >,
) -> axum::response::Response {
    let state_guard = state.read().await;
    if state_guard.config.routes.is_empty() {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "No routes configured",
        )
            .into_response();
    }
    (axum::http::StatusCode::OK, "Ready").into_response()
}

async fn liveness_handler() -> &'static str {
    "Alive"
}

async fn handler(
    axum::extract::State(state): axum::extract::State<
        Arc<tokio::sync::RwLock<schema_gateway::handler::AppState>>,
    >,
    method: axum::http::Method,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    body: axum::body::Body,
) -> axum::response::Response {
    schema_gateway::handler::handle_request(axum::extract::State(state), method, uri, headers, body)
        .await
}

#[tokio::test]
async fn test_metrics_endpoint_returns_prometheus_format() {
    let config = r#"
global:
  forward_on_error: false
routes:
  - path: /api/test
    method: GET
    upstream: http://localhost:3000
"#;

    let (_mock_server, port) = create_test_server(config).await;
    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/metrics", port);

    let response = client.get(&url).send().await.expect("send request");
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.headers().get("Content-Type").unwrap(),
        "text/plain; version=0.0.4"
    );

    let body = response.text().await.expect("read body");
    // Check that it contains Prometheus format elements
    // Note: Prometheus only outputs metrics that have been incremented
    // So we check for the format structure, not specific metric values
    assert!(body.contains("# HELP") || body.contains("# TYPE"));
    // If no requests have been made, the body might be empty or just have type definitions
    // That's valid Prometheus format
}

#[tokio::test]
async fn test_health_endpoint_returns_ok() {
    let config = r#"
global:
  forward_on_error: false
routes:
  - path: /api/test
    method: GET
    upstream: http://localhost:3000
"#;

    let (_mock_server, port) = create_test_server(config).await;
    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/health", port);

    let response = client.get(&url).send().await.expect("send request");
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.expect("read body"), "OK");
}

#[tokio::test]
async fn test_readiness_endpoint_returns_ready() {
    let config = r#"
global:
  forward_on_error: false
routes:
  - path: /api/test
    method: GET
    upstream: http://localhost:3000
"#;

    let (_mock_server, port) = create_test_server(config).await;
    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/health/ready", port);

    let response = client.get(&url).send().await.expect("send request");
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.expect("read body"), "Ready");
}

#[tokio::test]
async fn test_readiness_endpoint_unavailable_when_no_routes() {
    let config = r#"
global:
  forward_on_error: false
routes: []
"#;

    let (_mock_server, port) = create_test_server(config).await;
    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/health/ready", port);

    let response = client.get(&url).send().await.expect("send request");
    assert_eq!(response.status(), 503);
    assert_eq!(
        response.text().await.expect("read body"),
        "No routes configured"
    );
}

#[tokio::test]
async fn test_liveness_endpoint_returns_alive() {
    let config = r#"
global:
  forward_on_error: false
routes:
  - path: /api/test
    method: GET
    upstream: http://localhost:3000
"#;

    let (_mock_server, port) = create_test_server(config).await;
    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/health/live", port);

    let response = client.get(&url).send().await.expect("send request");
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().await.expect("read body"), "Alive");
}

#[tokio::test]
async fn test_metrics_increment_on_request() {
    let schema_json = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "name": {"type": "string"}
        },
        "required": ["name"]
    }"#;

    let schema_dir = tempfile::tempdir().expect("create temp dir");
    let schema_path = schema_dir.path().join("schema.json");
    fs::write(&schema_path, schema_json).expect("write schema");

    let config = format!(
        r#"
global:
  forward_on_error: false
routes:
  - path: /api/users
    method: POST
    schema: {}
    upstream: http://localhost:3000
"#,
        schema_path.display()
    );

    let (mock_server, port) = create_test_server(&config).await;

    Mock::given(path("/api/users"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let client = Client::new();
    let base_url = format!("http://127.0.0.1:{}", port);

    // Make a request
    let response = client
        .post(&format!("{}/api/users", base_url))
        .json(&serde_json::json!({"name": "test"}))
        .send()
        .await
        .expect("send request");
    assert_eq!(response.status(), 200);

    // Check metrics
    let metrics_response = client
        .get(&format!("{}/metrics", base_url))
        .send()
        .await
        .expect("get metrics");
    let metrics_body = metrics_response.text().await.expect("read metrics");

    // Check that metrics were recorded
    assert!(metrics_body.contains("http_requests_total"));
    assert!(metrics_body.contains("validation_attempts_total"));
    assert!(metrics_body.contains("validation_success_total"));
}
