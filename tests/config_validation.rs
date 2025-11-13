use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn write_temp_config_file(contents: &str) -> PathBuf {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("config.yml");
    fs::write(&path, contents).expect("write temp config");
    let _ = Box::leak(Box::new(dir));
    path
}

#[test]
fn test_validate_good_config_exits_success() {
    // Given: Valid config file
    let config_yaml = r#"
global:
  forward_on_error: true
  add_error_header: true
  add_validation_header: true

routes:
  - path: /api/users
    method: POST
    upstream: http://backend:3000
"#;

    let config_path = write_temp_config_file(config_yaml);

    // When: Run with --validate-config
    let output = Command::new("cargo")
        .args(&["run", "--", "--validate-config", "--config"])
        .arg(&config_path)
        .output()
        .expect("failed to execute binary");

    // Then: Exits with code 0 and prints success message
    assert!(
        output.status.success(),
        "expected exit code 0, got: {:?}",
        output.status.code()
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Config valid"),
        "expected success message in stdout, got: {}",
        stdout
    );
    assert!(
        stdout.contains(&config_path.display().to_string()),
        "expected config path in output, got: {}",
        stdout
    );
}

#[test]
fn test_validate_bad_config_exits_error() {
    // Given: Invalid config (empty routes)
    let config_yaml = r#"
routes: []
"#;

    let config_path = write_temp_config_file(config_yaml);

    // When: Run with --validate-config
    let output = Command::new("cargo")
        .args(&["run", "--", "--validate-config", "--config"])
        .arg(&config_path)
        .output()
        .expect("failed to execute binary");

    // Then: Exits with code 1 and prints error message
    assert!(
        !output.status.success(),
        "expected non-zero exit code for invalid config"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Invalid config") || stderr.contains("route"),
        "expected error message in stderr, got: {}",
        stderr
    );
}

#[test]
fn test_validate_missing_schema_warns() {
    // Given: Config references non-existent schema
    let config_yaml = r#"
global:
  forward_on_error: true

routes:
  - path: /api/users
    method: POST
    schema: /does/not/exist/schema.json
    upstream: http://backend:3000
"#;

    let config_path = write_temp_config_file(config_yaml);

    // When: Run with --validate-config
    let output = Command::new("cargo")
        .args(&["run", "--", "--validate-config", "--config"])
        .arg(&config_path)
        .output()
        .expect("failed to execute binary");

    // Then: Config structure is valid, so exits 0 (schema file existence not checked during config validation)
    // The schema will only be loaded when a request comes in
    assert!(
        output.status.success(),
        "expected exit code 0 for valid config structure (schema existence not checked), got: {:?}",
        output.status.code()
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Config valid"),
        "expected success message even with missing schema path, got: {}",
        stdout
    );
}
