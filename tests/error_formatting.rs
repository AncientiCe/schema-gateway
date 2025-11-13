use schema_gateway::schema::SchemaCache;
use schema_gateway::validation::validate;
use std::fs;
use std::path::PathBuf;

fn write_temp_schema_file(contents: &str) -> PathBuf {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("schema.json");
    fs::write(&path, contents).expect("write temp schema");
    let _ = Box::leak(Box::new(dir));
    path
}

#[test]
fn test_error_includes_field_path() {
    let schema_json = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "user": {
                "type": "object",
                "properties": {
                    "profile": {
                        "type": "object",
                        "properties": {
                            "email": {"type": "string"}
                        },
                        "required": ["email"]
                    }
                }
            }
        }
    }"#;

    let path = write_temp_schema_file(schema_json);
    let mut cache = SchemaCache::new();
    let schema = cache.load(&path).expect("load schema");

    let invalid_json = serde_json::json!({
        "user": {
            "profile": {
                "email": 12345
            }
        }
    });

    let result = validate(&schema, &invalid_json);

    assert!(!result.valid, "expected validation to fail");
    assert!(!result.errors.is_empty(), "expected at least one error");

    let error_text = result.errors.join(" ");
    // The error should include a path to the field, like "/user/profile/email"
    assert!(
        error_text.contains("user")
            && error_text.contains("profile")
            && error_text.contains("email"),
        "expected error to include full path to field, got: {:?}",
        result.errors
    );
}

#[test]
fn test_error_includes_expected_vs_actual() {
    let schema_json = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "age": {"type": "number"},
            "name": {"type": "string"}
        }
    }"#;

    let path = write_temp_schema_file(schema_json);
    let mut cache = SchemaCache::new();
    let schema = cache.load(&path).expect("load schema");

    let invalid_json = serde_json::json!({
        "age": "not a number",
        "name": 12345
    });

    let result = validate(&schema, &invalid_json);

    assert!(!result.valid, "expected validation to fail");
    assert!(!result.errors.is_empty(), "expected at least one error");

    // Errors should describe the type mismatch
    let error_text = result.errors.join(" ").to_lowercase();
    // The jsonschema crate typically includes type information in errors
    assert!(
        error_text.contains("type")
            || error_text.contains("string")
            || error_text.contains("number"),
        "expected error to describe type mismatch, got: {:?}",
        result.errors
    );
}
