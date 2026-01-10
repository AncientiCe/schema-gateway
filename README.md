# Schema Gateway

A lightweight, composable schema validation proxy written in Rust that validates JSON requests against JSON Schema or OpenAPI operations before forwarding them to upstream services.

## Features

- 🔍 **JSON Schema Validation** - Validate requests against JSON Schema (Draft 2020-12)
- 📘 **OpenAPI Validation** - Reuse existing OpenAPI 3.x specs to validate request bodies and parameters, plus optional JSON response validation
- 🔀 **Flexible Error Handling** - Forward or reject on validation errors (per-route configurable)
- 📋 **Informative Headers** - Add validation status and error details to forwarded requests
- ⚡ **High Performance** - Built with Rust, Tokio, and Axum for maximum throughput
- 🎯 **Path Parameters** - Support for dynamic routes with path parameters (e.g., `/api/users/:id`)
- 🔧 **Easy Configuration** - Simple YAML-based configuration with sensible defaults
- 🧪 **Well Tested** - Comprehensive test suite with TDD methodology

## Quick Start

### Installation

```bash
# Clone the repository
git clone https://github.com/AncientiCe/schema-gateway.git
cd schema-gateway

# Build the project
cargo build --release

# The binary will be at target/release/schema-gateway
```

### Basic Usage

1. **Create a configuration file** (`config.yml`):

```yaml
global:
  forward_on_error: false      # Reject invalid requests
  add_error_header: true
  add_validation_header: true

routes:
  - path: /api/users
    method: POST
    schema: ./schemas/user.json
    upstream: http://localhost:3000
```

2. **Create a JSON Schema** (`schemas/user.json`):

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "type": "object",
  "properties": {
    "email": {"type": "string", "format": "email"},
    "username": {"type": "string", "minLength": 3}
  },
  "required": ["email", "username"]
}
```

3. **Start the gateway**:

```bash
./target/release/schema-gateway --config config.yml --port 8080
```

4. **Send requests**:

```bash
# Valid request - forwarded to upstream
curl -X POST http://localhost:8080/api/users \
  -H "Content-Type: application/json" \
  -d '{"email": "user@example.com", "username": "alice"}'

# Invalid request - rejected with 400
curl -X POST http://localhost:8080/api/users \
  -H "Content-Type: application/json" \
  -d '{"username": "bob"}'  # Missing required "email" field
```

## Configuration Reference

### Global Configuration

```yaml
global:
  # Forward requests to upstream even on validation/schema errors
  # Default: true
  forward_on_error: true

  # Add X-Gateway-Error header with error details when errors occur
  # Default: true
  add_error_header: true

  # Add X-Schema-Validated header when validation succeeds
  # Default: true
  add_validation_header: true
```

### Route Configuration

```yaml
routes:
  - path: /api/users          # Request path (supports :param placeholders)
    method: POST              # HTTP method (GET, POST, PUT, DELETE, etc.)
    schema: ./schemas/user.json  # Optional: Path to JSON Schema file
    upstream: http://backend:3000  # Upstream service URL
    config:                   # Optional: Override global config for this route
      forward_on_error: false
      add_error_header: true
      add_validation_header: true
```

### Path Parameters

The gateway supports dynamic path parameters using `:param` syntax:

```yaml
routes:
  - path: /api/users/:id
    method: GET
    upstream: http://backend:3000

  - path: /api/:resource/:id/comments/:comment_id
    method: POST
    schema: ./schemas/comment.json
    upstream: http://backend:3000
```

#### OpenAPI Routes

Instead of referencing a raw JSON Schema file, a route can point at an OpenAPI document. The gateway will load the spec, resolve the matching operation, and validate JSON request bodies using the operation's `requestBody`.

```yaml
routes:
  - path: /api/users
    method: POST
    openapi: ./specs/api.yaml      # Path to OpenAPI 3.x document (YAML or JSON)
    upstream: http://backend:3000

  - path: /api/users/:id
    method: GET
    openapi:
      spec: ./specs/api.yaml
      operation_id: getUser        # Optional: explicitly choose an operationId
    upstream: http://backend:3000
