# Schema Gateway MVP - Complete Task List

## Project Goals
Build a lightweight, composable schema validation proxy in Rust that:
- Validates JSON requests against JSON Schema
- Forwards requests to upstream services
- **NEW: Gracefully handles ALL errors with `forward_on_error` flag**
- Adds informative headers about validation/error status
- Follows TDD methodology throughout

## Key Behavior: `forward_on_error`
The gateway can forward requests to upstream even when errors occur:
- **Missing schema file** - Schema path in config doesn't exist
- **Invalid schema file** - Schema file contains malformed JSON or invalid JSON Schema
- **Validation failure** - Request body doesn't match schema
- **Internal errors** - JSON parsing errors, IO errors, etc.

When `forward_on_error: true`:
- Log the error
- Add `X-Gateway-Error` header with error details
- Forward request to upstream anyway
- Let the upstream service handle it

When `forward_on_error: false`:
- Return appropriate HTTP error (400, 500, etc.)
- Don't forward to upstream

---

## Phase 0: Project Setup with Testing Foundation

### Initial Setup
- [x] **Initialize Rust project**
  - Run `cargo new schema-gateway --lib`
  - Create binary crate at `src/main.rs`
  - Create library crate at `src/lib.rs`
  - Add `tests/` directory for integration tests
  
- [ ] **Set up GitHub repository**
  - ✓ Initialize git repo
  - ✓ Create `.gitignore` for Rust (target/, Cargo.lock for libs)
  - Create GitHub Actions workflow for CI
  - Set up workflow to run: `cargo test`, `cargo clippy`, `cargo fmt --check`

### Testing Dependencies
- [x] **Add dev dependencies to Cargo.toml**
  - `wiremock = "0.6"` - Mock HTTP upstream servers
  - `insta = "1"` - Snapshot testing for error messages
  - `assert_matches = "1.5"` - Better assertion syntax
  - `tower = { version = "0.4", features = ["util"] }` - For testing axum handlers

- [ ] **Set up TDD workflow**
  - Add `cargo-watch` for development: `cargo install cargo-watch`
  - Create script: `cargo watch -x test -x clippy`
  - Document TDD workflow in README

---

## Phase 1: Core Validation Engine (TDD)

### Schema Loading
**Goal**: Load JSON Schema files from disk with comprehensive error handling

- [x] **Write test**: `test_load_valid_schema()`
  - Given: Valid JSON Schema file at path
  - When: Load schema
  - Then: Returns compiled JSONSchema successfully

- [x] **Write test**: `test_load_missing_schema()`
  - Given: Path to non-existent file
  - When: Load schema
  - Then: Returns `Error::SchemaNotFound` with path

- [x] **Write test**: `test_load_invalid_json()`
  - Given: File contains malformed JSON
  - When: Load schema
  - Then: Returns `Error::InvalidSchema` with parse error

- [x] **Write test**: `test_load_invalid_schema_syntax()`
  - Given: Valid JSON but invalid JSON Schema (e.g., unknown keyword)
  - When: Compile schema
  - Then: Returns `Error::InvalidSchema` with schema compilation error

- [x] **Write test**: `test_schema_caching()`
  - Given: Schema loaded once
  - When: Load same schema again
  - Then: Returns cached version (test with mock to ensure file only read once)

- [x] **Implement**: `SchemaCache` struct in `src/schema.rs`
  - `HashMap<PathBuf, Arc<JSONSchema>>` for cache
  - `load()` method that checks cache first
  - Proper error types for all failure modes

### JSON Validation
**Goal**: Validate JSON payloads against schemas with detailed error reporting

- [x] **Write test**: `test_valid_json_passes()`
  - Given: JSON matching schema requirements
  - When: Validate
  - Then: Returns `ValidationResult { valid: true, errors: [] }`

- [x] **Write test**: `test_missing_required_field()`
  - Given: JSON missing required field
  - When: Validate
  - Then: Returns `ValidationResult { valid: false, errors: ["field 'name' is required"] }`

