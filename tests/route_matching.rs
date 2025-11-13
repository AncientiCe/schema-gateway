use schema_gateway::config::{Config, Route};

fn create_route(path: &str, method: &str) -> Route {
    serde_yaml::from_str(&format!(
        r#"
path: {}
method: {}
upstream: http://backend:3000
"#,
        path, method
    ))
    .expect("create route")
}

#[test]
fn test_match_exact_path() {
    let route = create_route("/api/users", "POST");

    // Exact match should succeed
    assert!(
        route.matches("/api/users", "POST"),
        "expected exact path and method to match"
    );

    // Different path should not match
    assert!(
        !route.matches("/api/posts", "POST"),
        "expected different path to not match"
    );

    // Same path but different method should not match
    assert!(
        !route.matches("/api/users", "GET"),
        "expected different method to not match"
    );
}

#[test]
fn test_match_with_path_param() {
    let route = create_route("/api/users/:id", "GET");

    // Should match with any value for :id
    assert!(
        route.matches("/api/users/123", "GET"),
        "expected path with param to match"
    );

    assert!(
        route.matches("/api/users/abc", "GET"),
        "expected path with different param value to match"
    );

    // Should not match with different number of segments
    assert!(
        !route.matches("/api/users", "GET"),
        "expected path with missing segment to not match"
    );

    assert!(
        !route.matches("/api/users/123/extra", "GET"),
        "expected path with extra segment to not match"
    );

    // Should not match with different static segments
    assert!(
        !route.matches("/api/posts/123", "GET"),
        "expected different static segment to not match"
    );
}

#[test]
fn test_match_correct_method() {
    let post_route = create_route("/api/users", "POST");

    // Should match with same method
    assert!(
        post_route.matches("/api/users", "POST"),
        "expected POST to match POST"
    );

    // Should match case-insensitively
    assert!(
        post_route.matches("/api/users", "post"),
        "expected case-insensitive method matching"
    );

    // Should not match GET request
    assert!(
        !post_route.matches("/api/users", "GET"),
        "expected POST route to not match GET request"
    );

    // Should not match other methods
    assert!(
        !post_route.matches("/api/users", "PUT"),
        "expected POST route to not match PUT request"
    );

    assert!(
        !post_route.matches("/api/users", "DELETE"),
        "expected POST route to not match DELETE request"
    );
}

#[test]
fn test_no_match_returns_none() {
    // Create a config with a few routes
    let yaml = r#"
routes:
  - path: /api/users
    method: POST
    upstream: http://backend:3000
  
  - path: /api/posts/:id
    method: GET
    upstream: http://backend:3000
"#;

    let config: Config = serde_yaml::from_str(yaml).expect("parse config");

    // No matching path
    assert!(
        config.find_route("/api/comments", "GET").is_none(),
        "expected no match for non-existent path"
    );

    // Matching path but wrong method
    assert!(
        config.find_route("/api/users", "GET").is_none(),
        "expected no match for wrong method"
    );

    // Should match when both path and method are correct
    assert!(
        config.find_route("/api/users", "POST").is_some(),
        "expected match for correct path and method"
    );

    assert!(
        config.find_route("/api/posts/123", "GET").is_some(),
        "expected match for parameterized path"
    );
}

#[test]
fn test_multiple_params() {
    let route = create_route("/api/:resource/:id/comments/:comment_id", "GET");

    // Should match with all params filled
    assert!(
        route.matches("/api/posts/123/comments/456", "GET"),
        "expected path with multiple params to match"
    );

    assert!(
        route.matches("/api/users/abc/comments/xyz", "GET"),
        "expected path with different param values to match"
    );

    // Should not match with wrong number of segments
    assert!(
        !route.matches("/api/posts/123/comments", "GET"),
        "expected path with missing segment to not match"
    );

    assert!(
        !route.matches("/api/posts/123", "GET"),
        "expected shorter path to not match"
    );

    assert!(
        !route.matches("/api/posts/123/comments/456/extra", "GET"),
        "expected path with extra segment to not match"
    );

    // Should not match with wrong static segments
    assert!(
        !route.matches("/api/posts/123/replies/456", "GET"),
        "expected different static segment to not match"
    );
}