```

Notes:

- Routes may use either `schema` **or** `openapi`, but not both.
- When `operation_id` is not provided, the gateway matches based on the configured path/method (with `:params` matching `{params}` in the spec).
- The OpenAPI integration validates JSON request bodies **and** path/query/header/cookie parameters. Response bodies declared under `responses[*].content` for JSON media types are also validated before being returned (and forwarded with an `X-Gateway-Error` header when permissive mode is enabled).

## Error Handling Behavior

The `forward_on_error` flag controls what happens when errors occur:

### When `forward_on_error: true` (Permissive Mode)

- **Missing schema file** → Log warning, add `X-Gateway-Error`, forward to upstream
- **Invalid schema file** → Log warning, add `X-Gateway-Error`, forward to upstream
- **Validation failure** → Log warning, add `X-Gateway-Error`, forward to upstream
- **Invalid JSON** → Log warning, add `X-Gateway-Error`, forward to upstream

The upstream service receives the request with error details in headers and can decide how to handle it.

### When `forward_on_error: false` (Strict Mode)

- **Missing schema file** → Return 500 Internal Server Error
- **Invalid schema file** → Return 500 Internal Server Error
- **Validation failure** → Return 400 Bad Request with error details
- **Invalid JSON** → Return 400 Bad Request with error details

The upstream service is not called, and the client receives an immediate error response.

## Error Header Format

When `add_error_header: true`, the gateway adds an `X-Gateway-Error` header with descriptive error messages:

### Examples

```
X-Gateway-Error: Schema not found: ./schemas/user.json
X-Gateway-Error: Invalid schema JSON in ./schemas/user.json: unexpected token at line 5
X-Gateway-Error: Validation failed: /email: 'email' is a required property
X-Gateway-Error: Invalid JSON: expected value at line 1 column 12
```

The error header contains:
- **Error type** - What kind of error occurred
- **Context** - File paths, field names, line numbers where applicable
- **Details** - Human-readable description of the problem

Upstream services can parse this header to:
- Log validation errors for monitoring
- Return custom error messages to clients
- Implement fallback behavior for certain error types

## Git Hooks (fmt + clippy)

Enable the repo-provided hooks to ensure formatting and linting run automatically:

```bash
git config core.hooksPath githooks
```

The hooks run:
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo +nightly udeps --all-targets --all-features` (if `cargo-udeps` is installed) to flag unused dependencies

## Validation Header Format

When `add_validation_header: true` and validation succeeds, the gateway adds an `X-Schema-Validated` header:

- `X-Schema-Validated: true` — JSON Schema validation
- `X-Schema-Validated: openapi` — OpenAPI operation validation

This header indicates to the upstream service that the request has been validated and can be trusted.

## Example Configurations

The `examples/` directory contains three complete configuration examples:

### [Strict Mode](examples/strict.yml)
Reject all requests with validation errors. Use for production APIs where data quality is critical.

```bash
schema-gateway --config examples/strict.yml
```

### [Permissive Mode](examples/permissive.yml)
Forward all requests to upstream, even on errors. Use for beta services or when migrating to schema validation.

```bash
schema-gateway --config examples/permissive.yml
```

### [Hybrid Mode](examples/hybrid.yml)
Different behavior per route - strict for production endpoints, permissive for experimental ones.

```bash
schema-gateway --config examples/hybrid.yml
```

## CLI Reference

```
schema-gateway [OPTIONS]

OPTIONS:
  -c, --config <FILE>     Path to config file [default: config.yml]
  -p, --port <PORT>       Port to listen on [default: 8080]
  --validate-config       Validate config and exit (doesn't start server)
  -h, --help              Print help
  -V, --version           Print version
```

### Validate Configuration

Before deploying, validate your configuration:

```bash
schema-gateway --validate-config --config config.yml
```