- [x] **Write test**: `test_wrong_type()`
  - Given: Field has wrong type (string instead of number)
  - When: Validate
  - Then: Returns error with type mismatch details

- [x] **Write test**: `test_nested_validation()`
  - Given: Nested object with validation errors
  - When: Validate
  - Then: Returns error with full path (e.g., "user.address.zipcode")

- [x] **Write test**: `test_array_validation()`
  - Given: Array with invalid items
  - When: Validate
  - Then: Returns errors for invalid array items

- [x] **Write test**: `test_multiple_validation_errors()`
  - Given: JSON with multiple violations
  - When: Validate
  - Then: Returns all errors, not just first one

- [x] **Implement**: `validation.rs` module
  - `ValidationResult` struct with `valid: bool` and `errors: Vec<String>`
  - `validate(schema, json) -> ValidationResult` function
  - Format error messages to be human-readable

### Error Types & Formatting
**Goal**: Clear, actionable error messages for all failure modes

- [x] **Write test**: `test_error_includes_field_path()`
  - Given: Nested field validation failure
  - When: Format error
  - Then: Error includes JSONPath to field

- [x] **Write test**: `test_error_includes_expected_vs_actual()`
  - Given: Type mismatch error
  - When: Format error
  - Then: Shows "expected number, got string"

- [ ] **Write test**: `test_snapshot_validation_errors()`
  - Use `insta` for snapshot testing
  - Create fixtures with various validation failures
  - Snapshot the formatted error output
  - Ensures error messages don't regress

- [x] **Implement**: `error.rs` module
  - Define all error types with `thiserror`
  - Implement `Display` for human-readable messages
  - Implement `IntoResponse` for HTTP error responses

---

## Phase 2: Configuration System (TDD)

### Config Structure
**Goal**: YAML configuration with global defaults and per-route overrides

- [x] **Write test**: `test_parse_minimal_config()`
  - Given: YAML with just routes
  - When: Parse config
  - Then: Uses global defaults

- [x] **Write test**: `test_parse_full_config()`
  - Given: YAML with global config and route overrides
  - When: Parse config
  - Then: Parses all fields correctly

- [x] **Write test**: `test_reject_empty_routes()`
  - Given: Config with empty routes array
  - When: Validate config
  - Then: Returns validation error

- [x] **Write test**: `test_reject_invalid_method()`
  - Given: Route with method "INVALID"
  - When: Validate config
  - Then: Returns error about invalid HTTP method

- [x] **Write test**: `test_reject_empty_upstream()`
  - Given: Route with empty upstream URL
  - When: Validate config
  - Then: Returns validation error

- [x] **Implement**: `config.rs` module with structs:
  ```rust
  struct Config {
    routes: Vec<Route>,
    global: GlobalConfig,
  }
  
  struct Route {
    path: String,
    method: String,
    schema: Option<PathBuf>,
    upstream: String,
    config: RouteConfig,
  }
  
  struct GlobalConfig {
    forward_on_error: bool,      // default: true
    add_error_header: bool,      // default: true
    add_validation_header: bool, // default: true
  }
  
  struct RouteConfig {
    forward_on_error: bool,      // default: uses global
    add_error_header: bool,      // default: uses global
    add_validation_header: bool, // default: uses global
  }
  ```

### Route Matching
**Goal**: Match incoming requests to configured routes

- [x] **Write test**: `test_match_exact_path()`
  - Given: Route "/api/users", request "/api/users"
  - When: Find route
  - Then: Returns matching route

- [x] **Write test**: `test_match_with_path_param()`
  - Given: Route "/api/users/:id", request "/api/users/123"
  - When: Find route
  - Then: Returns matching route

- [x] **Write test**: `test_match_correct_method()`
  - Given: POST route, GET request
  - When: Find route
  - Then: Returns None (method mismatch)

- [x] **Write test**: `test_no_match_returns_none()`
  - Given: No matching routes
  - When: Find route
  - Then: Returns None

