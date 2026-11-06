// Load scenario for email-and-password accounts (D-084): every request pays Argon2id on the
// blocking pool — a sign-in verifies, a taken sign-up hashes before it finds the account, and a
// missing account verifies against the dummy hash so it costs the same as a wrong password.
// setup() signs a small pool of accounts up; iterations sign them in and get refused two ways.
// No sign-in: these endpoints are open, so no session is in the loop.

import http from 'k6/http';
import { check, fail } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '50', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '500');
const SEEDED = 8;
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
  setupTimeout: '60s',
};

const FAILED = http.expectedStatuses(400);
const TAKEN = http.expectedStatuses(409);
const JSON_HEADERS = { 'Content-Type': 'application/json' };

const run = `${__VU}-${Date.now().toString(36)}`;

export function setup() {
  const emails = [];
  for (let i = 0; i < SEEDED; i++) {
    const address = `k6-${Date.now().toString(36)}-${i}@accounts.test`;
    const made = http.post(`${BASE_URL}/auth/signup`, JSON.stringify({ email: address, password: PASSWORD }), { headers: JSON_HEADERS });
    if (made.status !== 201) fail(`signup ${i}: HTTP ${made.status} ${made.body}`);
    emails.push(address);
  }
  return { emails };
}

export default function (data) {
  const roll = __ITER % 5;
  if (roll < 3) {
    // A real sign-in: the hash verifies, the session comes back.
    const address = data.emails[(__VU + __ITER) % data.emails.length];
    const res = http.post(`${BASE_URL}/auth/signin`, JSON.stringify({ email: address, password: PASSWORD }), { headers: JSON_HEADERS, tags: { endpoint: 'signin' } });
    check(res, {
      'signed in': (r) => r.status === 204,
      'session cookie set': (r) => (r.headers['Set-Cookie'] || '').includes('colophon_session='),
    });
  } else if (roll === 3) {
    // A missing account: the dummy-hash verify runs, then the same refusal a wrong password gets.
    const res = http.post(`${BASE_URL}/auth/signin`, JSON.stringify({ email: `k6-${run}-${__ITER}@accounts.test`, password: 'guessed wrong' }), { headers: JSON_HEADERS, responseCallback: FAILED, tags: { endpoint: 'missing' } });
    check(res, { 'refused like a wrong password': (r) => r.status === 400 });
  } else {
    // A taken sign-up: the new password is hashed before the email's account is found.
    const res = http.post(`${BASE_URL}/auth/signup`, JSON.stringify({ email: data.emails[__ITER % data.emails.length], password: 'a fresh password' }), { headers: JSON_HEADERS, responseCallback: TAKEN, tags: { endpoint: 'taken' } });
    check(res, { 'refused as taken': (r) => r.status === 409 });
  }
}
