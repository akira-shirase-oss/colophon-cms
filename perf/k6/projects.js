// Load scenario for who may create projects (step 1.8, D-043; D-235). setup() signs in the bootstrapped
// administrator (dev-owner) and makes someone who isn't (a fresh account). Nine requests in twenty are
// a fresh account trying to create a project, each of which must be refused (the gate's lookup and
// nothing written): with 403 when registration is invite or closed, and, when it is open, with 409
// once setup() has filled the account's project cap (the cap's count under FOR UPDATE, nothing
// written). Setup learns which from the first refusal; nine are its /me, which must say "not an administrator";
// two are dev-owner's /me, which must say "administrator". teardown() checks that the account
// owns no more projects than at the start. Same settings as health.js; scripts/load.sh gives
// this endpoint RATE 2000 and P99_MS 100.

import http from 'k6/http';
import { check, fail } from 'k6';
import exec from 'k6/execution';

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
  const signedIn = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: `${username}@colophon.test`, password: 'colophon-dev' }), { jar, headers: { 'Content-Type': 'application/json' } });
  if (signedIn.status !== 204) fail(`sign-in as ${username} failed: HTTP ${signedIn.status}`);
  const cookies = jar.cookiesForURL(PUBLIC_URL);
  return {
    Cookie: `__Host-colophon_session=${cookies['__Host-colophon_session'][0]}`, 'X-CSRF-Token': cookies['__Host-colophon_csrf'][0], Origin: PUBLIC_URL,
    'Content-Type': 'application/json',
  };
}

const owned = (headers) => http.get(`${BASE_URL}/api/v1/projects`, { headers }).json('data').length;

/** A password account made for this run: not an administrator, in no project, so what its /me
 *  reads never depends on what earlier runs or test suites left on the development accounts. */
function freshPerson() {
  const jar = new http.CookieJar();
  const run = Date.now().toString(36);
  const made = http.post(`${PUBLIC_URL}/auth/signup`, JSON.stringify({ email: `k6-projects-${run}@load.test`, password: 'a password for this run' }), { jar, headers: { 'Content-Type': 'application/json' } });
  if (made.status !== 201) fail(`signup: HTTP ${made.status} ${made.body}`);
  const cookies = jar.cookiesForURL(PUBLIC_URL);
  return {
    Cookie: `__Host-colophon_session=${cookies['__Host-colophon_session'][0]}`, 'X-CSRF-Token': cookies['__Host-colophon_csrf'][0], Origin: PUBLIC_URL,
    'Content-Type': 'application/json',
  };
}

export function setup() {
  // The administrator is the bootstrapped dev-owner — the one account that can be. Its /me is a
  // tenth of the requests, and lists the projects every earlier run made; the person who isn't
  // an administrator is made fresh.
  const admin = signIn('dev-owner');
  const other = freshPerson();
  const me = http.get(`${BASE_URL}/me`, { headers: other });
  if (me.json('data.administrator') !== false) fail('a fresh account must not be an administrator');
  // Open registration lets the account create its own projects up to the cap: fill it, so every
  // create in the run meets the cap's refusal. Invite or closed refuses the first one with 403.
  const run = Date.now().toString(36);
  let refusal = null;
  for (let made = 0; made <= 50 && refusal === null; made += 1) {
    const res = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-cap-${run}-${made}`, name: 'Fills the cap' }),
      { headers: other, redirects: 0, responseCallback: http.expectedStatuses(201, 403, 409) });
    if (res.status === 403 && res.body.indexOf('only an administrator') !== -1) refusal = { status: 403, says: 'only an administrator' };
    else if (res.status === 409 && res.body.indexOf('projects an account may own') !== -1) refusal = { status: 409, says: 'projects an account may own' };
    else if (res.status !== 201) fail(`filling the project cap: HTTP ${res.status} ${res.body}`);
  }
  if (refusal === null) fail('a fresh account created more than 50 projects without meeting the cap');
  return { admin, other, refusal, before: owned(other) };
}

// A refusal is the expected answer here, not a failed request.
const refusedOk = http.expectedStatuses(403, 409);

export default function (data) {
  const slot = exec.scenario.iterationInTest % 20;
  if (slot < 9) {
    const body = JSON.stringify({ slug: `k6-no-${__VU}-${__ITER}`, name: 'Never created' });
    const res = http.post(`${BASE_URL}/api/v1/projects`, body,
      { headers: data.other, redirects: 0, responseCallback: refusedOk, tags: { endpoint: 'refused' } });
    check(res, { refused: (r) => r.status === data.refusal.status && r.body.indexOf(data.refusal.says) !== -1 });
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
  if (after !== data.before) fail(`the fresh account's projects went from ${data.before} to ${after}: a refusal created something`);
}