- [x] **Write test**: `test_multiple_params()`
  - Given: Route "/api/:resource/:id/comments/:comment_id"
  - When: Match against real path
  - Then: Matches correctly

- [x] **Implement**: Route matching logic
  - Split paths by '/' and compare segments
  - Treat segments starting with ':' as wildcards
  - Case-insensitive method matching

### Config Merging
**Goal**: Per-route config overrides global config

- [x] **Write test**: `test_route_overrides_global()`
  - Given: Global `forward_on_error: true`, Route `forward_on_error: false`
  - When: Get effective config
  - Then: Route config wins (returns false)

- [x] **Write test**: `test_route_uses_global_default()`
  - Given: Global `add_error_header: false`, Route doesn't specify
  - When: Get effective config
  - Then: Uses global default (false)

- [x] **Implement**: `get_effective_config()` function
  - Merge route config with global config
  - Route values override global values

---

## Phase 3: HTTP Proxy with Error Handling (TDD)

### Request Handling
**Goal**: Extract and validate request bodies

- [x] **Write test**: `test_extract_json_body()`
  - Given: Request with JSON body
  - When: Extract body
  - Then: Returns parsed JSON

- [x] **Write test**: `test_handle_empty_body()`
  - Given: Request with no body
  - When: Process request
  - Then: Skips validation, forwards request

- [x] **Write test**: `test_handle_invalid_json()`
  - Given: Request with malformed JSON
  - When: Parse body
  - Then: Returns error OR forwards with error header (if forward_on_error)

- [x] **Write test**: `test_validate_and_forward()`
  - Given: Valid request matching schema
  - When: Process request
  - Then: Validates, adds X-Schema-Validated header, forwards

- [x] **Write test**: `test_forward_on_validation_failure()`
  - Given: Invalid request, `forward_on_error: true`
  - When: Process request
  - Then: Adds X-Gateway-Error header, forwards anyway

- [x] **Write test**: `test_reject_on_validation_failure()`
  - Given: Invalid request, `forward_on_error: false`
  - When: Process request
  - Then: Returns 400 with error details, doesn't forward

- [x] **Implement**: Request handler in `src/main.rs`
  - Extract method, path, headers, body
  - Find matching route
  - Load schema (if configured)
  - Validate body (if schema exists)
  - Handle errors according to config
  - Forward or reject

### Forward on Error Scenarios
**Goal**: Test all error scenarios with forward_on_error

- [x] **Write test**: `test_forward_on_missing_schema()`
  - Given: Schema path doesn't exist, `forward_on_error: true`
  - When: Process request
  - Then: Logs warning, adds X-Gateway-Error, forwards

- [x] **Write test**: `test_reject_on_missing_schema()`
  - Given: Schema path doesn't exist, `forward_on_error: false`
  - When: Process request
  - Then: Returns 500, doesn't forward

- [x] **Write test**: `test_forward_on_invalid_schema()`
  - Given: Schema file is malformed, `forward_on_error: true`
  - When: Process request
  - Then: Adds error header with "Invalid schema", forwards

- [x] **Write test**: `test_forward_on_read_body_error()`
  - Given: Body reading fails, `forward_on_error: true`
  - When: Process request
  - Then: Forwards with empty body and error header

- [x] **Write test**: `test_error_header_content()`
  - Given: Various error scenarios
  - When: Forward with error
  - Then: X-Gateway-Error contains descriptive message
  - Examples:
    - "Schema not found: ./schemas/user.json"
    - "Invalid schema: unexpected token at line 5"
    - "Validation failed: field 'email' is required"
    - "Invalid JSON: expected value at line 1 column 12"

### Proxying
**Goal**: Forward valid requests to upstream services

- [x] **Write test**: `test_forward_request_to_upstream()`
  - Given: Valid request
  - When: Forward
  - Then: Upstream receives request with same method, path, body

- [x] **Write test**: `test_preserve_headers()`
  - Given: Request with custom headers
  - When: Forward
  - Then: Upstream receives headers (except Host, Connection)

