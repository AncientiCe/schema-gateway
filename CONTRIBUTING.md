# Contributing to Schema Gateway

Thank you for your interest in contributing to Schema Gateway! This document provides guidelines and instructions for contributing to the project.

## Development Workflow

We follow a Test-Driven Development (TDD) methodology throughout the project. All new features and bug fixes should:

1. Start with a failing test
2. Implement the minimal code to make the test pass
3. Refactor while keeping tests green
4. Ensure all tests pass before submitting

## Getting Started

### Prerequisites

- Rust 1.70 or later
- Git

### Setup

1. **Clone the repository**

```bash
git clone https://github.com/AncientiCe/schema-gateway.git
cd schema-gateway
```

2. **Build the project**

```bash
cargo build
```

3. **Run tests**

```bash
cargo test
```

4. **Watch mode for development** (optional)

```bash
# Install cargo-watch if you don't have it
cargo install cargo-watch

# Run tests automatically on file changes
cargo watch -x test -x clippy
```

## Running Tests

### Run all tests

```bash
cargo test
```

### Run specific test file

```bash
cargo test --test integration
```

### Run specific test

```bash
cargo test test_forward_on_error
```

### Run tests with output

```bash
cargo test -- --nocapture
```

### Run tests with logging

```bash
RUST_LOG=debug cargo test -- --nocapture
```

## Code Quality

Before submitting a pull request, ensure your code passes all quality checks:

### Format code

```bash
cargo fmt --all
```

### Check formatting without modifying files

```bash
cargo fmt --all --check
```

### Run linter

```bash
cargo clippy -- -D warnings
```

### Run all quality checks

```bash
cargo fmt --all --check && cargo clippy -- -D warnings && cargo test
```

## Code Style Guidelines

### General Principles

- Write clear, self-documenting code
- Keep functions small and focused
- Use meaningful variable names
- Add comments for complex logic
- Follow Rust naming conventions

### Rust-Specific Guidelines

- Use `rustfmt` for consistent formatting (run `cargo fmt`)
- Address all `clippy` warnings
- Use `Result` for error handling (avoid `unwrap()` in library code)
- Prefer `&str` over `String` for function parameters when possible
- Use `thiserror` for error types
- Use structured logging with `tracing` crate

### Testing Guidelines

- Write unit tests for individual functions
- Write integration tests for end-to-end flows
- Use descriptive test names: `test_<scenario>_<expected_behavior>`
- Follow the Given-When-Then pattern in test comments
- Test both success and failure paths
- Mock external dependencies (use `wiremock` for HTTP servers)

Example test structure:

```rust
#[tokio::test]
async fn test_validation_failure_rejects_request() {
    // Given: Config with forward_on_error: false
    let config = create_test_config(false);
    
    // When: Send request with invalid JSON
    let response = send_request(&app, invalid_json).await;
    
    // Then: Returns 400 Bad Request
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
```

## Project Structure

```
schema-gateway/
├── src/
│   ├── main.rs           # Entry point and server setup
│   ├── lib.rs            # Library exports
│   ├── cli.rs            # CLI argument parsing
│   ├── config.rs         # Configuration loading and validation
│   ├── error.rs          # Error types
│   ├── handler.rs        # Request handling logic
│   ├── proxy.rs          # HTTP proxying
│   ├── schema.rs         # Schema loading and caching
│   └── validation.rs     # JSON Schema validation
├── tests/                # Integration tests
│   ├── integration.rs
│   ├── schema_loading.rs
│   └── ...
├── examples/             # Example configurations
│   ├── strict.yml
│   ├── permissive.yml
│   ├── hybrid.yml
│   └── schemas/
│       ├── user.json
│       └── post.json
└── docs/                 # Documentation
    └── mvp_task_list.md
```

## Making Changes

### Adding a New Feature

1. **Create an issue** describing the feature
2. **Write tests** for the new feature (TDD)
3. **Implement** the feature to make tests pass
4. **Update documentation** (README, examples, etc.)
5. **Submit a pull request** referencing the issue

### Fixing a Bug

1. **Create an issue** describing the bug
2. **Write a test** that reproduces the bug
3. **Fix** the bug to make the test pass
4. **Submit a pull request** referencing the issue

### Updating Documentation

- Update README.md for user-facing changes
- Update inline documentation for code changes
- Update examples if configuration changes
- Add comments for complex logic

## Pull Request Process

1. **Fork the repository** and create a branch from `main`
2. **Make your changes** following the guidelines above
3. **Ensure all tests pass** and quality checks succeed
4. **Update documentation** as needed
5. **Commit with clear messages** describing your changes
6. **Push to your fork** and submit a pull request

### Pull Request Guidelines

- Keep PRs focused on a single feature or bug fix
- Write a clear PR description explaining:
  - What problem does this solve?
  - How does it solve it?
  - Are there any breaking changes?
- Reference related issues using `Fixes #123` or `Relates to #456`
- Ensure CI passes before requesting review
- Be responsive to feedback and questions

### Commit Message Format

Use clear, descriptive commit messages:

```
Add support for path parameter extraction

- Implement path matching with :param syntax
- Add tests for path parameter matching
- Update documentation with examples
```

Good commit messages:
- ✅ `Fix validation error handling for empty bodies`
- ✅ `Add X-Gateway-Error header to forwarded requests`
- ✅ `Update README with configuration examples`

Poor commit messages:
- ❌ `fix bug`
- ❌ `wip`
- ❌ `updates`

## Continuous Integration

All pull requests must pass CI checks:

- ✅ `cargo test` - All tests pass
- ✅ `cargo clippy -- -D warnings` - No linter warnings
- ✅ `cargo fmt --check` - Code is properly formatted

CI runs on:
- Linux (ubuntu-latest)
- macOS (macos-latest)
- Windows (windows-latest)

## Release Process

(For maintainers)

1. Update version in `Cargo.toml`
2. Update CHANGELOG.md
3. Create a git tag: `git tag v0.x.x`
4. Push tag: `git push origin v0.x.x`
5. GitHub Actions will automatically build and publish release artifacts

## Getting Help

- **Questions**: Open a GitHub Discussion
- **Bug Reports**: Open a GitHub Issue
- **Feature Requests**: Open a GitHub Issue with the "enhancement" label
- **Security Issues**: Email maintainers directly (see README)

## Code of Conduct

- Be respectful and inclusive
- Welcome newcomers and help them learn
- Focus on constructive feedback
- Assume good intentions

## License

By contributing to Schema Gateway, you agree that your contributions will be licensed under the MIT License.

## Recognition

All contributors will be recognized in the project README. Thank you for helping make Schema Gateway better!

