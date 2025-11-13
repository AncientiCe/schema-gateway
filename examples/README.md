# Schema Gateway Examples & Demo

This directory contains example configurations, schemas, and demo scripts to showcase the Schema Gateway's capabilities.

## Quick Start Demo

### 1. Start the Mock Upstream Server

In Terminal 1:

```bash
python3 examples/mock-upstream.py
```

The mock server will start on `http://localhost:3001` and echo back all requests with validation headers.

### 2. Start the Schema Gateway

In Terminal 2:

```bash
cargo run --release -- --config examples/demo-config.yml --port 8080
```

The gateway will start on `http://localhost:8080` and forward requests to the upstream.

### 3. Run the Interactive Demo

In Terminal 3:

```bash
./examples/demo.sh
```

This will run through 5 test scenarios demonstrating:
- ✅ Valid request with successful validation
- ❌ Invalid request rejection (strict mode)
- ⚠️  Invalid request forwarding (permissive mode)
- 🔄 Passthrough without validation
- 🔗 Path parameter matching

## Example Configurations

### `demo-config.yml`
Demonstration configuration with multiple routes showcasing different validation behaviors:
- `/api/users` - Strict validation (reject on error)
- `/api/beta/users` - Permissive validation (forward on error)
- `/api/posts` - Post creation with complex schema
- `/api/health` - No validation (passthrough)
- `/api/users/:id` - Path parameters

### `strict.yml`
Production-ready configuration that rejects all invalid requests:
```yaml
global:
  forward_on_error: false
  add_error_header: false
  add_validation_header: true
```

### `permissive.yml`
Beta/experimental configuration that forwards all requests:
```yaml
global:
  forward_on_error: true
  add_error_header: true
  add_validation_header: true
```

### `hybrid.yml`
Mixed configuration with per-route behavior:
- Production routes: Strict validation
- Beta routes: Permissive validation
- Admin routes: Strict with no error headers (security)

## Example Schemas

### `schemas/user.json`
Comprehensive user schema demonstrating:
- Required fields (`email`, `username`, `name`)
- Nested objects (`name.first`, `name.last`, `address`)
- String formats and patterns (email, username, zipcode)
- Arrays with enums (`roles`)
- Min/max constraints
- Type validation

### `schemas/post.json`
Blog post schema demonstrating:
- Complex nested structures (`author`, `comments`)
- Date-time formats
- Enums (`status`: draft, published, archived)
- Array validation with constraints
- Metadata objects

## Manual Testing

### Test Valid User Creation

```bash
curl -X POST http://localhost:8080/api/users \
  -H "Content-Type: application/json" \
  -d '{
    "email": "alice@example.com",
    "username": "alice123",
    "name": {
      "first": "Alice",
      "last": "Smith"
    },
    "age": 30,
    "roles": ["user"]
  }'
```

**Expected**: 201 Created with `X-Schema-Validated: true` header

### Test Invalid User (Missing Email)

```bash
curl -X POST http://localhost:8080/api/users \
  -H "Content-Type: application/json" \
  -d '{
    "username": "bob123",
    "name": {
      "first": "Bob",
      "last": "Jones"
    }
  }'
```

**Expected**: 400 Bad Request with validation error details

### Test Permissive Mode

```bash
curl -X POST http://localhost:8080/api/beta/users \
  -H "Content-Type: application/json" \
  -d '{
    "username": "charlie"
  }'
```

**Expected**: 201 Created with `X-Gateway-Error` header containing validation details

### Test Passthrough (No Validation)

```bash
curl -X GET http://localhost:8080/api/health
```

**Expected**: 200 OK, no validation performed

## Load Testing

### Using the Provided Script

```bash
./examples/load-test.sh
```

This script uses Apache Bench (ab) to test:
- Health endpoint throughput (no validation)
- User creation throughput (with validation)

### Using wrk (Advanced)

Install wrk:
```bash
# macOS
brew install wrk

# Linux
sudo apt-get install wrk
```

Run load test:
```bash
wrk -t4 -c100 -d30s --latency \
  -s examples/wrk-post.lua \
  http://localhost:8080/api/users
```

### Using k6 (Advanced)

Install k6:
```bash
# macOS
brew install k6

# Linux
sudo apt-key adv --keyserver hkp://keyserver.ubuntu.com:80 --recv-keys C5AD17C747E3415A3642D57D77C6C491D6AC1D69
echo "deb https://dl.k6.io/deb stable main" | sudo tee /etc/apt/sources.list.d/k6.list
sudo apt-get update
sudo apt-get install k6
```