- [x] **Write test**: `test_add_validation_header()`
  - Given: Successfully validated request, `add_validation_header: true`
  - When: Forward
  - Then: Upstream receives X-Schema-Validated: true

- [x] **Write test**: `test_dont_add_validation_header_when_disabled()`
  - Given: `add_validation_header: false`
  - When: Forward
  - Then: No X-Schema-Validated header

- [x] **Write test**: `test_return_upstream_response()`
  - Given: Upstream returns 201 with body
  - When: Forward
  - Then: Client receives 201 with same body

- [x] **Write test**: `test_handle_upstream_connection_failure()`
  - Given: Upstream is unreachable
  - When: Forward
  - Then: Returns 502 Bad Gateway

- [x] **Write test**: `test_handle_upstream_timeout()`
  - Given: Upstream doesn't respond in time
  - When: Forward
  - Then: Returns 504 Gateway Timeout

- [x] **Implement**: `proxy.rs` module
  - HTTP client using `reqwest`
  - Forward method/path/headers/body to upstream
  - Handle connection errors gracefully
  - Stream response back to client

### Integration Tests
**Goal**: End-to-end testing of full request flow

- [x] **Write test**: `test_full_flow_valid_request()`
  - Given: Config with schema, mock upstream
  - When: Send valid POST request
  - Then: Upstream receives request, client gets response

- [x] **Write test**: `test_full_flow_invalid_request_reject()`
  - Given: `forward_on_error: false`
  - When: Send invalid request
  - Then: Returns 400, upstream not called

- [x] **Write test**: `test_full_flow_invalid_request_forward()`
  - Given: `forward_on_error: true`
  - When: Send invalid request
  - Then: Upstream receives request with error header

- [x] **Write test**: `test_full_flow_no_schema()`
  - Given: Route without schema configured
  - When: Send request
  - Then: Forwards without validation

- [x] **Write test**: `test_full_flow_missing_schema_forward()`
  - Given: Schema path doesn't exist, `forward_on_error: true`
  - When: Send request
  - Then: Upstream receives request with X-Gateway-Error

- [x] **Write test**: `test_route_not_found()`
  - Given: Request to unconfigured path
  - When: Send request
  - Then: Returns 404

- [x] **Write test**: `test_method_not_allowed()`
  - Given: Route configured for POST, send GET
  - When: Send request
  - Then: Returns 405

---

## Phase 4: CLI Interface (TDD)

### Argument Parsing
**Goal**: Clean CLI with sensible defaults

- [x] **Write test**: `test_default_arguments()`
  - Given: No arguments
  - When: Parse CLI
  - Then: Uses defaults (config.yml, port 8080)

- [x] **Write test**: `test_custom_config_path()`
  - Given: `--config custom.yml`
  - When: Parse CLI
  - Then: Uses custom config path

- [x] **Write test**: `test_custom_port()`
  - Given: `--port 3000`
  - When: Parse CLI
  - Then: Binds to port 3000

