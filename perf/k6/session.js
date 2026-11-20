// Load scenario for the signed-in path (step 1.1): every request is authenticated by session
// cookie, so each one looks the session up in Postgres and reads through row-level security.
// setup() makes a person of its own, the member of one project of its own, so the measured state
// is the same whatever earlier runs left behind. Requests alternate between GET /me and
// GET /api/v1/projects. Same settings as health.js, with the defaults
// scripts/load.sh gives this endpoint: RATE 2000 and P99_MS 100.

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
  // setup() follows the sign-in redirects itself; the measured requests never redirect.
  setupTimeout: '60s',
};

/** The headers a browser tab of the session in `jar` would send. */
function browserOf(jar) {
  const cookies = jar.cookiesForURL(PUBLIC_URL);
  return {
    Cookie: `colophon_session=${cookies.colophon_session[0]}`, 'X-CSRF-Token': cookies.colophon_csrf[0], Origin: PUBLIC_URL,
    'Content-Type': 'application/json',
  };
}

/** A person made for this run, so what is measured never depends on what earlier runs or test
 *  suites left on the shared development accounts (dev-owner's project list grows with every
 *  project a test makes): a fresh password account, the member of exactly one fresh project,
 *  which the administrator `owner` creates and invites them to. Their browser headers and the
 *  project's id. */
function freshMember(owner, label, role) {
  const run = Date.now().toString(36);
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-${label}-${run}`, name: `k6 ${label}` }), { headers: owner });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const id = project.json('data.id');
  const invited = http.post(`${BASE_URL}/api/v1/projects/${id}/invitations`, JSON.stringify({ role, label: `k6 ${label}` }), { headers: owner });
  if (invited.status !== 201) fail(`invitation: HTTP ${invited.status} ${invited.body}`);
  const jar = new http.CookieJar();
  const made = http.post(`${PUBLIC_URL}/auth/signup`, JSON.stringify({ email: `k6-${label}-${run}@load.test`, password: 'a password for this run', display_name: `k6 ${label}` }), { jar, headers: { 'Content-Type': 'application/json' } });
  if (made.status !== 201) fail(`signup: HTTP ${made.status} ${made.body}`);
  const person = browserOf(jar);
  const joined = http.post(`${BASE_URL}/api/v1/invitations/accept`, JSON.stringify({ token: invited.json('data.link').split('#')[1] }), { headers: person });
  if (joined.status !== 200) fail(`accept: HTTP ${joined.status} ${joined.body}`);
  return { person, project: id };
}

export function setup() {
  // The built-in password card (D-146) for the administrator, who makes the project.
  const jar = new http.CookieJar();
  const signedIn = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }), { jar, headers: { 'Content-Type': 'application/json' } });
  if (signedIn.status !== 204) fail(`sign-in failed: HTTP ${signedIn.status}`);
  const { person } = freshMember(browserOf(jar), 'session', 'editor');
  return { cookie: person.Cookie };
}

export default function (data) {
  const params = { headers: { Cookie: data.cookie }, redirects: 0 };
  if (exec.scenario.iterationInTest % 2 === 0) {
    const res = http.get(`${BASE_URL}/me`, Object.assign({ tags: { endpoint: 'me' } }, params));
    check(res, {
      'me 200': (r) => r.status === 200,
      'me names the person': (r) => r.body.indexOf('"display_name":"k6 session"') !== -1,
    });
  } else {
    const res = http.get(`${BASE_URL}/api/v1/projects`, Object.assign({ tags: { endpoint: 'projects' } }, params));
    check(res, {
      'projects 200': (r) => r.status === 200,
      'projects listed': (r) => r.body.indexOf('{"data":[') === 0,
      'just the one project': (r) => r.json('data').length === 1,
    });
  }
}