This checks:
- ✅ Config file syntax is valid YAML
- ✅ All required fields are present
- ✅ HTTP methods are valid
- ✅ Upstream URLs are not empty
- ⚠️  Schema files exist (warning only)

## Metrics and Observability

The gateway exposes Prometheus metrics and health check endpoints for monitoring and observability.

### Metrics Endpoint

The gateway exposes metrics in Prometheus format at `/metrics`:

```bash
curl http://localhost:8080/metrics
```

### Available Metrics

- **`http_requests_total`** - Total number of HTTP requests by method, route, and status code
- **`http_request_duration_seconds`** - Histogram of HTTP request latency
- **`validation_attempts_total`** - Total number of validation attempts by type (json_schema, openapi, none)
- **`validation_success_total`** - Total number of successful validations by type
- **`validation_failures_total`** - Total number of validation failures by type and error type
- **`upstream_requests_total`** - Total number of upstream requests by status code
- **`upstream_request_duration_seconds`** - Histogram of upstream request latency
- **`upstream_errors_total`** - Total number of upstream errors by error type
- **`schema_cache_hits_total`** - Total number of schema cache hits
- **`schema_cache_misses_total`** - Total number of schema cache misses
- **`routes_not_found_total`** - Total number of 404 responses by method

### Health Check Endpoints

The gateway provides three health check endpoints:

- **`/health`** - Basic health check (returns 200 OK if server is running)
- **`/health/ready`** - Readiness probe (returns 200 OK if server is ready to accept requests, 503 if no routes configured)
- **`/health/live`** - Liveness probe (returns 200 OK if server process is alive)

```bash
# Basic health check
curl http://localhost:8080/health

# Readiness check
curl http://localhost:8080/health/ready

# Liveness check
curl http://localhost:8080/health/live
```

### Prometheus Configuration

To scrape metrics with Prometheus, add the following to your `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'schema-gateway'
    static_configs:
      - targets: ['localhost:8080']
    metrics_path: '/metrics'
    scrape_interval: 15s
```

### Example Metrics Query

```promql
# Request rate per route
rate(http_requests_total[5m])

# Validation success rate
rate(validation_success_total[5m]) / rate(validation_attempts_total[5m])

# Average request latency
histogram_quantile(0.95, rate(http_request_duration_seconds_bucket[5m]))

# Upstream error rate
rate(upstream_errors_total[5m])
```

## Logging

The gateway uses structured logging via `tracing`. Set the log level using the `RUST_LOG` environment variable:

```bash
# Only errors
RUST_LOG=error schema-gateway

# Errors and warnings (recommended for production)
RUST_LOG=warn schema-gateway

# Info level (default)
RUST_LOG=info schema-gateway

# Debug level (for development)
RUST_LOG=debug schema-gateway
```

Log events include:
- `INFO` - Server startup, request routing
- `WARN` - Validation failures, missing schemas, invalid JSON
- `ERROR` - Upstream connection failures, internal errors
- `DEBUG` - Successful validations, request details

## Example Schemas

The `examples/schemas/` directory contains example JSON Schemas:

### [User Schema](examples/schemas/user.json)
Demonstrates:
- Required fields (`email`, `username`, `name`)
- Nested objects (`name.first`, `name.last`, `address`)
- String formats and patterns (email, username, zipcode)
- Arrays with enums (`roles`)
- Min/max constraints

### [Post Schema](examples/schemas/post.json)
Demonstrates:
- Complex nested structures (author, comments)
- Date-time formats
- Enums (`status`: draft, published, archived)
- Array validation with constraints
- Metadata objects with defaults

## Docker Deployment

### Quick Start with Docker

```bash
# Start gateway + upstream
docker-compose up

# Run advanced load tests
docker-compose --profile testing up wrk-test
docker-compose --profile testing up k6-test

# Memory profiling
docker-compose --profile profiling up valgrind
```

See [docker/README.md](docker/README.md) for comprehensive Docker guide.

### Build Docker Image

```bash
docker build -t schema-gateway:latest .
```

### Run with Custom Config

