// Load scenario for roles owners define (step R.2, D-058). setup() signs two people in through
// Keycloak; the first creates a project and a "desk" role that may read and edit articles only,
// invites the second with it, and makes one more role per virtual user to edit. Every measured
// request then exercises a custom role: five in ten are the desk member listing articles (their
// role's permissions are loaded with their membership and checked per collection), one reads the
// project as them (the caller's permissions, for the Studio), two are the owner listing every
// role with its members, invitations and permissions, and two edit a role with If-Match (each
// virtual user its own role, so every edit names the version it last saw and none is stale). Same
// settings as members.js; scripts/load.sh gives this endpoint RATE 1000 and P99_MS 100.

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

// One role to edit per virtual user that can exist, plus the four built-in roles and the desk,
// within a project's 100 roles: steady runs grow to at most 95 virtual users.
const MAX_VUS = Math.min(VUS * 2, 95);
const EDITORS = SCENARIO === 'steady' ? MAX_VUS : VUS;
if (EDITORS > 95) throw new Error(`at most 95 virtual users can each edit a role of their own; VUS is ${VUS}`);
const scenarios = {
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '20m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: Math.min(VUS, MAX_VUS), maxVUs: MAX_VUS },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
// Reported per kind of request, so a tail can be traced to its cause; only the overall p99 fails a run.
for (const endpoint of ['entries', 'project', 'roles', 'edit']) {
  thresholds[`http_req_duration{endpoint:${endpoint}}`] = ['p(99)<60000'];
}
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '120s',
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

const articles = (actions) => actions.map((action) => ({ action, subject: 'articles' }));

export function setup() {
  const owner = signIn('dev-owner');
  const desk = signIn('dev-editor');
  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 roles' }), { headers: owner });
  if (project.status !== 201) fail(`project: HTTP ${project.status}`);
  const id = project.json('data.id');
  const base = `${BASE_URL}/api/v1/projects/${id}`;

  const role = http.post(`${base}/roles`, JSON.stringify({
    key: 'desk', name: 'Desk', permissions: articles(['content.read', 'content.update']),
  }), { headers: owner });
  if (role.status !== 201) fail(`role: HTTP ${role.status} ${role.body}`);
  const invited = http.post(`${base}/invitations`, JSON.stringify({ role: 'desk', label: 'k6 desk' }), { headers: owner });
  if (invited.status !== 201) fail(`invitation: HTTP ${invited.status}`);
  const joined = http.post(`${BASE_URL}/api/v1/invitations/accept`,
    JSON.stringify({ token: invited.json('data.link').split('#')[1] }), { headers: desk });
  if (joined.status !== 200) fail(`accept: HTTP ${joined.status} ${joined.body}`);

  for (let vu = 1; vu <= EDITORS; vu += 1) {
    const made = http.post(`${base}/roles`, JSON.stringify({
      key: `vu-${vu}`, name: `Editor ${vu}`, permissions: articles(['content.read']),
    }), { headers: owner });
    if (made.status !== 201) fail(`role vu-${vu}: HTTP ${made.status} ${made.body}`);
  }
  return { owner, desk, base, entries: `${base}/environments/production/collections/articles/entries` };
}

// Each virtual user's role, and the version its last edit left it at.
let version = 1;

export default function (data) {
  const slot = __ITER % 10;
  if (slot < 5) {
    const res = http.get(data.entries, { headers: data.desk, redirects: 0, tags: { endpoint: 'entries' } });
    check(res, { 'entries 200': (r) => r.status === 200 });
  } else if (slot === 5) {
    const res = http.get(data.base, { headers: data.desk, redirects: 0, tags: { endpoint: 'project' } });
    check(res, {
      'project 200': (r) => r.status === 200,
      "the desk's permissions": (r) => r.json('data.role') === 'desk' && r.json('data.permissions').length === 2,
    });
  } else if (slot < 8) {
    const res = http.get(`${data.base}/roles`, { headers: data.owner, redirects: 0, tags: { endpoint: 'roles' } });
    check(res, { 'roles 200': (r) => r.status === 200 && r.json('data.#') === EDITORS + 5 });
  } else {
    const actions = __ITER % 20 < 10 ? ['content.read', 'content.create'] : ['content.read'];
    const res = http.put(`${data.base}/roles/vu-${__VU}`, JSON.stringify({ name: `Editor ${__VU}`, permissions: articles(actions) }), {
      headers: Object.assign({ 'If-Match': `"v${version}"` }, data.owner),
      redirects: 0,
      tags: { endpoint: 'edit' },
    });
    const ok = check(res, { 'edit 200': (r) => r.status === 200 });
    if (ok) version = res.json('data.version');
  }
}
