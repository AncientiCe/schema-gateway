// k6 Load Testing Script for Schema Gateway
import http from 'k6/http';
import { check, group, sleep } from 'k6';
import { Rate, Trend } from 'k6/metrics';

// Custom metrics
const validationSuccessRate = new Rate('validation_success');
const validationErrorRate = new Rate('validation_errors');
const gatewayLatency = new Trend('gateway_latency');

// Test configuration
export const options = {
  stages: [
    { duration: '30s', target: 50 },   // Ramp up to 50 users
    { duration: '1m', target: 100 },   // Stay at 100 users
    { duration: '30s', target: 200 },  // Spike to 200 users
    { duration: '1m', target: 100 },   // Scale back down
    { duration: '30s', target: 0 },    // Ramp down to 0
  ],
  thresholds: {
    http_req_duration: ['p(95)<500'],           // 95% of requests under 500ms
    http_req_failed: ['rate<0.1'],              // Less than 10% errors
    validation_success: ['rate>0.9'],           // 90%+ validation success
  },
};

const BASE_URL = 'http://gateway:8080';

export default function() {
  group('Health Check (No Validation)', function() {
    const res = http.get(`${BASE_URL}/api/health`);
    
    check(res, {
      'health check status is 200': (r) => r.status === 200,
    });
    
    sleep(1);
  });

  group('Valid User Creation', function() {
    const payload = JSON.stringify({
      email: `user-${__VU}-${__ITER}@example.com`,
      username: `user_${__VU}_${__ITER}`,
      name: {
        first: 'Test',
        last: 'User'
      },
      age: 25 + (__VU % 50),
      roles: ['user']
    });

    const params = {
      headers: {
        'Content-Type': 'application/json',
      },
    };

    const res = http.post(`${BASE_URL}/api/users`, payload, params);
    
    const validated = res.headers['X-Schema-Validated'] === 'true';
    validationSuccessRate.add(validated);
    gatewayLatency.add(res.timings.duration);

    check(res, {
      'valid user status is 201': (r) => r.status === 201,
      'has validation header': (r) => r.headers['X-Schema-Validated'] === 'true',
      'upstream received request': (r) => {
        try {
          const body = JSON.parse(r.body);
          return body.gateway_validated === 'true';
        } catch {
          return false;
        }
      },
    });

    sleep(1);
  });

  group('Invalid User (Strict Mode)', function() {
    const payload = JSON.stringify({
      username: 'invalid_user'  // Missing required email
    });

    const params = {
      headers: {
        'Content-Type': 'application/json',
      },
    };

    const res = http.post(`${BASE_URL}/api/users`, payload, params);
    
    check(res, {
      'invalid user status is 400': (r) => r.status === 400,
      'has error message': (r) => {
        try {
          const body = JSON.parse(r.body);
          return body.error && body.error.includes('email');
        } catch {
          return false;
        }
      },
    });

    sleep(1);
  });

  group('Invalid User (Permissive Mode)', function() {
    const payload = JSON.stringify({
      username: 'permissive_test'  // Missing required email
    });

    const params = {
      headers: {
        'Content-Type': 'application/json',
      },
    };

    const res = http.post(`${BASE_URL}/api/beta/users`, payload, params);
    
    const hasError = res.headers['X-Gateway-Error'] !== undefined;
    validationErrorRate.add(hasError);

    check(res, {
      'permissive status is 201': (r) => r.status === 201,
      'has error header': (r) => r.headers['X-Gateway-Error'] !== undefined,
      'request was forwarded': (r) => {
        try {
          const body = JSON.parse(r.body);
          return body.gateway_error !== null;
        } catch {
          return false;
        }
      },
    });

    sleep(1);
  });

  group('Complex Post Creation', function() {
    const payload = JSON.stringify({
      title: `Test Post ${__VU}-${__ITER}`,
      body: 'This is a test post with some content.',
      author: {
        id: __VU,
        username: `user_${__VU}`
      },
      status: 'published',
      tags: ['test', 'k6', 'load-testing']
    });

    const params = {
      headers: {
        'Content-Type': 'application/json',
      },
    };

    const res = http.post(`${BASE_URL}/api/posts`, payload, params);
    
    check(res, {
      'post creation status is 201': (r) => r.status === 201,
      'validation succeeded': (r) => r.headers['X-Schema-Validated'] === 'true',
    });

    sleep(1);
  });
}

export function handleSummary(data) {
  return {
    'stdout': textSummary(data, { indent: ' ', enableColors: true }),
    '/tmp/k6-summary.json': JSON.stringify(data),
  };
}

function textSummary(data, options) {
  const indent = options.indent || '';
  const enableColors = options.enableColors || false;
  
  let summary = '\n' + indent + '═══════════════════════════════════════════════════\n';
  summary += indent + '  k6 Load Test Summary - Schema Gateway\n';
  summary += indent + '═══════════════════════════════════════════════════\n\n';
  
  summary += indent + `Total Requests: ${data.metrics.http_reqs.values.count}\n`;
  summary += indent + `Request Rate: ${data.metrics.http_reqs.values.rate.toFixed(2)} req/s\n`;
  summary += indent + `Duration: ${(data.state.testRunDurationMs / 1000).toFixed(2)}s\n\n`;
  
  summary += indent + 'HTTP Metrics:\n';
  summary += indent + `  Avg Duration: ${data.metrics.http_req_duration.values.avg.toFixed(2)}ms\n`;
  summary += indent + `  P95 Duration: ${data.metrics.http_req_duration.values['p(95)'].toFixed(2)}ms\n`;
  summary += indent + `  P99 Duration: ${data.metrics.http_req_duration.values['p(99)'].toFixed(2)}ms\n`;
  summary += indent + `  Failed Requests: ${(data.metrics.http_req_failed.values.rate * 100).toFixed(2)}%\n\n`;
  
  summary += indent + 'Validation Metrics:\n';
  summary += indent + `  Validation Success Rate: ${(data.metrics.validation_success.values.rate * 100).toFixed(2)}%\n`;
  summary += indent + `  Validation Error Rate: ${(data.metrics.validation_errors.values.rate * 100).toFixed(2)}%\n`;
  summary += indent + `  Gateway Latency (avg): ${data.metrics.gateway_latency.values.avg.toFixed(2)}ms\n\n`;
  
  return summary;
}

