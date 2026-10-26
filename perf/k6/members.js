// Load scenario for members and invitations (step 1.7). setup() signs two people in through
// Keycloak, the first creates a project and invites the second, who accepts. Every measured
// request is then the owner's: six in ten list the members (row-level security and a join),
// two look an invitation up by its token (a hash lookup, then a scoped read of the project's
// name), one changes the second person's role (owners locked FOR UPDATE, so these queue on one
// project), and one creates an invitation (a random token, hashed and stored). Same settings as
// health.js; scripts/load.sh gives this endpoint RATE 1000 and P99_MS 100.

import http from 'k6/http';
import { check, fail } from 'k6';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '1000', 10);
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

const tokenOf = (res) => res.json('data.link').split('#')[1];

export function setup() {
  const owner = signIn('dev-editor');
  const guest = signIn('dev-owner');
  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 members' }), { headers: owner });
  if (project.status !== 201) fail(`project: HTTP ${project.status}`);
  const base = `${BASE_URL}/api/v1/projects/${project.json('data.id')}`;

  const invited = http.post(`${base}/invitations`, JSON.stringify({ role: 'editor', label: 'k6 guest' }), { headers: owner });
  if (invited.status !== 201) fail(`invitation: HTTP ${invited.status}`);
  const joined = http.post(`${BASE_URL}/api/v1/invitations/accept`, JSON.stringify({ token: tokenOf(invited) }), { headers: guest });
  if (joined.status !== 200) fail(`accept: HTTP ${joined.status} ${joined.body}`);
  const listed = http.get(`${base}/members`, { headers: owner });
  const member = listed.json('data').find((m) => m.role !== 'owner');
  if (!member) fail('the guest did not join');

  // One pending invitation to look up for the whole run.
  const pending = http.post(`${base}/invitations`, JSON.stringify({ role: 'publisher', label: 'k6 lookups' }), { headers: owner });
  if (pending.status !== 201) fail(`invitation: HTTP ${pending.status}`);
  return { owner, base, guest: member.identity_id, token: tokenOf(pending) };
}

export default function (data) {
  const params = { headers: data.owner, redirects: 0 };
  const slot = __ITER % 10;
  if (slot < 6) {
    const res = http.get(`${data.base}/members`, Object.assign({ tags: { endpoint: 'members' } }, params));
    check(res, {
      'members 200': (r) => r.status === 200,
      'both members': (r) => r.body.indexOf('"role":"editor"') > 0 || r.body.indexOf('"role":"publisher"') > 0,
    });
  } else if (slot < 8) {
    const res = http.post(`${BASE_URL}/api/v1/invitations/lookup`, JSON.stringify({ token: data.token }),
      Object.assign({ tags: { endpoint: 'lookup' } }, params));
    check(res, { 'lookup 200': (r) => r.status === 200 && r.json('data.project_name') === 'k6 members' });
  } else if (slot === 8) {
    const role = __ITER % 20 < 10 ? 'editor' : 'publisher';
    const res = http.put(`${data.base}/members/${data.guest}`, JSON.stringify({ role }), Object.assign({ tags: { endpoint: 'role' } }, params));
    check(res, { 'role 204': (r) => r.status === 204 });
  } else {
    const res = http.post(`${data.base}/invitations`, JSON.stringify({ role: 'editor', label: `k6 ${__VU}-${__ITER}` }),
      Object.assign({ tags: { endpoint: 'invite' } }, params));
    check(res, { 'invite 201': (r) => r.status === 201 });
  }
}
