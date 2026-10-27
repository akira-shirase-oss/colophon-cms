// Load scenario for setup codes on a set-up installation (step 3.7, D-060). setup() signs in the
// bootstrapped administrator (dev-owner) and someone who isn't (dev-editor). Nine requests in
// twenty are dev-editor presenting a setup code (a different one each time), each of which must
// be refused with 409 `already_set_up`: once anyone is an administrator the server refuses before
// counting an attempt or comparing anything, so this is the cost of the endpoint everyone can
// reach. Nine are dev-editor's /me, which must say the installation has an administrator and they
// aren't one; two are dev-owner's /me. teardown() checks that dev-editor is still not an
// administrator. Same settings as projects.js; scripts/load.sh gives it RATE 2000 and P99_MS 100.

import http from 'k6/http';
import { check, fail } from 'k6';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '2000', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '100');

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const scenarios = {
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '20m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
// Reported per kind of request, so a tail can be traced to its cause; only the overall p99 fails a run.
for (const endpoint of ['setup-refused', 'me', 'admin-me']) {
  thresholds[`http_req_duration{endpoint:${endpoint}}`] = ['p(99)<60000'];
}
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '60s',
};

/** Signs `username` in with a cookie jar of its own; the headers a browser tab would send. */
function signIn(username) {
  const jar = new http.CookieJar();
  const page = http.get(`${PUBLIC_URL}/auth/login?return_to=/me`, { jar });
  const action = parseHTML(page.body).find('#kc-form-login').attr('action');
  if (!action) fail(`no Keycloak login form for ${username}`);
  const me = http.post(action.replace(/&amp;/g, '&'), { username, password: 'colophon-dev' }, { jar });
  if (me.status !== 200) fail(`sign-in as ${username} failed: HTTP ${me.status}`);
  const cookies = jar.cookiesForURL(PUBLIC_URL);
  return {
    Cookie: `colophon_session=${cookies.colophon_session[0]}`, 'X-CSRF-Token': cookies.colophon_csrf[0], Origin: PUBLIC_URL,
    'Content-Type': 'application/json',
  };
}

export function setup() {
  const admin = signIn('dev-owner');
  const other = signIn('dev-editor');
  const me = http.get(`${BASE_URL}/me`, { headers: other });
  if (me.json('data.administrator') !== false) fail('dev-editor must not be an administrator');
  if (me.json('data.installation.has_administrator') !== true) fail('the installation must have an administrator (scripts/migrate.sh)');
  return { admin, other };
}

// Crockford's base-32: a well-formed code that is never right.
const ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
function code(seed) {
  let text = '';
  let value = seed;
  for (let index = 0; index < 20; index += 1) {
    if (index > 0 && index % 5 === 0) text += '-';
    text += ALPHABET[value % 32];
    value = Math.floor(value / 32) + index * 7919;
  }
  return text;
}

// A refusal is the expected answer here, not a failed request.
const refusedOk = http.expectedStatuses(409);

export default function (data) {
  const slot = __ITER % 20;
  if (slot < 9) {
    const body = JSON.stringify({ code: code(__VU * 1000003 + __ITER) });
    const res = http.post(`${BASE_URL}/api/v1/setup`, body,
      { headers: data.other, redirects: 0, responseCallback: refusedOk, tags: { endpoint: 'setup-refused' } });
    check(res, { 'refused 409 already_set_up': (r) => r.status === 409 && r.body.indexOf('"already_set_up"') !== -1 });
  } else if (slot < 18) {
    const res = http.get(`${BASE_URL}/me`, { headers: data.other, redirects: 0, tags: { endpoint: 'me' } });
    check(res, {
      'me: set up, not an administrator': (r) =>
        r.status === 200 && r.body.indexOf('"administrator":false') !== -1 && r.body.indexOf('"has_administrator":true') !== -1,
    });
  } else {
    const res = http.get(`${BASE_URL}/me`, { headers: data.admin, redirects: 0, tags: { endpoint: 'admin-me' } });
    check(res, { 'me: administrator': (r) => r.status === 200 && r.body.indexOf('"administrator":true') !== -1 });
  }
}

export function teardown(data) {
  const me = http.get(`${BASE_URL}/me`, { headers: data.other });
  if (me.json('data.administrator') !== false) fail('dev-editor became an administrator: a refused code did something');
}