- [x] **Write test**: `test_validate_config_mode()`
  - Given: `--validate-config`
  - When: Run
  - Then: Validates config and exits (doesn't start server)

- [x] **Implement**: CLI with `clap`
  ```
  schema-gateway [OPTIONS]
  
  OPTIONS:
    -c, --config <FILE>     Path to config file [default: config.yml]
    -p, --port <PORT>       Port to listen on [default: 8080]
    --validate-config       Validate config and exit
    -h, --help              Print help
    -V, --version           Print version
  ```

### Config Validation Mode
**Goal**: Validate config before deploying

- [x] **Write test**: `test_validate_good_config_exits_success()`
  - Given: Valid config file
  - When: Run with --validate-config
  - Then: Prints "Config valid", exits 0

- [x] **Write test**: `test_validate_bad_config_exits_error()`
  - Given: Invalid config (missing required field)
  - When: Run with --validate-config
  - Then: Prints error details, exits 1

- [x] **Write test**: `test_validate_missing_schema_warns()`
  - Given: Config references non-existent schema
  - When: Validate
  - Then: Prints warning but allows (since forward_on_error)

- [x] **Implement**: Validation mode
  - Load config
  - Try to load all schemas
  - Report errors/warnings
  - Exit with appropriate code

### Logging
**Goal**: Observable operations for debugging

- [x] **Write test**: `test_log_validation_failure()`
  - Given: Request fails validation
  - When: Process
  - Then: Logs at WARN level with details

- [x] **Write test**: `test_log_missing_schema()`
  - Given: Schema file not found
  - When: Process
  - Then: Logs at WARN level

- [x] **Write test**: `test_log_upstream_error()`
  - Given: Upstream connection fails
  - When: Forward
  - Then: Logs at ERROR level

- [x] **Write test**: `test_respect_log_level()`
  - Given: RUST_LOG=error
  - When: Validation fails (WARN)
  - Then: Not logged

- [x] **Implement**: Structured logging with `tracing`
  - Log all validation failures (WARN)
  - Log all schema loading errors (WARN/ERROR)
  - Log upstream errors (ERROR)
  - Log successful requests (DEBUG)
  - Use structured fields: method, path, error, upstream

---

## Phase 5: Documentation & Distribution

### Documentation
- [ ] **Write README.md**
  - Quick start example
  - Installation instructions
  - Configuration reference
  - Explain `forward_on_error` behavior with examples
  - Link to example configs

- [ ] **Create example configs**
  - `examples/strict.yml` - Reject on any error
  - `examples/permissive.yml` - Forward on all errors
  - `examples/hybrid.yml` - Strict for /api/v1, permissive for /api/beta

- [ ] **Create example schemas**
  - `examples/schemas/user.json`
  - `examples/schemas/post.json`
  - Show common patterns (nested objects, arrays, enums)

- [ ] **Document error header format**
  - What information is included
  - How upstream services should parse it
  - Example: `X-Gateway-Error: Validation failed: field 'email' is required`

- [ ] **Write CONTRIBUTING.md**
  - How to run tests
  - TDD workflow
  - Code style guidelines
  - PR process

### Packaging
- [ ] **Set up GitHub Actions CI**
  - Run tests on push/PR
  - Run clippy with warnings as errors
  - Run `cargo fmt --check`
  - Test on Linux, macOS, Windows

- [ ] **Create release workflow**
  - Trigger on git tag
  - Build binaries for: Linux x86_64, macOS x86_64, macOS ARM64, Windows x86_64
  - Upload to GitHub Releases
  - Generate changelog from commits

- [ ] **Create Docker image**
  - Multi-stage build (rust builder, minimal runtime)
  - Published to Docker Hub
  - Include example config in image
  - Document volume mounts for configs/schemas

- [ ] **Create installation script**
  - `curl https://install.schema-gateway.dev | sh`
  - Detects OS/arch
  - Downloads appropriate binary
  - Installs to ~/.local/bin or /usr/local/bin

- [ ] **Optional: Homebrew formula**
  - Create tap repository
  - Write formula
  - Submit to homebrew-core (after adoption)

### Testing & Quality
- [ ] **Load testing**
  - Use `wrk` or `k6` to test throughput
  - Measure latency overhead vs direct upstream
  - Test with/without validation
  - Document results in README

- [ ] **Memory profiling**
  - Ensure no memory leaks
  - Test with many concurrent requests
  - Profile schema cache memory usage

- [ ] **Edge cases testing**
  - Very large JSON payloads (multi-MB)
  - Very complex schemas (deep nesting)
  - Thousands of routes
  - Malformed configs
  - Binary content (should reject gracefully)

---

## Out of Scope for MVP

These are explicitly NOT included to keep the project focused:

- ❌ Authentication/Authorization
- ❌ Rate limiting
- ❌ Request/response transformations
- ❌ Metrics/observability integrations (Prometheus, etc.)
- ❌ Schema hot-reloading
- ❌ OpenAPI support (JSON Schema only)
- ❌ GraphQL validation
- ❌ WASM/edge deployment (native binary first)
- ❌ WebSocket support
- ❌ gRPC support
- ❌ Custom validation rules beyond JSON Schema
- ❌ Web UI/dashboard
- ❌ Database/persistence

---

## Example Configs

### Strict Mode (Reject Errors)
```yaml
# Fail fast - any error stops the request
global:
  forward_on_error: false
  add_error_header: false
  add_validation_header: true

routes:
  - path: /api/users
    method: POST
    schema: ./schemas/user.json
    upstream: http://backend:3000
```

### Permissive Mode (Forward Everything)
```yaml
# Beta service - let upstream handle everything
global:
  forward_on_error: true
  add_error_header: true
  add_validation_header: true

routes:
  - path: /api/beta/:resource
    method: POST
    schema: ./schemas/beta-resource.json  # May not exist yet
    upstream: http://beta-backend:3000
```

### Hybrid Mode (Per-Route Control)
```yaml
# Different behavior for stable vs experimental endpoints
global:
  forward_on_error: false
  add_error_header: true
  add_validation_header: true

routes:
  # Strict validation for production API
  - path: /api/v1/users
    method: POST
    schema: ./schemas/v1/user.json
    upstream: http://prod-backend:3000
    # Uses global: forward_on_error: false
  
  # Permissive for experimental API
  - path: /api/experimental/:resource
    method: POST
    schema: ./schemas/experimental.json
    upstream: http://experimental-backend:3000
    config:
      forward_on_error: true  # Override global
```

---

## Example Test Scenarios

### Scenario: Missing Schema with forward_on_error
```rust
#[tokio::test]
async fn test_missing_schema_forwards_with_error_header() {
    let config = r#"
global:
  forward_on_error: true
  add_error_header: true

routes:
  - path: /api/users
    method: POST
    schema: ./nonexistent.json
    upstream: MOCK_SERVER
"#;

    let (app, mock_server) = create_test_app(config).await;

    // Expect upstream to receive request
    Mock::given(method("POST"))
        .and(path("/api/users"))
        .and(header_exists("X-Gateway-Error"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&mock_server)
        .await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/users")
                .body(Body::from(r#"{"name":"test"}"#))
                .unwrap()
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
}
```

### Scenario: Validation Failure with Rejection
```rust
#[tokio::test]
async fn test_validation_failure_rejects() {
    let schema = json!({
        "type": "object",
        "required": ["email"],
        "properties": {
            "email": {"type": "string", "format": "email"}
        }
    });

    let config = format!(r#"
global:
  forward_on_error: false

routes:
  - path: /api/users
    method: POST
    schema: {}
    upstream: MOCK_SERVER
"#, write_temp_schema(&schema));

    let (app, mock_server) = create_test_app(&config).await;

    // Upstream should NOT be called
    Mock::given(method("POST"))
        .and(path("/api/users"))
        .respond_with(ResponseTemplate::new(201))
        .expect(0)  // Should not be called!
        .mount(&mock_server)
        .await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/users")
                .body(Body::from(r#"{"name":"test"}"#))  // Missing email
                .unwrap()
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    
    let body = response.into_body();
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    
    assert!(json["error"].as_str().unwrap().contains("email"));
}
```

---

## Success Criteria

The MVP is complete when:

1. ✅ All tests pass (unit + integration)
2. ✅ Can validate JSON requests against JSON Schema
3. ✅ Can forward valid requests to upstream
4. ✅ `forward_on_error` works for all error types
5. ✅ Error headers contain useful debugging info
6. ✅ CLI works with sensible defaults
7. ✅ Documentation is clear and complete
8. ✅ Can be installed with single command
9. ✅ Binary runs standalone (no dependencies)
10. ✅ Performs well under load (>1000 req/s)

## Estimated Timeline

- Phase 0: 2 days
- Phase 1: 3-4 days
- Phase 2: 2-3 days
- Phase 3: 4-5 days
- Phase 4: 2 days
- Phase 5: 3-4 days

**Total: ~2-3 weeks** for a solid, well-tested MVP