Create k6 script (`k6-test.js`):
```javascript
import http from 'k6/http';
import { check } from 'k6';

export const options = {
  vus: 10,
  duration: '30s',
};

export default function() {
  const url = 'http://localhost:8080/api/users';
  const payload = JSON.stringify({
    email: 'test@example.com',
    username: 'testuser',
    name: {
      first: 'Test',
      last: 'User'
    },
    roles: ['user']
  });

  const params = {
    headers: {
      'Content-Type': 'application/json',
    },
  };

  const res = http.post(url, payload, params);
  
  check(res, {
    'status is 201': (r) => r.status === 201,
    'has validation header': (r) => r.headers['X-Schema-Validated'] === 'true',
  });
}
```

Run k6:
```bash
k6 run k6-test.js
```

## Testing Different Scenarios

### 1. Schema Not Found (Permissive Mode)

Modify `demo-config.yml` to point to a non-existent schema:
```yaml
- path: /api/test
  method: POST
  schema: ./does-not-exist.json
  upstream: http://localhost:3001
  config:
    forward_on_error: true
```

Test:
```bash
curl -X POST http://localhost:8080/api/test \
  -H "Content-Type: application/json" \
  -d '{"data": "anything"}'
```

**Expected**: Request forwarded with `X-Gateway-Error: Schema not found` header

### 2. Invalid Schema File

Create an invalid schema file and test error handling.

### 3. Complex Nested Validation

```bash
curl -X POST http://localhost:8080/api/posts \
  -H "Content-Type: application/json" \
  -d '{
    "title": "My First Post",
    "body": "This is the content",
    "author": {
      "id": 1,
      "username": "alice"
    },
    "status": "published",
    "tags": ["rust", "gateway", "validation"]
  }'
```

### 4. Type Mismatch Errors

```bash
curl -X POST http://localhost:8080/api/users \
  -H "Content-Type: application/json" \
  -d '{
    "email": "valid@example.com",
    "username": "test",
    "name": {
      "first": "Test",
      "last": "User"
    },
    "age": "thirty"
  }'
```

**Expected**: 400 with error about `age` expecting integer, got string

## Observing Gateway Behavior

### View Logs

The gateway logs to stdout with structured logging:

```bash
RUST_LOG=debug cargo run --release -- --config examples/demo-config.yml
```

Log levels:
- `ERROR` - Upstream connection failures, critical errors
- `WARN` - Validation failures, schema loading errors
- `INFO` - Server startup, route configuration
- `DEBUG` - Successful validations, request details

### Monitor Upstream

The mock upstream server shows all received headers, including:
- `X-Schema-Validated: true` - Request was validated successfully
- `X-Gateway-Error: <message>` - Validation or schema error occurred

## Performance Expectations

Based on Rust + Tokio async architecture:

- **Without validation**: 5,000+ req/s (minimal overhead)
- **With validation**: 1,000-3,000 req/s (depends on schema complexity)
- **Schema caching**: First request compiles schema, subsequent requests use cached version
- **Memory usage**: ~10-50MB base + schema cache
- **Latency overhead**: <1ms for simple schemas, 1-5ms for complex schemas

## Troubleshooting

### Port Already in Use

```bash
# Find process using port
lsof -ti:8080

# Kill process
kill -9 $(lsof -ti:8080)
```

### Python Script Not Executing

```bash
chmod +x examples/mock-upstream.py
```

### Connection Refused

Ensure both servers are running:
1. Mock upstream on port 3001
2. Gateway on port 8080

### No Response from Gateway

Check gateway logs for errors:
```bash
RUST_LOG=info cargo run -- --config examples/demo-config.yml
```

## Next Steps

1. **Create Your Own Schemas**: Add `.json` files to `examples/schemas/`
2. **Customize Configuration**: Modify `demo-config.yml` for your use case
3. **Integrate with Your Backend**: Change `upstream` URLs to your actual services
4. **Deploy to Production**: Use the provided configurations as templates

## Resources

- [JSON Schema Documentation](https://json-schema.org/)
- [JSON Schema Validator](https://www.jsonschemavalidator.net/) - Test schemas online
- [Schema Examples](https://json-schema.org/learn/miscellaneous-examples.html)
- [wrk Documentation](https://github.com/wg/wrk)
- [k6 Documentation](https://k6.io/docs/)

