// Load scenario for self-serve sign-up (D-235): with open registration and no Turnstile (the
// launch configuration), each iteration signs a fresh account up, which pays Argon2id once,
// gets its session in the same answer, and reads its own usage with it. The sign-up cap per
// address is lifted for this run alone (scripts/load.sh): one machine sends every request, and the
// cap itself is pinned by the API tests (signup_guards.rs), not by load.
//
// It holds when every sign-up is 201 with a session cookie and no Turnstile token, every usage read
// is 200 with the account's own empty counts and the open project cap, and the p99 stays in
// budget. scripts/load.sh gives this endpoint RATE 20 and P99_MS 700.

import http from 'k6/http';
import { check } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '32', 10);
const RATE = parseInt(__ENV.RATE || '20', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '700');
const PASSWORD = 'a k6 password, long enough';

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const scenarios = {
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '15m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
};

const JSON_HEADERS = { 'Content-Type': 'application/json' };
const run = Date.now().toString(36);

export default function () {
  const email = `k6-${run}-${__VU}-${__ITER}@signup.test`;
  const made = http.post(`${BASE_URL}/auth/signup`, JSON.stringify({ email, password: PASSWORD }), {
    headers: JSON_HEADERS,
    tags: { endpoint: 'signup' },
  });
  const cookie = (made.headers['Set-Cookie'] || '').match(/__Host-colophon_session=([^;]+)/);
  check(made, {
    'signed up': (r) => r.status === 201,
    'session in the same answer': () => cookie !== null,
  });
  if (cookie === null) return;
  const used = http.get(`${BASE_URL}/api/v1/me/usage`, {
    headers: { Cookie: `__Host-colophon_session=${cookie[1]}` },
    tags: { endpoint: 'usage' },
  });
  check(used, {
    'usage read': (r) => r.status === 200,
    'its own, empty, under the open cap': (r) => {
      const data = r.json('data');
      return data.projects.used === 0 && data.projects.limit !== null && data.entries.used === 0;
    },
  });
}
