# Docker & Advanced Testing Suite

This directory contains Docker configurations and advanced testing tools for the Schema Gateway.

## Quick Start

### Basic Demo with Docker Compose

Start the entire stack (gateway + upstream):

```bash
docker-compose up
```

This starts:
- **Mock Upstream** on port 3001
- **Schema Gateway** on port 8080

Test it:
```bash
curl -X POST http://localhost:8080/api/users \
  -H "Content-Type: application/json" \
  -d '{
    "email": "test@example.com",
    "username": "testuser",
    "name": {"first": "Test", "last": "User"},
    "roles": ["user"]
  }'
```

### Advanced Load Testing

#### Option 1: WRK Load Testing

```bash
docker-compose --profile testing up wrk-test
```

This runs comprehensive load tests using wrk:
- Health endpoint (no validation)
- User creation (with validation)
- Permissive mode (invalid requests)

**Expected Results:**
- Health endpoint: >5000 req/s
- With validation: >1000 req/s
- P95 latency: <10ms

#### Option 2: K6 Load Testing

```bash
docker-compose --profile testing up k6-test
```

This runs sophisticated k6 tests with:
- Ramping VU stages
- Multiple test scenarios
- Custom metrics tracking
- Performance thresholds

**Features:**
- Validates response codes
- Tracks validation headers
- Measures latency percentiles
- Tests error handling modes

### Memory Profiling

Run valgrind for memory leak detection:

```bash
docker-compose --profile profiling up valgrind
```

This will:
- Build gateway with debug symbols
- Run under valgrind
- Check for memory leaks
- Report memory usage patterns

## Docker Images

### Main Gateway Image

Build the gateway:
```bash
docker build -t schema-gateway:latest .
```

Run standalone:
```bash
docker run -p 8080:8080 \
  -v $(pwd)/examples:/app/config:ro \
  -v $(pwd)/examples/schemas:/app/schemas:ro \
  schema-gateway:latest
```

### Custom Configuration

Use your own config:
```bash
docker run -p 8080:8080 \
  -v /path/to/your/config.yml:/app/config/config.yml:ro \
  -v /path/to/your/schemas:/app/schemas:ro \
  schema-gateway:latest \
  --config /app/config/config.yml
```

## Docker Compose Services

### Base Services (Default Profile)

- `upstream` - Mock upstream server
- `gateway` - Schema Gateway

### Testing Profile

- `wrk-test` - WRK load testing
- `k6-test` - K6 load testing

### Profiling Profile

- `valgrind` - Memory profiling

## Advanced Testing Scenarios

### 1. WRK Custom Tests

Edit `wrk-scripts/run-tests.sh` to customize:
- Duration
- Thread count
- Connection count
- Test scenarios

### 2. K6 Custom Scenarios

Edit `k6-scripts/load-test.js` to:
- Add new test groups
- Modify VU stages
- Change thresholds
- Add custom metrics

### 3. Sustained Load Test

Test for 30 minutes:

```bash
# In k6-scripts/load-test.js, modify options.stages:
stages: [
  { duration: '5m', target: 100 },
  { duration: '20m', target: 100 },
  { duration: '5m', target: 0 },
]
```

### 4. Spike Testing

```bash
# In k6-scripts/load-test.js:
stages: [
  { duration: '1m', target: 100 },
  { duration: '30s', target: 500 },  // Spike
  { duration: '1m', target: 100 },
]
```

### 5. Stress Testing

Find breaking point:

```bash
# In k6-scripts/load-test.js:
stages: [
  { duration: '2m', target: 100 },
  { duration: '5m', target: 200 },
  { duration: '5m', target: 400 },
  { duration: '5m', target: 800 },
  { duration: '2m', target: 0 },
]
```

## Performance Benchmarks

### Expected Results

| Scenario | Throughput | P95 Latency | P99 Latency |
|----------|------------|-------------|-------------|
| Health (no validation) | 5000+ req/s | <5ms | <10ms |
| Simple validation | 2000+ req/s | <10ms | <20ms |
| Complex validation | 1000+ req/s | <20ms | <50ms |
| Permissive mode | 1500+ req/s | <15ms | <30ms |