```bash
docker run -p 8080:8080 \
  -v $(pwd)/my-config.yml:/app/config/config.yml:ro \
  -v $(pwd)/my-schemas:/app/schemas:ro \
  schema-gateway:latest \
  --config /app/config/config.yml
```

## Architecture

```
┌─────────┐         ┌──────────────────┐         ┌──────────┐
│ Client  │────────▶│ Schema Gateway   │────────▶│ Upstream │
│         │◀────────│                  │◀────────│ Service  │
└─────────┘         └──────────────────┘         └──────────┘
                           │
                           ▼
                    ┌──────────────┐
                    │ JSON Schema  │
                    │ Files        │
                    └──────────────┘
```

### Request Flow

1. **Client** sends HTTP request to gateway
2. **Gateway** matches request to configured route
3. **Schema Loading** (if configured):
   - Load schema from file
   - Cache compiled schema for performance
4. **Validation** (if schema exists):
   - Parse JSON body
   - Validate against schema
   - Collect all validation errors
5. **Error Handling**:
   - If `forward_on_error: true` → Add error header, forward to upstream
   - If `forward_on_error: false` → Return error response to client
6. **Forwarding**:
   - Proxy request to upstream service
   - Add validation/error headers as configured
7. **Response**:
   - Return upstream response to client

## Performance

The gateway is designed for high performance:

- **Schema Caching** - Compiled schemas are cached in memory
- **Async I/O** - Built on Tokio for non-blocking operations
- **Zero-copy** - Minimal data copying where possible
- **Efficient JSON** - Uses `serde_json` for fast parsing

Expected performance: **>1000 req/s** per core with validation enabled.

## Live Demo & Testing

### Interactive Demo

Test the gateway's capabilities with the included demo suite:

**1. Start the mock upstream server:**
```bash
python3 examples/mock-upstream.py
```

**2. Start the gateway:**
```bash
cargo run --release -- --config examples/demo-config.yml --port 8080
```

**3. Run the interactive demo:**
```bash
./examples/demo.sh
```

The demo showcases:
- ✅ Valid request with successful validation
- ❌ Invalid request rejection (strict mode)
- ⚠️  Invalid request forwarding (permissive mode)
- 🔄 Passthrough without validation
- 🔗 Path parameter matching

### Quick Manual Test

```bash
# Valid user creation (will succeed)
curl -X POST http://localhost:8080/api/users \
  -H "Content-Type: application/json" \
  -d '{
    "email": "alice@example.com",
    "username": "alice123",
    "name": {"first": "Alice", "last": "Smith"},
    "roles": ["user"]
  }'

# Invalid user (will be rejected)
curl -X POST http://localhost:8080/api/users \
  -H "Content-Type: application/json" \
  -d '{"username": "bob"}'
```

### Load Testing

Test the gateway's performance:

```bash
./examples/load-test.sh
```

Expected performance: **>1000 req/s** with validation enabled.

See [examples/README.md](examples/README.md) for comprehensive testing guide.

## Development

### Running Tests

```bash
# Run all tests
cargo test

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_name
```

### Code Quality

```bash
# Check formatting
cargo fmt --all --check

# Run linter
cargo clippy -- -D warnings

# Run all quality checks
cargo fmt --all --check && cargo clippy -- -D warnings && cargo test
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development workflow, code style guidelines, and how to submit pull requests.

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Roadmap

Current version: **0.1.0** (MVP)

Completed:
- ✅ JSON Schema validation
- ✅ Flexible error handling
- ✅ Route matching with path parameters
- ✅ CLI interface
- ✅ Structured logging
- ✅ Metrics/observability (Prometheus)
- ✅ Health check endpoints
- ✅ OpenAPI support

Future enhancements (not in MVP):
- 🔮 Schema hot-reloading
- 🔮 Rate limiting
- 🔮 Request/response transformations

## Support

- **Issues** - Report bugs or request features via GitHub Issues
- **Discussions** - Ask questions or share ideas in GitHub Discussions
- **Documentation** - Check this README and examples for guidance
