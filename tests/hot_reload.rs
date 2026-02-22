//! Integration tests for config/schema hot-reloading.

use reqwest::Client;
use schema_gateway::config::Config;
use schema_gateway::handler::{build_http_client, handle_request, AppState};
use schema_gateway::metrics::Metrics;
use schema_gateway::openapi::OpenApiCache;
use schema_gateway::schema::SchemaCache;
use schema_gateway::watcher;
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

async fn create_server_with_watcher(
    config_path: PathBuf,
    mock_uri: &str,
) -> (u16, PathBuf, schema_gateway::watcher::WatcherHandle) {
    let config_content = fs::read_to_string(&config_path).expect("read config");
    let config_content = config_content.replace("http://localhost:3000", mock_uri);
    fs::write(&config_path, config_content).expect("write config");

    let config = Config::from_file(&config_path).expect("load config");
    let metrics = Arc::new(Metrics::new().expect("create metrics"));
    let mut schema_cache = SchemaCache::new();
    let schema_paths: Vec<_> = config
        .routes
        .iter()
        .filter_map(|r| r.schema.clone())
        .collect();
    let _ = schema_cache.preload_all(schema_paths.iter());
    let mut openapi_cache = OpenApiCache::new();
    let _ = openapi_cache.preload_routes(&config.routes);

    let app_state = AppState {
        config,
        schema_cache: Arc::new(tokio::sync::RwLock::new(schema_cache)),
        openapi_cache: Arc::new(tokio::sync::RwLock::new(openapi_cache)),
        http_client: build_http_client(),
        metrics: metrics.clone(),
    };
    let shared_state = Arc::new(RwLock::new(app_state));

    let handle = watcher::start_watcher(config_path.clone(), shared_state.clone());

    let app =
        axum::Router::new()
            .route("/*path", axum::routing::any(handler))
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
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    (port, config_path, handle)
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

/// After modifying the schema file on disk, the gateway picks up the new validation rules.
#[tokio::test]
async fn test_hot_reload_picks_up_schema_changes() {
    let schema_dir = tempfile::tempdir().expect("create temp dir");
    let schema_path = schema_dir.path().join("schema.json");
    let schema_v1 = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "name": {"type": "string"}
        },
        "required": ["name"]
    }"#;
    fs::write(&schema_path, schema_v1).expect("write schema v1");

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

    let config_path = write_temp_config(&config);

    let mock_server = MockServer::start().await;
    Mock::given(path("/api/users"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let (_port, _config_path, _watcher_handle) =
        create_server_with_watcher(config_path, mock_server.uri().as_str()).await;
    let port = _port;

    let client = Client::new();
    let base = format!("http://127.0.0.1:{}", port);

    let ok = client
        .post(format!("{}/api/users", base))
        .json(&serde_json::json!({"name": "alice"}))
        .send()
        .await
        .expect("request");
    assert_eq!(
        ok.status(),
        200,
        "initial request with name only should pass"
    );

    let schema_v2 = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "email": {"type": "string"}
        },
        "required": ["name", "email"]
    }"#;
    fs::write(&schema_path, schema_v2).expect("write schema v2");

    // Debounce is 500ms; allow time for notify event and reload to complete
    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    let rejected = client
        .post(format!("{}/api/users", base))
        .json(&serde_json::json!({"name": "bob"}))
        .send()
        .await
        .expect("request");
    assert_eq!(
        rejected.status(),
        400,
        "after reload, request without email should be rejected"
    );
}
