// Load scenario for who may create projects (step 1.8, D-043). setup() signs in the bootstrapped
// administrator (dev-owner) and someone who isn't (dev-editor). Nine requests in twenty are
// dev-editor trying to create a project, each of which must be refused with 403 (the gate's
// lookup and nothing written); nine are dev-editor's /me, which must say "not an administrator";
// two are dev-owner's /me, which must say "administrator". teardown() checks that dev-editor
// owns no more projects than at the start. Same settings as health.js; scripts/load.sh gives
// this endpoint RATE 2000 and P99_MS 100.

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
for (const endpoint of ['refused', 'me', 'admin-me']) {
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

const owned = (headers) => http.get(`${BASE_URL}/api/v1/projects`, { headers }).json('data').length;

export function setup() {
  const admin = signIn('dev-owner');
  const other = signIn('dev-editor');
  const me = http.get(`${BASE_URL}/me`, { headers: other });
  if (me.json('data.administrator') !== false) fail('dev-editor must not be an administrator');
  return { admin, other, before: owned(other) };
}

// A refusal is the expected answer here, not a failed request.
const refusedOk = http.expectedStatuses(403);

export default function (data) {
  const slot = __ITER % 20;
  if (slot < 9) {
    const body = JSON.stringify({ slug: `k6-no-${__VU}-${__ITER}`, name: 'Never created' });
    const res = http.post(`${BASE_URL}/api/v1/projects`, body,
      { headers: data.other, redirects: 0, responseCallback: refusedOk, tags: { endpoint: 'refused' } });
    check(res, { 'refused 403': (r) => r.status === 403 && r.body.indexOf('only an administrator') !== -1 });
  } else if (slot < 18) {
    const res = http.get(`${BASE_URL}/me`, { headers: data.other, redirects: 0, tags: { endpoint: 'me' } });
    check(res, { 'me: not an administrator': (r) => r.status === 200 && r.body.indexOf('"administrator":false') !== -1 });
  } else {
    const res = http.get(`${BASE_URL}/me`, { headers: data.admin, redirects: 0, tags: { endpoint: 'admin-me' } });
    check(res, { 'me: administrator': (r) => r.status === 200 && r.body.indexOf('"administrator":true') !== -1 });
  }
}

export function teardown(data) {
  const after = owned(data.other);
  if (after !== data.before) fail(`dev-editor's projects went from ${data.before} to ${after}: a refusal created something`);
}
