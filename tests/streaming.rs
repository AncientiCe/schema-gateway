//! Integration tests for streaming response proxy.
//! Verifies that large upstream responses are streamed through the gateway without full buffering.

use reqwest::Client;
use schema_gateway::config::Config;
use schema_gateway::handler::{build_http_client, handle_request, AppState};
use schema_gateway::metrics::Metrics;
use schema_gateway::openapi::OpenApiCache;
use schema_gateway::schema::SchemaCache;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;
use wiremock::{matchers::path, Mock, MockServer, ResponseTemplate};

fn write_temp_config(contents: &str) -> PathBuf {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("config.yml");
    fs::write(&path, contents).expect("write temp config");
    let _ = Box::leak(Box::new(dir));
    path
}

async fn create_test_server_with_large_response(
    config_content: &str,
    response_body_size: usize,
) -> (MockServer, u16) {
    let mock_server = MockServer::start().await;
    let mock_uri = mock_server.uri();
    let config_content = config_content.replace("http://localhost:3000", mock_uri.as_str());

    let config_path = write_temp_config(&config_content);
    let config = Config::from_file(&config_path).expect("load config");

    let metrics = Arc::new(Metrics::new().expect("create metrics"));
    let app_state = AppState {
        config,
        schema_cache: Arc::new(tokio::sync::RwLock::new(SchemaCache::new())),
        openapi_cache: Arc::new(tokio::sync::RwLock::new(OpenApiCache::new())),
        http_client: build_http_client(),
        metrics: metrics.clone(),
    };

    let shared_state = Arc::new(RwLock::new(app_state));

    let app =
        axum::Router::new()
            .route("/{*path}", axum::routing::any(handler))
            .with_state(shared_state)
            .layer(TraceLayer::new_for_http().make_span_with(
                |_request: &axum::http::Request<_>| tracing::info_span!("http_request"),
            ))
            .layer(PropagateRequestIdLayer::new(
                axum::http::HeaderName::from_static("x-request-id"),
            ))
            .layer(SetRequestIdLayer::new(
                axum::http::HeaderName::from_static("x-request-id"),
                MakeRequestUuid,
            ));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("local addr").port();

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Upstream returns a large body (route has no schema, so gateway uses streaming proxy)
    let large_body: String = (0..response_body_size).map(|_| 'x').collect();
    Mock::given(path("/api/data"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(large_body)
                .insert_header("Content-Type", "application/octet-stream"),
        )
        .mount(&mock_server)
        .await;

    (mock_server, port)
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
    handle_request(axum::extract::State(state), method, uri, headers, body).await
}

/// Route without schema uses forward_request_streaming; client receives full large body.
#[tokio::test]
async fn test_streaming_large_response_through_gateway() {
    let config = r#"
global:
  forward_on_error: true
routes:
  - path: /api/data
    method: GET
    upstream: http://localhost:3000
"#;

    const LARGE_BODY_SIZE: usize = 256 * 1024; // 256 KiB
    let (_mock_server, port) =
        create_test_server_with_large_response(config, LARGE_BODY_SIZE).await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/api/data", port);

    let response = client.get(&url).send().await.expect("send request");
    assert_eq!(response.status(), 200, "gateway should return 200");

    let body_bytes = response.bytes().await.expect("read body");
    assert_eq!(
        body_bytes.len(),
        LARGE_BODY_SIZE,
        "client should receive full streamed body"
    );
    assert!(
        body_bytes.iter().all(|&b| b == b'x'),
        "body content should match upstream"
    );
}

/// Streaming path preserves upstream response headers.
#[tokio::test]
async fn test_streaming_preserves_response_headers() {
    let config = r#"
global:
  forward_on_error: true
routes:
  - path: /api/data
    method: GET
    upstream: http://localhost:3000
"#;

    let mock_server = MockServer::start().await;
    let config_content = config.replace("http://localhost:3000", mock_server.uri().as_str());
    let config_path = write_temp_config(&config_content);
    let config = Config::from_file(&config_path).expect("load config");

    let metrics = Arc::new(Metrics::new().expect("create metrics"));
    let app_state = AppState {
        config,
        schema_cache: Arc::new(tokio::sync::RwLock::new(SchemaCache::new())),
        openapi_cache: Arc::new(tokio::sync::RwLock::new(OpenApiCache::new())),
        http_client: build_http_client(),
        metrics: metrics.clone(),
    };
    let shared_state = Arc::new(RwLock::new(app_state));

    Mock::given(path("/api/data"))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_string("ok")
                .insert_header("X-Upstream-Header", "value"),
        )
        .mount(&mock_server)
        .await;

    let app = axum::Router::new()
        .route("/{*path}", axum::routing::any(handler))
        .with_state(shared_state)
        .layer(PropagateRequestIdLayer::new(
            axum::http::HeaderName::from_static("x-request-id"),
        ))
        .layer(SetRequestIdLayer::new(
            axum::http::HeaderName::from_static("x-request-id"),
            MakeRequestUuid,
        ));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let port = listener.local_addr().expect("local addr").port();
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let client = Client::new();
    let response = client
        .get(format!("http://127.0.0.1:{}/api/data", port))
        .send()
        .await
        .expect("send request");

    assert_eq!(response.status(), 201);
    assert_eq!(
        response
            .headers()
            .get("X-Upstream-Header")
            .and_then(|v| v.to_str().ok()),
        Some("value")
    );
}
