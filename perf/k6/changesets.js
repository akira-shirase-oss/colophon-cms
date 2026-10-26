// Load scenario for changesets and their review (step 2.1, D-044). setup() signs in the
// administrator (dev-owner), creates a project with five articles, and pins them into one
// submitted changeset per virtual user, as many people reviewing different changesets would.
// Every measured request is theirs: 55 in 100 read a changeset (items, reviews and the status
// decided from them), 20 list the project's changesets, 15 review a changeset's current hash
// (under a lock on its row), and 10 create a new changeset pinning two of the articles (an
// idempotent create). Reviews of one changeset queue on its row by design; this scenario measures
// the path, not that queue (docs/modules/03-auth.md measures the same kind of queue for members).
// Same settings as health.js; scripts/load.sh gives this endpoint RATE 1000 and P99_MS 100.

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
// Reported per kind of request, so a tail can be traced to its cause; only the overall p99 fails a run.
for (const endpoint of ['get', 'list', 'review', 'create']) {
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

export function setup() {
  const page = http.get(`${PUBLIC_URL}/auth/login?return_to=/me`);
  const action = parseHTML(page.body).find('#kc-form-login').attr('action');
  if (!action) fail('no Keycloak login form');
  const me = http.post(action.replace(/&amp;/g, '&'), { username: 'dev-owner', password: 'colophon-dev' });
  if (me.status !== 200) fail(`sign-in failed: HTTP ${me.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const headers = {
    Cookie: `colophon_session=${jar.colophon_session[0]}`, 'X-CSRF-Token': jar.colophon_csrf[0], Origin: PUBLIC_URL,
    'Content-Type': 'application/json',
  };

  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-${Date.now().toString(36)}`, name: 'k6 changesets' }), { headers });
  if (project.status !== 201) fail(`project: HTTP ${project.status}`);
  const base = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;
  const entries = [];
  for (let i = 0; i < 5; i += 1) {
    const body = { title: `Article ${i}`, body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } };
    const created = http.post(`${base}/collections/articles/entries`, JSON.stringify(body), { headers: Object.assign({ 'Idempotency-Key': `k6-setup-entry-${Date.now()}-${i}` }, headers) });
    if (created.status !== 201) fail(`entry: HTTP ${created.status}`);
    entries.push(created.json('data.id'));
  }
  // One submitted changeset per virtual user (the steady scenario can use twice as many).
  const sets = [];
  for (let i = 0; i < VUS * 2; i += 1) {
    const set = http.post(`${base}/changesets`, JSON.stringify({ title: `k6 ${i}`, entries }), { headers: Object.assign({ 'Idempotency-Key': `k6-setup-set-${Date.now()}-${i}` }, headers) });
    if (set.status !== 201) fail(`changeset: HTTP ${set.status} ${set.body}`);
    const path = `${base}/changesets/${set.json('data.id')}`;
    const hash = set.json('data.hash');
    const submitted = http.post(`${path}/submit`, null, { headers: Object.assign({ 'If-Match': `"${hash}"` }, headers) });
    if (submitted.status !== 200) fail(`submit: HTTP ${submitted.status}`);
    sets.push({ path, hash });
  }
  return { headers, base, sets, entries, run: Date.now().toString(36) };
}

export default function (data) {
  const slot = __ITER % 100;
  const mine = data.sets[(__VU - 1) % data.sets.length];
  const params = (endpoint, extra) => ({ headers: Object.assign({}, data.headers, extra || {}), redirects: 0, tags: { endpoint } });
  if (slot < 55) {
    const res = http.get(mine.path, params('get'));
    check(res, { 'get 200': (r) => r.status === 200 && r.json('data.items').length === 5 });
  } else if (slot < 75) {
    const res = http.get(`${data.base}/changesets`, params('list'));
    check(res, { 'list 200': (r) => r.status === 200 && r.body.indexOf('{"data":[') === 0 });
  } else if (slot < 90) {
    const decision = __ITER % 2 === 0 ? 'approve' : 'request_changes';
    const res = http.post(`${mine.path}/reviews`, JSON.stringify({ decision }), params('review', { 'If-Match': `"${mine.hash}"` }));
    check(res, { 'review 200': (r) => r.status === 200 && r.json('data.hash') === mine.hash });
  } else {
    const pair = [data.entries[__ITER % 5], data.entries[(__ITER + 1) % 5]];
    const res = http.post(`${data.base}/changesets`, JSON.stringify({ title: `k6 ${__VU}-${__ITER}`, entries: pair }),
      params('create', { 'Idempotency-Key': `k6-${data.run}-${__VU}-${__ITER}` }));
    check(res, { 'create 201': (r) => r.status === 201 });
  }
}
