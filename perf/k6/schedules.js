// Load scenario for scheduled publishing (D-060). setup() signs the owner in through Keycloak,
// creates a project and one approved changeset per virtual user that can exist. Every measured
// request is the owner's: six in ten list the environment's schedules, two list one changeset's,
// and each virtual user schedules its own changeset a day ahead (one in ten) and cancels that
// schedule (one in ten), so a schedule is resolved in its time zone, checked against the approval
// and the role, stored and cancelled under the changeset's lock. Same settings as members.js;
// scripts/load.sh gives this endpoint RATE 1000 and P99_MS 100.

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

// One changeset per virtual user that can exist; steady runs grow to twice VUS.
const MAX_VUS = SCENARIO === 'steady' ? VUS * 2 : VUS;
const scenarios = {
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '20m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: MAX_VUS },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
// Reported per kind of request, so a tail can be traced to its cause; only the overall p99 fails a run.
for (const endpoint of ['list', 'mine', 'schedule', 'cancel']) {
  thresholds[`http_req_duration{endpoint:${endpoint}}`] = ['p(99)<60000'];
}
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '300s',
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

const uuid = () => 'xxxxxxxx-xxxx-4xxx-8xxx-xxxxxxxxxxxx'.replace(/x/g, () => Math.floor(Math.random() * 16).toString(16));

export function setup() {
  const owner = signIn('dev-owner');
  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 schedules' }), { headers: owner });
  if (project.status !== 201) fail(`project: HTTP ${project.status}`);
  const base = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;
  const doc = { type: 'doc', content: [{ type: 'paragraph' }] };
  const changesets = [];
  for (let vu = 1; vu <= MAX_VUS; vu += 1) {
    const withKey = { headers: Object.assign({ 'Idempotency-Key': uuid() }, owner) };
    const entry = http.post(`${base}/collections/articles/entries`,
      JSON.stringify({ title: `Note ${vu}`, body: { schema: 'colophon/rich-text', version: 1, doc } }), withKey);
    if (entry.status !== 201) fail(`entry ${vu}: HTTP ${entry.status}`);
    const set = http.post(`${base}/changesets`, JSON.stringify({ title: `Note ${vu}`, entries: [entry.json('data.id')] }),
      { headers: Object.assign({ 'Idempotency-Key': uuid() }, owner) });
    if (set.status !== 201) fail(`changeset ${vu}: HTTP ${set.status}`);
    const id = set.json('data.id');
    const hash = set.json('data.hash');
    const ifMatch = { headers: Object.assign({ 'If-Match': `"${hash}"` }, owner) };
    if (http.post(`${base}/changesets/${id}/submit`, null, ifMatch).status !== 200) fail(`submit ${vu}`);
    if (http.post(`${base}/changesets/${id}/reviews`, JSON.stringify({ decision: 'approve' }), ifMatch).status !== 200) fail(`approve ${vu}`);
    changesets.push({ id, hash });
  }
  // A day ahead, in UTC: well inside the approval's seven days.
  const at = new Date(Date.now() + 24 * 3600 * 1000).toISOString().slice(0, 16);
  return { owner, base, changesets, at };
}

// This virtual user's pending schedule, between its scheduling and its cancelling.
let pending = null;

export default function (data) {
  const params = { headers: data.owner, redirects: 0 };
  const mine = data.changesets[(__VU - 1) % data.changesets.length];
  const slot = __ITER % 10;
  if (slot < 6) {
    const res = http.get(`${data.base}/schedules`, Object.assign({ tags: { endpoint: 'list' } }, params));
    check(res, { 'list 200': (r) => r.status === 200 });
  } else if (slot < 8) {
    const res = http.get(`${data.base}/schedules?changeset=${mine.id}`, Object.assign({ tags: { endpoint: 'mine' } }, params));
    check(res, { 'mine 200': (r) => r.status === 200 });
  } else if (slot === 8 || pending === null) {
    const res = http.post(`${data.base}/changesets/${mine.id}/schedule`, JSON.stringify({ local_time: data.at, time_zone: 'Europe/London' }), {
      headers: Object.assign({ 'If-Match': `"${mine.hash}"` }, data.owner),
      redirects: 0,
      tags: { endpoint: 'schedule' },
    });
    if (check(res, { 'schedule 201': (r) => r.status === 201 })) pending = res.json('data.id');
  } else {
    const res = http.del(`${data.base}/schedules/${pending}`, null, Object.assign({ tags: { endpoint: 'cancel' } }, params));
    check(res, { 'cancel 200': (r) => r.status === 200 });
    pending = null;
  }
}
