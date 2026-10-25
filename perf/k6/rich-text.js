// Load scenario for rich-text validation (step 0.5): real CPU work per request, unlike the
// health checks. Requests alternate between the realistic blog post (valid: must come back in
// canonical form) and a document with a javascript: link (must be refused with exactly
// unsafe_link). Same SCENARIO, TOTAL, VUS, RATE, DURATION and P99_MS settings as health.js.

import http from 'k6/http';
import { check } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '20000', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '25');

const valid = JSON.parse(open('../../fixtures/rich-text/v1/valid/12-blog-post.json'));
const invalid = JSON.parse(open('../../fixtures/rich-text/v1/invalid/12-javascript-link.json'));
const VALID_BODY = JSON.stringify(valid.value);
const INVALID_BODY = JSON.stringify(invalid.value);
const CANONICAL = `"value":${VALID_BODY}`;
const PARAMS = { headers: { 'Content-Type': 'application/json' } };
const URL = `${BASE_URL}/api/v1/rich-text/validate`;

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const scenarios = {
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '15m' },
  steady: {
    executor: 'constant-arrival-rate',
    rate: RATE,
    timeUnit: '1s',
    duration: DURATION,
    preAllocatedVUs: VUS,
    maxVUs: VUS * 2,
  },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);

const thresholds = {
  // Every 422 here is expected: only transport failures and 5xx count as failures.
  http_req_failed: ['rate==0'],
  checks: ['rate==1'],
  http_req_duration: [`p(99)<${P99_MS}`],
};
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}

http.setResponseCallback(http.expectedStatuses(200, 422));

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
};

export default function () {
  if (__ITER % 2 === 0) {
    const res = http.post(URL, VALID_BODY, PARAMS);
    check(res, {
      'valid 200': (r) => r.status === 200,
      'valid canonical': (r) => r.body.indexOf(CANONICAL) === 0 || r.body.indexOf(CANONICAL) === 1,
    });
  } else {
    const res = http.post(URL, INVALID_BODY, PARAMS);
    check(res, {
      'invalid 422': (r) => r.status === 422,
      'invalid unsafe_link': (r) => r.body.indexOf('"reason":"unsafe_link"') !== -1,
    });
  }
}