### Resource Usage

- **CPU**: ~50% per core at 1000 req/s
- **Memory**: 10-50MB base + ~1MB per cached schema
- **Network**: Minimal overhead (~200 bytes headers)

## Continuous Integration

### GitHub Actions Integration

```yaml
name: Load Tests

on: [push, pull_request]

jobs:
  load-test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Run load tests
        run: |
          docker-compose up -d gateway upstream
          docker-compose --profile testing run wrk-test
          docker-compose --profile testing run k6-test
          
      - name: Collect results
        run: |
          docker-compose logs gateway
          cat /tmp/k6-summary.json
```

## Troubleshooting

### Container Won't Start

```bash
# Check logs
docker-compose logs gateway

# Check health
docker-compose ps
```

### Port Already in Use

```bash
# Change ports in docker-compose.yml
ports:
  - "9090:8080"  # Use 9090 instead
```

### Memory Issues

```bash
# Increase Docker memory limit
# Docker Desktop → Settings → Resources → Memory
```

### Performance Lower Than Expected

1. **Check Docker resources** - Ensure adequate CPU/memory
2. **Disable logging** - Set `RUST_LOG=error`
3. **Use release build** - Dockerfile uses `--release` by default
4. **Check host system load** - Close other applications

## Production Deployment

### Using Docker Swarm

```bash
docker stack deploy -c docker-compose.yml schema-gateway
```

### Using Kubernetes

Convert to K8s manifests:
```bash
kompose convert -f docker-compose.yml
```

Or use the provided Kubernetes manifests (if added).

### Environment Variables

```yaml
services:
  gateway:
    environment:
      - RUST_LOG=info
      - RUST_BACKTRACE=1  # For debugging
```

### Volume Mounts

```yaml
volumes:
  - ./config:/app/config:ro           # Read-only configs
  - ./schemas:/app/schemas:ro         # Read-only schemas
  - gateway-logs:/var/log/gateway     # Persistent logs
```

## Advanced Profiling

### CPU Profiling with perf

```bash
docker run --privileged \
  --pid=host \
  -v $(pwd):/app \
  schema-gateway:latest \
  perf record -g -- schema-gateway --config /app/config/demo-config.yml
```

### Flame Graphs

```bash
# Generate flame graph from perf data
docker run --rm -v $(pwd):/data \
  brendangregg/flamegraph \
  perf script > out.perf-folded
```

### Memory Profiling

```bash
# Detailed memory analysis
docker-compose --profile profiling up valgrind

# Check for leaks
docker logs schema-gateway-valgrind | grep "definitely lost"
```

## Monitoring

### Prometheus Integration (Future)

```yaml
services:
  prometheus:
    image: prom/prometheus
    volumes:
      - ./prometheus.yml:/etc/prometheus/prometheus.yml
    ports:
      - "9090:9090"
```

### Grafana Dashboards (Future)

```yaml
services:
  grafana:
    image: grafana/grafana
    ports:
      - "3000:3000"
```

## Best Practices

1. **Always test in Docker** - Ensures consistent environment
2. **Use multi-stage builds** - Smaller final images
3. **Pin versions** - `rust:1.75` not `rust:latest`
4. **Health checks** - Ensure services are ready
5. **Resource limits** - Prevent resource exhaustion
6. **Read-only volumes** - Security best practice
7. **Network isolation** - Use Docker networks

## Clean Up

```bash
# Stop all services
docker-compose down

# Remove volumes
docker-compose down -v

# Remove images
docker-compose down --rmi all

# Full cleanup
docker system prune -a
```

## Further Reading

- [Docker Best Practices](https://docs.docker.com/develop/dev-best-practices/)
- [WRK Documentation](https://github.com/wg/wrk)
- [K6 Documentation](https://k6.io/docs/)
- [Valgrind Manual](https://valgrind.org/docs/manual/manual.html)

