use schema_gateway::config::Config;

#[test]
fn test_parse_minimal_config() {
    // Minimal config: just routes, should use global defaults
    let yaml = r#"
routes:
  - path: /api/users
    method: POST
    upstream: http://backend:3000
"#;

    let config: Config = serde_yaml::from_str(yaml).expect("parse minimal config");

    assert_eq!(config.routes.len(), 1);

    let route = &config.routes[0];
    assert_eq!(route.path, "/api/users");
    assert_eq!(route.method, "POST");
    assert_eq!(route.upstream, "http://backend:3000");
    assert!(route.schema.is_none());

    // Global defaults should be present
    assert!(config.global.forward_on_error);
    assert!(config.global.add_error_header);
    assert!(config.global.add_validation_header);
}

#[test]
fn test_parse_full_config() {
    // Full config with global settings and route overrides
    let yaml = r#"
global:
  forward_on_error: false
  add_error_header: false
  add_validation_header: true

routes:
  - path: /api/users
    method: POST
    schema: ./schemas/user.json
    upstream: http://backend:3000
    config:
      forward_on_error: true
      add_error_header: true
  
  - path: /api/posts
    method: GET
    upstream: http://backend:3000
"#;

    let config: Config = serde_yaml::from_str(yaml).expect("parse full config");

    // Global config
    assert!(!config.global.forward_on_error);
    assert!(!config.global.add_error_header);
    assert!(config.global.add_validation_header);

    // Routes
    assert_eq!(config.routes.len(), 2);

    // First route with overrides
    let route1 = &config.routes[0];
    assert_eq!(route1.path, "/api/users");
    assert_eq!(route1.method, "POST");
    assert_eq!(route1.upstream, "http://backend:3000");
    assert!(route1.schema.is_some());
    assert_eq!(
        route1.schema.as_ref().unwrap().to_str().unwrap(),
        "./schemas/user.json"
    );

    // Route config overrides
    assert_eq!(route1.config.forward_on_error, Some(true));
    assert_eq!(route1.config.add_error_header, Some(true));
    assert_eq!(route1.config.add_validation_header, None);

    // Second route without overrides
    let route2 = &config.routes[1];
    assert_eq!(route2.path, "/api/posts");
    assert_eq!(route2.method, "GET");
    assert_eq!(route2.upstream, "http://backend:3000");
    assert!(route2.schema.is_none());

    // No route-level overrides
    assert_eq!(route2.config.forward_on_error, None);
    assert_eq!(route2.config.add_error_header, None);
    assert_eq!(route2.config.add_validation_header, None);
}

#[test]
fn test_reject_empty_routes() {
    // Config with empty routes array should fail validation
    let yaml = r#"
routes: []
"#;

    let result: Result<Config, _> = serde_yaml::from_str(yaml);

    // Should parse but validation should fail
    if let Ok(config) = result {
        let validation_result = config.validate();
        assert!(
            validation_result.is_err(),
            "expected validation error for empty routes"
        );
        let err = validation_result.unwrap_err();
        let err_msg = err.to_string().to_lowercase();
        assert!(
            err_msg.contains("route") || err_msg.contains("empty"),
            "error should mention route or empty, got: {}",
            err
        );
    } else {
        // Alternative: could fail at parse time if we use custom deserialize
        panic!("parsing empty routes should succeed but validation should fail");
    }
}

#[test]
fn test_reject_invalid_method() {
    // Route with invalid HTTP method should fail validation
    let yaml = r#"
routes:
  - path: /api/users
    method: INVALID
    upstream: http://backend:3000
"#;

    let result: Result<Config, _> = serde_yaml::from_str(yaml);

    if let Ok(config) = result {
        let validation_result = config.validate();
        assert!(
            validation_result.is_err(),
            "expected validation error for invalid method"
        );
        let err = validation_result.unwrap_err();
        assert!(
            err.to_string().contains("method") || err.to_string().contains("INVALID"),
            "error should mention method or INVALID, got: {}",
            err
        );
    } else {
        panic!("parsing should succeed but validation should fail for invalid method");
    }
}

#[test]
fn test_reject_empty_upstream() {
    // Route with empty upstream should fail validation
    let yaml = r#"
routes:
  - path: /api/users
    method: POST
    upstream: ""
"#;

    let result: Result<Config, _> = serde_yaml::from_str(yaml);

    if let Ok(config) = result {
        let validation_result = config.validate();
        assert!(
            validation_result.is_err(),
            "expected validation error for empty upstream"
        );
        let err = validation_result.unwrap_err();
        assert!(
            err.to_string().contains("upstream") || err.to_string().contains("empty"),
            "error should mention upstream or empty, got: {}",
            err
        );
    } else {
        panic!("parsing should succeed but validation should fail for empty upstream");
    }
}
