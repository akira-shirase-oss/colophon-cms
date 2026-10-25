// Load scenario for the health endpoints (step 0.3). Run it with scripts/load.sh, which
// starts the server, records the machine and commit, and keeps the results.
//
// SCENARIO=throughput  closed loop: VUS virtual users send requests back to back until
//                      exactly TOTAL requests are done. Measures how fast the server goes.
// SCENARIO=steady      open model: RATE requests per second for DURATION, whatever the
//                      server's latency. A slow server can't slow the test down, so the
//                      latency figures mean what they say. Fails if k6 itself falls behind.
//
// Every response is checked: status, body and the x-request-id header. Requests alternate
// between /livez and /readyz; /readyz must report "ready" (the stack must be up).

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
  // k6 for a few milliseconds and a handful are missed (8 of 1.2 M in a 2026-10-25 run). More
  // than 0.01% of the scheduled total means the run didn't test the rate it claims.
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

const LIVEZ = `${BASE_URL}/livez`;
const READYZ = `${BASE_URL}/readyz`;

export default function () {
  if (__ITER % 2 === 0) {
    const res = http.get(LIVEZ, { tags: { endpoint: 'livez' } });
    check(res, {
      'livez 200': (r) => r.status === 200,
      'livez body': (r) => r.body === '{"status":"live"}',
      'livez request id': (r) => (r.headers['X-Request-Id'] || '').length === 36,
    });
  } else {
    const res = http.get(READYZ, { tags: { endpoint: 'readyz' } });
    check(res, {
      'readyz 200': (r) => r.status === 200,
      'readyz ready': (r) => r.body.indexOf('"status":"ready"') !== -1,
      'readyz request id': (r) => (r.headers['X-Request-Id'] || '').length === 36,
    });
  }
}
