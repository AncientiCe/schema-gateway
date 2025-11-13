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
fn test_valid_json_passes() {
    let schema_json = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "age": {"type": "number"}
        },
        "required": ["name"]
    }"#;

    let path = write_temp_schema_file(schema_json);
    let mut cache = SchemaCache::new();
    let schema = cache.load(&path).expect("load schema");

    let valid_json = serde_json::json!({
        "name": "Alice",
        "age": 30
    });

    let result = validate(&schema, &valid_json);

    assert!(result.valid, "expected validation to pass");
    assert!(
        result.errors.is_empty(),
        "expected no errors, got: {:?}",
        result.errors
    );
}

#[test]
fn test_missing_required_field() {
    let schema_json = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "email": {"type": "string"}
        },
        "required": ["name", "email"]
    }"#;

    let path = write_temp_schema_file(schema_json);
    let mut cache = SchemaCache::new();
    let schema = cache.load(&path).expect("load schema");

    let invalid_json = serde_json::json!({
        "name": "Bob"
        // missing "email"
    });

    let result = validate(&schema, &invalid_json);

    assert!(!result.valid, "expected validation to fail");
    assert!(!result.errors.is_empty(), "expected at least one error");
    let error_text = result.errors.join(" ");
    assert!(
        error_text.contains("email"),
        "expected error to mention 'email', got: {:?}",
        result.errors
    );
}

#[test]
fn test_wrong_type() {
    let schema_json = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "age": {"type": "number"}
        },
        "required": ["age"]
    }"#;

    let path = write_temp_schema_file(schema_json);
    let mut cache = SchemaCache::new();
    let schema = cache.load(&path).expect("load schema");

    let invalid_json = serde_json::json!({
        "age": "not a number"
    });

    let result = validate(&schema, &invalid_json);

    assert!(!result.valid, "expected validation to fail");
    assert!(!result.errors.is_empty(), "expected at least one error");
    let error_text = result.errors.join(" ");
    assert!(
        error_text.contains("age"),
        "expected error to mention 'age', got: {:?}",
        result.errors
    );
}

#[test]
fn test_nested_validation() {
    let schema_json = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "user": {
                "type": "object",
                "properties": {
                    "address": {
                        "type": "object",
                        "properties": {
                            "zipcode": {"type": "string"}
                        },
                        "required": ["zipcode"]
                    }
                },
                "required": ["address"]
            }
        },
        "required": ["user"]
    }"#;

    let path = write_temp_schema_file(schema_json);
    let mut cache = SchemaCache::new();
    let schema = cache.load(&path).expect("load schema");

    let invalid_json = serde_json::json!({
        "user": {
            "address": {
                "zipcode": 12345
            }
        }
    });

    let result = validate(&schema, &invalid_json);

    assert!(!result.valid, "expected validation to fail");
    assert!(!result.errors.is_empty(), "expected at least one error");
    let error_text = result.errors.join(" ");
    assert!(
        error_text.contains("zipcode"),
        "expected error to mention 'zipcode', got: {:?}",
        result.errors
    );
}

#[test]
fn test_array_validation() {
    let schema_json = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "tags": {
                "type": "array",
                "items": {
                    "type": "string"
                }
            }
        },
        "required": ["tags"]
    }"#;

    let path = write_temp_schema_file(schema_json);
    let mut cache = SchemaCache::new();
    let schema = cache.load(&path).expect("load schema");

    let invalid_json = serde_json::json!({
        "tags": ["valid", 123, "another"]
    });

    let result = validate(&schema, &invalid_json);

    assert!(!result.valid, "expected validation to fail");
    assert!(!result.errors.is_empty(), "expected at least one error");
    let error_text = result.errors.join(" ");
    assert!(
        error_text.contains("tags"),
        "expected error to mention 'tags', got: {:?}",
        result.errors
    );
}

#[test]
fn test_multiple_validation_errors() {
    let schema_json = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "age": {"type": "number"},
            "email": {"type": "string"}
        },
        "required": ["name", "age", "email"]
    }"#;

    let path = write_temp_schema_file(schema_json);
    let mut cache = SchemaCache::new();
    let schema = cache.load(&path).expect("load schema");

    let invalid_json = serde_json::json!({
        "name": 123,
        "age": "not a number"
        // missing "email"
    });

    let result = validate(&schema, &invalid_json);

    assert!(!result.valid, "expected validation to fail");
    assert!(
        result.errors.len() >= 2,
        "expected at least 2 errors, got: {:?}",
        result.errors
    );

    let error_text = result.errors.join(" ");
    // Should report multiple issues: wrong type for name, wrong type for age, missing email
    assert!(
        error_text.contains("name") || error_text.contains("age") || error_text.contains("email"),
        "expected errors to mention fields, got: {:?}",
        result.errors
    );
}
