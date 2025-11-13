#!/bin/bash
# Schema Gateway Demo Script
# Demonstrates the gateway's capabilities with real requests

set -e

GATEWAY_PORT=8080
GATEWAY_URL="http://localhost:${GATEWAY_PORT}"

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║           Schema Gateway - Interactive Demo                   ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo "This demo showcases the Schema Gateway's key features:"
echo "  ✓ JSON Schema validation"
echo "  ✓ Request forwarding"
echo "  ✓ Error handling modes (strict vs permissive)"
echo "  ✓ Validation headers"
echo ""
echo -e "${YELLOW}Prerequisites:${NC}"
echo "  1. Mock upstream server running on port 3001"
echo "  2. Schema Gateway running on port ${GATEWAY_PORT}"
echo ""
echo -e "${YELLOW}To start the servers:${NC}"
echo "  Terminal 1: python3 examples/mock-upstream.py"
echo "  Terminal 2: cargo run --release -- --config examples/demo-config.yml --port ${GATEWAY_PORT}"
echo ""
read -p "Press Enter when both servers are running..."

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}Test 1: Valid User Creation (Should Succeed)${NC}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Sending valid user data..."
echo ""

RESPONSE=$(curl -s -w "\nHTTP_CODE:%{http_code}" \
  -X POST "${GATEWAY_URL}/api/users" \
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
  }')

HTTP_CODE=$(echo "$RESPONSE" | grep "HTTP_CODE" | cut -d: -f2)
BODY=$(echo "$RESPONSE" | sed '/HTTP_CODE/d')

echo -e "Response Code: ${GREEN}${HTTP_CODE}${NC}"
echo "Response Body:"
echo "$BODY" | jq '.' 2>/dev/null || echo "$BODY"
echo ""

if echo "$BODY" | jq -e '.gateway_validated == "true"' > /dev/null 2>&1; then
    echo -e "${GREEN}✓ Request was validated by gateway${NC}"
else
    echo -e "${YELLOW}⚠ Validation status unclear${NC}"
fi

read -p "Press Enter to continue..."

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${RED}Test 2: Invalid User (Missing Required Field - Should Fail)${NC}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Sending invalid user data (missing 'email')..."
echo ""

RESPONSE=$(curl -s -w "\nHTTP_CODE:%{http_code}" \
  -X POST "${GATEWAY_URL}/api/users" \
  -H "Content-Type: application/json" \
  -d '{
    "username": "bob123",
    "name": {
      "first": "Bob",
      "last": "Jones"
    }
  }')

HTTP_CODE=$(echo "$RESPONSE" | grep "HTTP_CODE" | cut -d: -f2)
BODY=$(echo "$RESPONSE" | sed '/HTTP_CODE/d')

echo -e "Response Code: ${RED}${HTTP_CODE}${NC}"
echo "Response Body:"
echo "$BODY" | jq '.' 2>/dev/null || echo "$BODY"
echo ""

if [ "$HTTP_CODE" = "400" ]; then
    echo -e "${GREEN}✓ Gateway correctly rejected invalid request (400 Bad Request)${NC}"
else
    echo -e "${YELLOW}⚠ Unexpected status code${NC}"
fi

read -p "Press Enter to continue..."

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${YELLOW}Test 3: Permissive Mode (Forward Despite Validation Error)${NC}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Sending invalid data to /api/beta/users (forward_on_error: true)..."
echo ""

RESPONSE=$(curl -s -w "\nHTTP_CODE:%{http_code}" \
  -X POST "${GATEWAY_URL}/api/beta/users" \
  -H "Content-Type: application/json" \
  -d '{
    "username": "charlie"
  }')

HTTP_CODE=$(echo "$RESPONSE" | grep "HTTP_CODE" | cut -d: -f2)
BODY=$(echo "$RESPONSE" | sed '/HTTP_CODE/d')

echo -e "Response Code: ${GREEN}${HTTP_CODE}${NC}"
echo "Response Body:"
echo "$BODY" | jq '.' 2>/dev/null || echo "$BODY"
echo ""

if echo "$BODY" | jq -e '.gateway_error' > /dev/null 2>&1; then
    ERROR=$(echo "$BODY" | jq -r '.gateway_error')
    echo -e "${GREEN}✓ Request forwarded with error header:${NC}"
    echo -e "  ${YELLOW}${ERROR}${NC}"
else
    echo -e "${YELLOW}⚠ No error header found${NC}"
fi

read -p "Press Enter to continue..."

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}Test 4: Health Check (No Validation)${NC}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Sending GET request to /api/health (no schema configured)..."
echo ""

RESPONSE=$(curl -s -w "\nHTTP_CODE:%{http_code}" \
  -X GET "${GATEWAY_URL}/api/health")

HTTP_CODE=$(echo "$RESPONSE" | grep "HTTP_CODE" | cut -d: -f2)
BODY=$(echo "$RESPONSE" | sed '/HTTP_CODE/d')

echo -e "Response Code: ${GREEN}${HTTP_CODE}${NC}"
echo "Response Body:"
echo "$BODY" | jq '.' 2>/dev/null || echo "$BODY"
echo ""
echo -e "${GREEN}✓ Request proxied without validation${NC}"

read -p "Press Enter to continue..."

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}Test 5: Path Parameters${NC}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Sending GET request to /api/users/123..."
echo ""

RESPONSE=$(curl -s -w "\nHTTP_CODE:%{http_code}" \
  -X GET "${GATEWAY_URL}/api/users/123")

HTTP_CODE=$(echo "$RESPONSE" | grep "HTTP_CODE" | cut -d: -f2)
BODY=$(echo "$RESPONSE" | sed '/HTTP_CODE/d')

echo -e "Response Code: ${GREEN}${HTTP_CODE}${NC}"
echo "Response Body:"
echo "$BODY" | jq '.' 2>/dev/null || echo "$BODY"
echo ""
echo -e "${GREEN}✓ Path parameter matched correctly${NC}"

read -p "Press Enter to continue..."

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${BLUE}Demo Complete!${NC}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Key Features Demonstrated:"
echo -e "  ${GREEN}✓${NC} JSON Schema validation (Draft 2020-12)"
echo -e "  ${GREEN}✓${NC} Strict validation mode (reject on error)"
echo -e "  ${GREEN}✓${NC} Permissive mode (forward with error headers)"
echo -e "  ${GREEN}✓${NC} Routes without validation (passthrough)"
echo -e "  ${GREEN}✓${NC} Path parameter matching"
echo -e "  ${GREEN}✓${NC} Informative error messages"
echo -e "  ${GREEN}✓${NC} Validation status headers"
echo ""
echo "Next steps:"
echo "  - Try modifying examples/demo-config.yml"
echo "  - Create your own schemas in examples/schemas/"
echo "  - Run load tests with: ./examples/load-test.sh"
echo ""

