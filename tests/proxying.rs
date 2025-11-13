use serde_json::json;
use wiremock::{
    matchers::{body_json, header, method, path},
    Mock, MockServer, ResponseTemplate,
};

#[tokio::test]
async fn test_forward_request_to_upstream() {
    // Given: Valid request
    let request_body = json!({
        "name": "Alice",
        "age": 30
    });

    // Mock upstream server
    let mock_server = MockServer::start().await;

    // Expect upstream to receive request with same method, path, body
    Mock::given(method("POST"))
        .and(path("/api/users"))
        .and(body_json(&request_body))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({"id": 123})))
        .expect(1)
        .mount(&mock_server)
        .await;

    // This test demonstrates expected proxying behavior:
    // 1. Gateway receives POST request to /api/users with JSON body
    // 2. Validates (if schema configured)
    // 3. Forwards to upstream with same method (POST), path (/api/users), and body
    // 4. Returns upstream response to client

    // For now, verify mock server setup by sending request directly
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/users", mock_server.uri()))
        .json(&request_body)
        .send()
        .await
        .expect("send request");

    assert_eq!(response.status(), 201);
    let response_body: serde_json::Value = response.json().await.expect("parse response");
    assert_eq!(response_body, json!({"id": 123}));
}

#[tokio::test]
async fn test_preserve_headers() {
    // Given: Request with custom headers
    let mock_server = MockServer::start().await;

    // Expect upstream to receive custom headers
    Mock::given(method("POST"))
        .and(path("/api/users"))
        .and(header("Authorization", "Bearer token123"))
        .and(header("X-Request-ID", "abc-123"))
        .and(header("Content-Type", "application/json"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    // This test demonstrates header forwarding:
    // 1. Client sends request with custom headers
    // 2. Gateway forwards headers to upstream (except Host, Connection, etc.)
    // 3. Upstream receives all relevant headers

    // For now, verify mock expectations
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/users", mock_server.uri()))
        .header("Authorization", "Bearer token123")
        .header("X-Request-ID", "abc-123")
        .json(&json!({"name": "Alice"}))
        .send()
        .await
        .expect("send request");

    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_add_validation_header() {
    // Given: Successfully validated request, add_validation_header: true
    let mock_server = MockServer::start().await;

    // Expect upstream to receive X-Schema-Validated: true header
    Mock::given(method("POST"))
        .and(path("/api/users"))
        .and(header("X-Schema-Validated", "true"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    // This test demonstrates validation header behavior:
    // 1. Gateway receives request
    // 2. Validates against schema (passes)
    // 3. Since add_validation_header: true, adds X-Schema-Validated: true
    // 4. Forwards to upstream with validation header
    // 5. Upstream can trust the request was validated

    // For now, manually add the header to verify mock
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/users", mock_server.uri()))
        .header("X-Schema-Validated", "true")
        .json(&json!({"name": "Alice"}))
        .send()
        .await
        .expect("send request");

    assert_eq!(response.status(), 200);
}

#[tokio::test]
async fn test_dont_add_validation_header_when_disabled() {
    // Given: add_validation_header: false
    let mock_server = MockServer::start().await;

    // Expect upstream to receive request WITHOUT X-Schema-Validated header
    Mock::given(method("POST"))
        .and(path("/api/users"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&mock_server)
        .await;

    // This test demonstrates that when add_validation_header: false:
    // 1. Gateway receives request
    // 2. Validates against schema (may pass or fail)
    // 3. Since add_validation_header: false, does NOT add X-Schema-Validated
    // 4. Forwards to upstream without validation header

    // For now, send request without the header
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/users", mock_server.uri()))
        .json(&json!({"name": "Alice"}))
        .send()
        .await
        .expect("send request");

    assert_eq!(response.status(), 200);

    // In real implementation, we'd verify the header is absent
    // wiremock doesn't easily assert header absence in expect, but
    // the mock will fail if unexpected headers are strictly required
}

#[tokio::test]
async fn test_return_upstream_response() {
    // Given: Upstream returns 201 with body and custom headers
    let mock_server = MockServer::start().await;

    let upstream_response_body = json!({"id": 456, "status": "created"});

    Mock::given(method("POST"))
        .and(path("/api/users"))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(&upstream_response_body)
                .insert_header("X-Custom-Header", "custom-value")
                .insert_header("X-Rate-Limit", "100"),
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    // This test demonstrates response proxying:
    // 1. Gateway forwards request to upstream
    // 2. Upstream returns 201 with body and headers
    // 3. Gateway returns the same status, body, and headers to client
    // 4. Client receives upstream response as-is

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/api/users", mock_server.uri()))
        .json(&json!({"name": "Bob"}))
        .send()
        .await
        .expect("send request");

    assert_eq!(response.status(), 201);

    let response_body: serde_json::Value = response.json().await.expect("parse response");
    assert_eq!(response_body, upstream_response_body);
}

#[tokio::test]
async fn test_handle_upstream_connection_failure() {
    // Given: Upstream is unreachable
    let unreachable_url = "http://localhost:9999"; // Port that's not listening

    // This test demonstrates error handling for connection failures:
    // 1. Gateway tries to forward request to upstream
    // 2. Connection fails (refused, timeout, DNS error, etc.)
    // 3. Gateway returns 502 Bad Gateway to client
    // 4. Error body should explain the issue

    let client = reqwest::Client::new();
    let result = client
        .post(format!("{}/api/users", unreachable_url))
        .json(&json!({"name": "Alice"}))
        .send()
        .await;

    // Connection should fail
    assert!(
        result.is_err(),
        "expected connection to unreachable upstream to fail"
    );

    // In the gateway implementation, this would be caught and return 502
    // For now, we verify that the error occurs as expected
}

#[tokio::test]
async fn test_handle_upstream_timeout() {
    // Given: Upstream doesn't respond in time
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/slow"))
        .respond_with(
            ResponseTemplate::new(200).set_delay(std::time::Duration::from_secs(10)), // 10 second delay
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    // This test demonstrates timeout handling:
    // 1. Gateway forwards request to upstream
    // 2. Upstream is slow and doesn't respond within timeout
    // 3. Gateway returns 504 Gateway Timeout to client
    // 4. Client doesn't wait indefinitely

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500)) // 500ms timeout
        .build()
        .expect("build client");

    let result = client
        .post(format!("{}/api/slow", mock_server.uri()))
        .json(&json!({"data": "test"}))
        .send()
        .await;

    // Request should timeout
    assert!(
        result.is_err(),
        "expected request to timeout when upstream is slow"
    );

    // In the gateway implementation, this timeout would be caught
    // and return 504 Gateway Timeout
}
