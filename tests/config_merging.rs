use schema_gateway::config::Config;

#[test]
fn test_route_overrides_global() {
    // Global says forward_on_error: true, but route overrides to false
    let yaml = r#"
global:
  forward_on_error: true
  add_error_header: true
  add_validation_header: false

routes:
  - path: /api/users
    method: POST
    upstream: http://backend:3000
    config:
      forward_on_error: false
      add_error_header: false
"#;

    let config: Config = serde_yaml::from_str(yaml).expect("parse config");
    let route = &config.routes[0];

    // Get the effective config for this route
    let effective = config.get_effective_config(route);

    // Route overrides should win
    assert!(
        !effective.forward_on_error,
        "expected route override for forward_on_error"
    );
    assert!(
        !effective.add_error_header,
        "expected route override for add_error_header"
    );

    // Route didn't specify add_validation_header, should use global
    assert!(
        !effective.add_validation_header,
        "expected global default for add_validation_header"
    );
}

#[test]
fn test_route_uses_global_default() {
    // Route doesn't specify any overrides, should use all global values
    let yaml = r#"
global:
  forward_on_error: false
  add_error_header: false
  add_validation_header: true

routes:
  - path: /api/users
    method: POST
    upstream: http://backend:3000
"#;

    let config: Config = serde_yaml::from_str(yaml).expect("parse config");
    let route = &config.routes[0];

    // Get the effective config for this route
    let effective = config.get_effective_config(route);

    // All values should come from global
    assert!(
        !effective.forward_on_error,
        "expected global value for forward_on_error"
    );
    assert!(
        !effective.add_error_header,
        "expected global value for add_error_header"
    );
    assert!(
        effective.add_validation_header,
        "expected global value for add_validation_header"
    );
}
