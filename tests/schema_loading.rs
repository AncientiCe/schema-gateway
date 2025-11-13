use std::fs;
use std::path::PathBuf;

use schema_gateway::schema::SchemaCache;

fn write_temp_schema_file(contents: &str) -> PathBuf {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("schema.json");
    fs::write(&path, contents).expect("write temp schema");
    // Keep the tempdir alive by leaking it for the duration of the test process.
    // This avoids the file being deleted before the cache reads it.
    let _ = Box::leak(Box::new(dir));
    path
}

#[test]
fn test_load_valid_schema() {
    // A minimal valid JSON Schema
    let schema_json = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {"name": {"type": "string"}},
        "required": ["name"]
    }"#;

    let path = write_temp_schema_file(schema_json);

    let mut cache = SchemaCache::new();
    let compiled = cache.load(&path);

    assert!(
        compiled.is_ok(),
        "expected schema to compile, got: {:?}",
        compiled
    );
}

#[test]
fn test_load_missing_schema() {
    let mut cache = SchemaCache::new();
    let missing = PathBuf::from("/definitely/does/not/exist.json");
    let err = cache.load(&missing).unwrap_err();
    let msg = format!("{}", err);
    assert!(
        msg.contains("Schema not found"),
        "unexpected error: {}",
        msg
    );
}

#[test]
fn test_load_invalid_json() {
    let path = write_temp_schema_file("{ this is not valid json }");
    let mut cache = SchemaCache::new();
    let err = cache.load(&path).unwrap_err();
    let msg = format!("{}", err);
    assert!(
        msg.contains("Invalid schema JSON"),
        "unexpected error: {}",
        msg
    );
}

#[test]
fn test_load_invalid_schema_syntax() {
    // Valid JSON but invalid JSON Schema (unknown keyword type value)
    let schema_json = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "not-a-valid-type"
    }"#;
    let path = write_temp_schema_file(schema_json);
    let mut cache = SchemaCache::new();
    let err = cache.load(&path).unwrap_err();
    let msg = format!("{}", err);
    assert!(
        msg.contains("Invalid schema syntax"),
        "unexpected error: {}",
        msg
    );
}

#[test]
fn test_schema_caching() {
    let schema_json = r#"{
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object"
    }"#;
    let path = write_temp_schema_file(schema_json);

    let mut cache = SchemaCache::new();
    let first = cache.load(&path).expect("compile first");
    // Replace file contents to ensure we don't re-read/compile if cached
    fs::write(&path, "{}\n").expect("overwrite schema");
    let second = cache.load(&path).expect("compile second from cache");

    assert!(
        std::sync::Arc::ptr_eq(&first, &second),
        "expected same Arc from cache"
    );
}
