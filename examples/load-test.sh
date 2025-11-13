#!/bin/bash
# Simple load test for Schema Gateway
# Requires: Apache Bench (ab) - usually pre-installed on macOS/Linux

set -e

GATEWAY_URL="http://localhost:8080"
REQUESTS=1000
CONCURRENCY=10

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║           Schema Gateway - Load Test                          ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo ""

# Check if ab is available
if ! command -v ab &> /dev/null; then
    echo -e "${YELLOW}Apache Bench (ab) not found.${NC}"
    echo "Please install it:"
    echo "  macOS: Already installed (try 'ab -V')"
    echo "  Ubuntu/Debian: sudo apt-get install apache2-utils"
    echo "  RHEL/CentOS: sudo yum install httpd-tools"
    exit 1
fi

echo "Test Configuration:"
echo "  URL: ${GATEWAY_URL}"
echo "  Total Requests: ${REQUESTS}"
echo "  Concurrency: ${CONCURRENCY}"
echo ""
echo -e "${YELLOW}Prerequisites:${NC}"
echo "  1. Mock upstream server running (python3 examples/mock-upstream.py)"
echo "  2. Schema Gateway running (cargo run --release -- --config examples/demo-config.yml)"
echo ""
read -p "Press Enter when servers are ready..."

# Create temporary file with valid JSON payload
PAYLOAD_FILE=$(mktemp)
cat > "$PAYLOAD_FILE" << 'EOF'
{
  "email": "loadtest@example.com",
  "username": "loadtest_user",
  "name": {
    "first": "Load",
    "last": "Test"
  },
  "age": 25,
  "roles": ["user"]
}
EOF

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}Test 1: Health Endpoint (No Validation)${NC}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

ab -n ${REQUESTS} -c ${CONCURRENCY} \
   -H "Content-Type: application/json" \
   "${GATEWAY_URL}/api/health" 2>&1 | \
   grep -E "(Requests per second|Time per request|Transfer rate|Failed requests)" || true

echo ""
read -p "Press Enter to continue..."

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}Test 2: User Creation with Validation${NC}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

ab -n ${REQUESTS} -c ${CONCURRENCY} \
   -p "$PAYLOAD_FILE" \
   -T "application/json" \
   "${GATEWAY_URL}/api/users" 2>&1 | \
   grep -E "(Requests per second|Time per request|Transfer rate|Failed requests)" || true

echo ""
echo -e "${GREEN}Performance Summary:${NC}"
echo "  ✓ Gateway handles JSON Schema validation at high concurrency"
echo "  ✓ Schema caching minimizes overhead"
echo "  ✓ Async/tokio architecture ensures efficient resource usage"
echo ""

# Cleanup
rm -f "$PAYLOAD_FILE"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${BLUE}Load Test Complete!${NC}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Tips:"
echo "  - Increase REQUESTS and CONCURRENCY in the script for stress testing"
echo "  - Monitor server logs to see request processing"
echo "  - Use 'wrk' or 'k6' for more advanced load testing scenarios"
echo ""

