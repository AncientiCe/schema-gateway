#!/bin/bash
# WRK Load Testing Script for Schema Gateway

set -e

GATEWAY_URL="http://gateway:8080"
DURATION="30s"
THREADS=4
CONNECTIONS=100

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║          Schema Gateway - WRK Load Testing Suite              ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "Configuration:"
echo "  Gateway: ${GATEWAY_URL}"
echo "  Duration: ${DURATION}"
echo "  Threads: ${THREADS}"
echo "  Connections: ${CONNECTIONS}"
echo ""

# Wait for gateway to be ready
echo "Waiting for gateway..."
until curl -sf "${GATEWAY_URL}/api/health" > /dev/null; do
    sleep 1
done
echo "✓ Gateway is ready"
echo ""

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Test 1: Health Endpoint (No Validation)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

wrk -t${THREADS} -c${CONNECTIONS} -d${DURATION} \
    --latency \
    "${GATEWAY_URL}/api/health"

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Test 2: User Creation with Validation"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

wrk -t${THREADS} -c${CONNECTIONS} -d${DURATION} \
    --latency \
    -s /scripts/post-user.lua \
    "${GATEWAY_URL}/api/users"

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Test 3: Permissive Mode (Invalid Requests)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

wrk -t${THREADS} -c${CONNECTIONS} -d${DURATION} \
    --latency \
    -s /scripts/post-invalid.lua \
    "${GATEWAY_URL}/api/beta/users"

echo ""
echo "════════════════════════════════════════════════════════════════"
echo "Load Testing Complete!"
echo "════════════════════════════════════════════════════════════════"

