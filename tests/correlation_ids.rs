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

async fn create_test_server(config_content: &str) -> (MockServer, u16) {
    // Start mock upstream server first
    let mock_server = MockServer::start().await;
    let mock_uri = mock_server.uri();

    // Replace placeholder with mock server URI
    let config_content = config_content.replace("http://localhost:3000", mock_uri.as_str());

    let config_path = write_temp_config(&config_content);
    let config = Config::from_file(&config_path).expect("load config");

    let metrics = Arc::new(Metrics::new().expect("create metrics"));
    let app_state = AppState {
        config,
        schema_cache: Arc::new(tokio::sync::RwLock::new(SchemaCache::new())),
        openapi_cache: Arc::new(tokio::sync::RwLock::new(OpenApiCache::new())),
        http_client: build_http_client(),
        metrics,
    };

    let shared_state = Arc::new(RwLock::new(app_state));

    let app = axum::Router::new()
        .route("/*path", axum::routing::any(handler))
        .with_state(shared_state)
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

async fn handler(
    axum::extract::State(state): axum::extract::State<Arc<tokio::sync::RwLock<AppState>>>,
    method: axum::http::Method,
    uri: axum::http::Uri,
    headers: axum::http::HeaderMap,
    body: axum::body::Body,
) -> axum::response::Response {
    schema_gateway::handler::handle_request(axum::extract::State(state), method, uri, headers, body)
        .await
}

#[tokio::test]
async fn test_request_id_generated_and_forwarded() {
    let config = r#"
global:
  forward_on_error: false
routes:
  - path: /api/test
    method: GET
    upstream: http://localhost:3000
"#;

    let (mock_server, port) = create_test_server(config).await;

    Mock::given(path("/api/test"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/api/test", port);

    let response = client.get(&url).send().await.expect("send request");
    assert_eq!(response.status(), 200);

    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        !request_id.is_empty(),
        "expected gateway to set x-request-id on response"
    );

    let received = mock_server
        .received_requests()
        .await
        .expect("received requests");
    assert_eq!(received.len(), 1);
    let upstream_request_id = received[0]
        .headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(
        upstream_request_id, request_id,
        "expected gateway to forward the same x-request-id upstream"
    );
}

#[tokio::test]
async fn test_request_id_propagated_when_provided() {
    let config = r#"
global:
  forward_on_error: false
routes:
  - path: /api/test
    method: GET
    upstream: http://localhost:3000
"#;

    let (mock_server, port) = create_test_server(config).await;

    Mock::given(path("/api/test"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = Client::new();
    let url = format!("http://127.0.0.1:{}/api/test", port);

    let response = client
        .get(&url)
        .header("x-request-id", "abc-123")
        .send()
        .await
        .expect("send request");
    assert_eq!(response.status(), 200);

    let response_request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(response_request_id, "abc-123");

    let received = mock_server
        .received_requests()
        .await
        .expect("received requests");
    assert_eq!(received.len(), 1);
    let upstream_request_id = received[0]
        .headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(upstream_request_id, "abc-123");
}

#[tokio::test]
async fn test_request_id_added_on_route_not_found() {
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
    let url = format!("http://127.0.0.1:{}/does/not/exist", port);

    let response = client.get(&url).send().await.expect("send request");
    assert_eq!(response.status(), 404);

    let request_id = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        !request_id.is_empty(),
        "expected gateway to set x-request-id on 404 responses"
    );
}
