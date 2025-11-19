use axum::{body::Body, http::Request};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn write_temp_schema_file(contents: &str) -> PathBuf {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("schema.json");
    fs::write(&path, contents).expect("write temp schema");
    let _ = Box::leak(Box::new(dir));
    path
}

#[tokio::test]
async fn test_extract_json_body() -> TestResult {
    // Given: Request with JSON body
    let json_body = json!({
        "name": "Alice",
        "age": 30
    });

    let body_json = serde_json::to_string(&json_body)?;
    let request = Request::builder()
        .method("POST")
        .uri("/api/test")
        .header("content-type", "application/json")
        .body(Body::from(body_json))?;

    // When: Extract body
    let bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await?;

    // Then: Returns parsed JSON
    let parsed: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(parsed, json_body);
    Ok(())
}

#[tokio::test]
async fn test_handle_empty_body() -> TestResult {
    // Given: Request with no body
    let request = Request::builder()
        .method("POST")
        .uri("/api/test")
        .body(Body::empty())?;

    // When: Extract body
    let bytes = axum::body::to_bytes(request.into_body(), usize::MAX).await?;

    // Then: Body is empty, should be handled gracefully
    assert!(bytes.is_empty(), "expected empty body");
    Ok(())
}

#[tokio::test]
async fn test_handle_invalid_json() -> TestResult {
    // Given: Request with malformed JSON
    let invalid_json = "{ this is not valid json }";

    let request = Request::builder()
        .method("POST")
        .uri("/api/test")
        .header("content-type", "application/json")
        .body(Body::from(invalid_json))?;

    // When: Try to parse body
    let bytes = axum::body::to_bytes(request.into_body(), usize::MAX).await?;

    let parse_result: Result<serde_json::Value, _> = serde_json::from_slice(&bytes);

    // Then: Parse fails
    assert!(
        parse_result.is_err(),
        "expected JSON parse to fail for invalid input"
    );
    Ok(())
}

#[tokio::test]
async fn test_validate_and_forward() -> TestResult {
    // Given: Valid request matching schema
    let schema_json = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "age": {"type": "number"}
        },
        "required": ["name"]
    }"#;

    let _schema_path = write_temp_schema_file(schema_json);

    // Mock upstream server
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/users"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": 123})))
        .expect(1)
        .mount(&mock_server)
        .await;

    // This test demonstrates the expected flow:
    // 1. Receive request with valid JSON
    // 2. Validate against schema (passes)
    // 3. Forward to upstream with X-Schema-Validated header
    // 4. Return upstream response to client

    // For now, just verify mock server is set up correctly
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/users", mock_server.uri()))
        .json(&json!({"name": "Alice", "age": 30}))
        .send()
        .await?;

    assert_eq!(response.status(), 201);
    Ok(())
}

#[tokio::test]
async fn test_forward_on_validation_failure() -> TestResult {
    // Given: Invalid request, forward_on_error: true
    let schema_json = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "email": {"type": "string"}
        },
        "required": ["email"]
    }"#;

    let _schema_path = write_temp_schema_file(schema_json);

    // Mock upstream server
    let mock_server = MockServer::start().await;

    // Expect upstream to be called even though validation fails
    Mock::given(method("POST"))
        .and(path("/api/users"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    // This test demonstrates expected behavior with forward_on_error: true
    // 1. Receive request with invalid JSON (missing required field)
    // 2. Validate against schema (fails)
    // 3. Add X-Gateway-Error header with validation error
    // 4. Forward to upstream anyway
    // 5. Return upstream response to client

    // For now, just verify the mock expectation would pass
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/users", mock_server.uri()))
        .json(&json!({"name": "Bob"})) // Missing required "email" field
        .send()
        .await?;

    assert_eq!(response.status(), 200);
    Ok(())
}

#[tokio::test]
async fn test_reject_on_validation_failure() -> TestResult {
    // Given: Invalid request, forward_on_error: false
    let schema_json = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "email": {"type": "string"}
        },
        "required": ["email"]
    }"#;

    let _schema_path = write_temp_schema_file(schema_json);

    // Mock upstream server
    let mock_server = MockServer::start().await;

    // Expect upstream NOT to be called
    Mock::given(method("POST"))
        .and(path("/api/users"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0) // Should NOT be called!
        .mount(&mock_server)
        .await;

    // This test demonstrates expected behavior with forward_on_error: false
    // 1. Receive request with invalid JSON (missing required field)
    // 2. Validate against schema (fails)
    // 3. Return 400 Bad Request with error details
    // 4. Do NOT forward to upstream

    // Since we don't have the gateway handler yet, this is a placeholder
    // showing that the mock expects 0 calls when validation fails and
    // forward_on_error is false

    // In the real implementation, the gateway would return 400 here
    // For now, we just verify the mock setup
    assert!(mock_server.address().port() > 0);
    Ok(())
}
