# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2024-12-19

### Added
- **JSON Schema Validation** - Validate requests against JSON Schema (Draft 2020-12)
- **OpenAPI 3.x Support** - Reuse existing OpenAPI specs to validate request bodies, parameters, and optional JSON response validation
- **Flexible Error Handling** - Configurable per-route behavior to forward or reject requests on validation errors
  - Strict mode: Reject invalid requests with 400/500 errors
  - Permissive mode: Forward all requests with error headers for upstream handling
  - Hybrid mode: Different behavior per route
- **Path Parameter Support** - Dynamic route matching with `:param` syntax (e.g., `/api/users/:id`)
- **Informative Headers** - Add validation status and error details to forwarded requests
  - `X-Schema-Validated` header on successful validation
  - `X-Gateway-Error` header with descriptive error messages
- **CLI Interface** - Command-line tool with options for:
  - Config file path (`--config`)
  - Port configuration (`--port`)
  - Config validation (`--validate-config`)
- **Structured Logging** - Integration with `tracing` crate, configurable via `RUST_LOG` environment variable
- **Prometheus Metrics** - Comprehensive metrics endpoint at `/metrics` including:
  - HTTP request metrics (total, duration, status codes)
  - Validation metrics (attempts, successes, failures by type)
  - Upstream metrics (requests, duration, errors)
  - Schema cache metrics (hits, misses)
  - Route not found metrics
- **Health Check Endpoints** - Three health check endpoints for Kubernetes and monitoring:
  - `/health` - Basic health check
  - `/health/ready` - Readiness probe
  - `/health/live` - Liveness probe
- **YAML Configuration** - Simple YAML-based configuration with sensible defaults
- **Schema Caching** - In-memory caching of compiled schemas for performance
- **Comprehensive Test Suite** - Full test coverage using TDD methodology
- **Docker Support** - Dockerfile and docker-compose setup for easy deployment
- **Example Configurations** - Three example configs (strict, permissive, hybrid) with demo schemas
- **Documentation** - Complete README with usage examples, configuration reference, and architecture overview

### Performance
- Built with Rust, Tokio, and Axum for high performance
- Expected performance: >1000 req/s per core with validation enabled
- Zero-copy JSON parsing where possible
- Async I/O for non-blocking operations

### Technical Details
- Written in Rust (edition 2021)
- Uses `jsonschema` crate for JSON Schema validation
- Uses `axum` for HTTP server
- Uses `reqwest` for upstream proxying
- Uses `prometheus` crate for metrics
- CI/CD pipeline with GitHub Actions
- Cross-platform support (Linux, macOS, Windows)

[0.1.0]: https://github.com/AncientiCe/schema-gateway/releases/tag/v0.1.0
