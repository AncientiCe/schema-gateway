# Multi-stage Dockerfile for Schema Gateway

# Stage 1: Build
FROM rust:1.75-slim as builder

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src
COPY tests ./tests

# Build release binary
RUN cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/schema-gateway /usr/local/bin/schema-gateway

# Copy example configs and schemas
COPY examples/strict.yml /app/config/strict.yml
COPY examples/permissive.yml /app/config/permissive.yml
COPY examples/hybrid.yml /app/config/hybrid.yml
COPY examples/demo-config.yml /app/config/demo-config.yml
COPY examples/schemas /app/schemas

# Create volume mount points
VOLUME ["/app/config", "/app/schemas"]

# Expose default port
EXPOSE 8080

# Default command
ENTRYPOINT ["schema-gateway"]
CMD ["--config", "/app/config/demo-config.yml", "--port", "8080"]

