use axum::body::Body;
use axum::extract::State;
use axum::http::{Method, Request, StatusCode};
use schema_gateway::config::Config;
use schema_gateway::handler::{build_http_client, handle_request, AppState};
use schema_gateway::openapi::{OpenApiCache, ResponseKey};
use schema_gateway::schema::SchemaCache;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn write_openapi_spec(contents: &str) -> PathBuf {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("openapi.yaml");
    fs::write(&path, contents).expect("write spec");
    let _ = Box::leak(Box::new(dir));
    path
}

#[test]
fn test_openapi_cache_reuses_schema() {
    let spec = r#"
openapi: 3.0.0
info:
  title: Demo
  version: "1.0.0"
paths:
  /api/users:
    post:
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: ["email"]
              properties:
                email:
                  type: string
"#;

    let path = write_openapi_spec(spec);
    let mut cache = OpenApiCache::new();

    let plan1 = cache
        .load_operation(&path, "/api/users", &Method::POST, None)
        .expect("load operation");
    assert!(plan1.body_required);
    let schema1 = plan1.schema.expect("has schema");

    let plan2 = cache
        .load_operation(&path, "/api/users", &Method::POST, None)
        .expect("load operation");
    let schema2 = plan2.schema.expect("has schema");

    assert!(Arc::ptr_eq(&schema1, &schema2));
}

#[test]
fn test_openapi_operation_id_lookup() {
    let spec = r#"
openapi: 3.0.0
info:
  title: Demo
  version: "1.0.0"
paths:
  /api/users/{id}:
    get:
      operationId: getUser
      requestBody:
        required: false
        content:
          application/json:
            schema:
              type: object
              properties:
                verbose:
                  type: boolean
"#;

    let path = write_openapi_spec(spec);
    let mut cache = OpenApiCache::new();
    let plan = cache
        .load_operation(&path, "/api/users/:id", &Method::GET, Some("getUser"))
        .expect("load operation");

    assert!(!plan.body_required);
    assert!(plan.schema.is_some());
}

#[test]
fn test_operation_plan_includes_parameters_and_responses() {
    let spec = r#"
openapi: 3.0.0
info:
  title: Demo
  version: "1.0.0"
paths:
  /api/items/{id}:
    get:
      parameters:
        - name: id
          in: path
          required: true
          schema:
            type: string
        - name: verbose
          in: query
          schema:
            type: boolean
      responses:
        '200':
          description: ok
          content:
            application/json:
              schema:
                type: object
                properties:
                  id:
                    type: string
"#;

    let path = write_openapi_spec(spec);
    let mut cache = OpenApiCache::new();
    let plan = cache
        .load_operation(&path, "/api/items/:id", &Method::GET, None)
        .expect("load operation");

    assert_eq!(plan.parameters.len(), 2);
    assert!(plan
        .response_schemas
        .contains_key(&ResponseKey::Status(200)));
}

#[tokio::test]
async fn test_openapi_validation_rejects_invalid_body() -> TestResult {
    let spec_path = write_openapi_spec(
        r#"
openapi: 3.0.0
info:
  title: Demo
  version: "1.0.0"
paths:
  /api/users:
    post:
      requestBody:
        required: true
        content:
          application/json:
            schema:
              type: object
              required: ["email"]
              properties:
                email:
                  type: string
components:
  schemas: {}
"#,
    );

    let config_yaml = format!(
        r#"
global:
  forward_on_error: false

routes:
  - path: /api/users
    method: POST
    openapi: {}
    upstream: http://backend:3000
"#,
        spec_path.display()
    );

    let config: Config = serde_yaml::from_str(&config_yaml)?;
    let app_state = AppState {
        config,
        schema_cache: SchemaCache::new(),
        openapi_cache: OpenApiCache::new(),
        http_client: build_http_client(),
        metrics: std::sync::Arc::new(schema_gateway::metrics::Metrics::new().unwrap()),
    };

    let state = Arc::new(RwLock::new(app_state));
    let request_body = json!({ "name": "Bob" }).to_string();
    let request = Request::builder()
        .method(Method::POST)
        .uri("/api/users")
        .header("content-type", "application/json")
        .body(Body::from(request_body))?;

    let (parts, body) = request.into_parts();
    let response = handle_request(State(state), parts.method, parts.uri, parts.headers, body).await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn test_openapi_query_parameter_validation() -> TestResult {
    let spec_path = write_openapi_spec(
        r#"
openapi: 3.0.0
info:
  title: Demo
  version: "1.0.0"
paths:
  /api/items:
    get:
      parameters:
        - name: limit
          in: query
          required: true
          schema:
            type: integer
      responses:
        '200':
          description: ok
          content:
            application/json:
              schema:
                type: array
                items:
                  type: string
components:
  schemas: {}
"#,
    );

    let config_yaml = format!(
        r#"
global:
  forward_on_error: false

routes:
  - path: /api/items
    method: GET
    openapi: {}
    upstream: http://backend:3000
"#,
        spec_path.display()
    );

    let config: Config = serde_yaml::from_str(&config_yaml)?;
    let app_state = AppState {
        config,
        schema_cache: SchemaCache::new(),
        openapi_cache: OpenApiCache::new(),
        http_client: build_http_client(),
        metrics: std::sync::Arc::new(schema_gateway::metrics::Metrics::new().unwrap()),
    };

    let state = Arc::new(RwLock::new(app_state));
    let request = Request::builder()
        .method(Method::GET)
        .uri("/api/items?limit=abc")
        .body(Body::empty())?;

    let (parts, body) = request.into_parts();
    let response = handle_request(State(state), parts.method, parts.uri, parts.headers, body).await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    Ok(())
}
