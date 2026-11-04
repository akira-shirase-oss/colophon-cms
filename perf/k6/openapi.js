// Load scenario for GET /openapi.json (DX.7, D-110). Run it with scripts/load.sh, which
// starts the server, records the machine and commit, and keeps the results.
//
// SCENARIO=throughput  closed loop: VUS virtual users send requests back to back until
//                      exactly TOTAL requests are done. Measures how fast the server goes.
// SCENARIO=steady      open model: RATE requests per second for DURATION, whatever the
//                      server's latency. A slow server can't slow the test down, so the
//                      latency figures mean what they say. Fails if k6 itself falls behind.
//
// The document is computed once when the router is built, so this endpoint shares the
// health checks' profile: no database, just precomputed bytes and an ETag. Every response
// is checked: a 200 parses as OpenAPI 3.1 with every path listed, and every other request
// sends If-None-Match and expects 304.

import http from 'k6/http';
import { check } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '20000', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '25');

const scenarios = {
  throughput: {
    executor: 'shared-iterations',
    vus: VUS,
    iterations: TOTAL,
    maxDuration: '15m',
  },
  steady: {
    executor: 'constant-arrival-rate',
    rate: RATE,
    timeUnit: '1s',
    duration: DURATION,
    preAllocatedVUs: VUS,
    maxVUs: VUS * 2,
  },
};

if (!(SCENARIO in scenarios)) {
  throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}, got ${SCENARIO}`);
}

const thresholds = {
  http_req_failed: ['rate==0'],
  checks: ['rate==1'],
  http_req_duration: [`p(99)<${P99_MS}`],
};
if (SCENARIO === 'steady') {
  // Requests k6 couldn't start on schedule. On a shared machine the OS occasionally pauses
  // k6 for a few milliseconds and a handful are missed. More than 0.01% of the scheduled
  // total means the run didn't test the rate it claims.
  const scheduled = RATE * durationSeconds(DURATION);
  thresholds.dropped_iterations = [`count<=${Math.floor(scheduled * 0.0001)}`];
}

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) {
    throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  }
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  discardResponseBodies: false,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
};

const DOCUMENT = `${BASE_URL}/openapi.json`;

// The document's ETag, learned once up front: every binary serves one fixed document, so
// a matching If-None-Match must always get a 304. (k6 canonicalizes header names: `Etag`.)
export function setup() {
  const res = http.get(DOCUMENT);
  if (res.status !== 200 || !res.headers['Etag']) {
    throw new Error(`/openapi.json did not answer 200 with an ETag: ${res.status}`);
  }
  return { etag: res.headers['Etag'] };
}

export default function (data) {
  if (__ITER % 2 === 0) {
    const res = http.get(DOCUMENT, { tags: { endpoint: 'openapi' } });
    check(res, {
      'openapi 200': (r) => r.status === 200,
      'openapi is json': (r) => (r.headers['Content-Type'] || '').indexOf('application/json') === 0,
      'openapi parses': (r) => {
        try {
          const doc = r.json();
          return doc.openapi === '3.1.0' && Object.keys(doc.paths).length > 70;
        } catch (_) {
          return false;
        }
      },
      'openapi etag': (r) => r.headers['Etag'] === data.etag,
      'openapi request id': (r) => (r.headers['X-Request-Id'] || '').length === 36,
    });
  } else {
    const res = http.get(DOCUMENT, {
      headers: { 'If-None-Match': data.etag },
      tags: { endpoint: 'openapi-304' },
    });
    check(res, {
      'openapi 304': (r) => r.status === 304,
      'openapi 304 empty': (r) => !r.body || r.body.length === 0,
      'openapi 304 etag': (r) => r.headers['Etag'] === data.etag,
      'openapi 304 request id': (r) => (r.headers['X-Request-Id'] || '').length === 36,
    });
  }
}
