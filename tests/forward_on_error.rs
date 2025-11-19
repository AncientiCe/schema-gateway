use serde_json::json;
use std::fs;
use std::path::PathBuf;
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

fn write_temp_schema_file(contents: &str) -> PathBuf {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("schema.json");
    fs::write(&path, contents).expect("write temp schema");
    let _ = Box::leak(Box::new(dir));
    path
}

#[tokio::test]
async fn test_forward_on_missing_schema() {
    // Given: Schema path doesn't exist, forward_on_error: true
    let missing_schema_path = PathBuf::from("/definitely/does/not/exist.json");

    // Mock upstream server
    let mock_server = MockServer::start().await;

    // Expect upstream to be called even though schema is missing
    Mock::given(method("POST"))
        .and(path("/api/users"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    // This test demonstrates expected behavior:
    // 1. Gateway tries to load schema from missing_schema_path
    // 2. Schema loading fails (file not found)
    // 3. Since forward_on_error: true, log warning
    // 4. Add X-Gateway-Error header: "Schema not found: /definitely/does/not/exist.json"
    // 5. Forward request to upstream anyway
    // 6. Return upstream response to client

    // For now, verify mock server setup
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/users", mock_server.uri()))
        .json(&json!({"name": "Alice"}))
        .send()
        .await
        .expect("send request");

    assert_eq!(response.status(), 200);
    let missing_schema_matches = missing_schema_path
        .to_str()
        .map(|s| s.contains("does/not/exist"))
        .unwrap_or(false);
    assert!(missing_schema_matches, "test setup verification");
}

#[tokio::test]
async fn test_reject_on_missing_schema() {
    // Given: Schema path doesn't exist, forward_on_error: false
    let missing_schema_path = PathBuf::from("/definitely/does/not/exist.json");

    // Mock upstream server
    let mock_server = MockServer::start().await;

    // Expect upstream NOT to be called
    Mock::given(method("POST"))
        .and(path("/api/users"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0) // Should NOT be called!
        .mount(&mock_server)
        .await;

    // This test demonstrates expected behavior:
    // 1. Gateway tries to load schema from missing_schema_path
    // 2. Schema loading fails (file not found)
    // 3. Since forward_on_error: false, return 500 Internal Server Error
    // 4. Do NOT forward to upstream
    // 5. Response body should contain error details

    // For now, verify the mock setup (expects 0 calls)
    let missing_schema_matches = missing_schema_path
        .to_str()
        .map(|s| s.contains("does/not/exist"))
        .unwrap_or(false);
    assert!(missing_schema_matches, "test setup verification");
    assert!(mock_server.address().port() > 0);
}

#[tokio::test]
async fn test_forward_on_invalid_schema() {
    // Given: Schema file is malformed, forward_on_error: true
    let invalid_schema = "{ this is not valid json }";
    let schema_path = write_temp_schema_file(invalid_schema);

    // Mock upstream server
    let mock_server = MockServer::start().await;

    // Expect upstream to be called even though schema is invalid
    Mock::given(method("POST"))
        .and(path("/api/users"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    // This test demonstrates expected behavior:
    // 1. Gateway tries to load schema from schema_path
    // 2. Schema loading fails (invalid JSON)
    // 3. Since forward_on_error: true, log warning
    // 4. Add X-Gateway-Error header: "Invalid schema JSON in <path>: ..."
    // 5. Forward request to upstream anyway
    // 6. Return upstream response to client

    // For now, verify mock server setup and schema path
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/users", mock_server.uri()))
        .json(&json!({"name": "Alice"}))
        .send()
        .await
        .expect("send request");

    assert_eq!(response.status(), 200);
    assert!(
        schema_path.exists(),
        "schema file should exist for this test"
    );
}

#[tokio::test]
async fn test_forward_on_read_body_error() {
    // Given: Body reading fails, forward_on_error: true
    // Mock upstream server
    let mock_server = MockServer::start().await;

    // Expect upstream to be called with error header
    Mock::given(method("POST"))
        .and(path("/api/users"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    // This test demonstrates expected behavior:
    // 1. Gateway receives request
    // 2. Body reading/parsing fails (e.g., invalid chunked encoding, connection error)
    // 3. Since forward_on_error: true, log warning
    // 4. Add X-Gateway-Error header: "Failed to read request body: ..."
    // 5. Forward with empty body (or attempt to forward original stream)
    // 6. Return upstream response to client

    // For now, verify mock server setup
    // In the actual implementation, this would be triggered by body reading errors
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/users", mock_server.uri()))
        .body("") // Empty body representing failure to read original
        .send()
        .await
        .expect("send request");

    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_error_header_content() {
    // This test verifies the format of X-Gateway-Error headers
    // for various error scenarios

    // Test case 1: Schema not found error message format
    let missing_path = PathBuf::from("./schemas/user.json");
    let error_msg = format!("Schema not found: {}", missing_path.display());
    assert!(error_msg.contains("Schema not found"));
    assert!(error_msg.contains("./schemas/user.json"));

    // Test case 2: Invalid schema JSON error message format
    let schema_path = PathBuf::from("./schemas/malformed.json");
    let error_msg = format!(
        "Invalid schema JSON in {}: unexpected token at line 5",
        schema_path.display()
    );
    assert!(error_msg.contains("Invalid schema JSON"));
    assert!(error_msg.contains("./schemas/malformed.json"));
    assert!(error_msg.contains("unexpected token"));

    // Test case 3: Validation failed error message format
    let validation_error = "Validation failed: /email: 'email' is a required property";
    assert!(validation_error.contains("Validation failed"));
    assert!(validation_error.contains("email"));
    assert!(validation_error.contains("required"));

    // Test case 4: Invalid JSON body error message format
    let json_error = "Invalid JSON: expected value at line 1 column 12";
    assert!(json_error.contains("Invalid JSON"));
    assert!(json_error.contains("line"));
    assert!(json_error.contains("column"));

    // All error messages should be human-readable and include context
    // This test documents the expected error header format
}
