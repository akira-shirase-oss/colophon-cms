// Load scenario for public delivery (step 2.2, D-045). setup() signs in the administrator,
// creates a project with 20 articles and publishes three releases: all 20, then new revisions of
// five, then of five more. Every measured request is anonymous (no cookie, no token), as the
// public reads: 60 in 100 list the latest release, 20 list a release pinned at generation 1, 2
// or 3, and 20 read one entry at the latest release. Each read runs as colophon_delivery.
// Same settings as health.js; scripts/load.sh gives this endpoint RATE 2000 and P99_MS 100.

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
for (const endpoint of ['latest', 'pinned', 'entry']) {
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

const body = (title, text) => ({
  title,
  body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] } },
});

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
  const run = Date.now().toString(36);
  let n = 0;
  const key = () => `k6-delivery-${run}-${(n += 1)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-${run}`, name: 'k6 delivery' }), { headers });
  if (project.status !== 201) fail(`project: HTTP ${project.status}`);
  const projectId = project.json('data.id');
  const base = `${BASE_URL}/api/v1/projects/${projectId}/environments/production`;
  const entries = [];
  for (let i = 0; i < 20; i += 1) {
    const created = http.post(`${base}/collections/articles/entries`, JSON.stringify(body(`Article ${i}`, 'first')), { headers: Object.assign({ 'Idempotency-Key': key() }, headers) });
    if (created.status !== 201) fail(`entry: HTTP ${created.status}`);
    entries.push(created.json('data.id'));
  }
  const release = (ids, generation) => {
    const set = http.post(`${base}/changesets`, JSON.stringify({ title: `release ${generation + 1}`, entries: ids }), { headers: Object.assign({ 'Idempotency-Key': key() }, headers) });
    const path = `${base}/changesets/${set.json('data.id')}`;
    const ifMatch = Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, headers);
    http.post(`${path}/submit`, null, { headers: ifMatch });
    http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch });
    const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: generation }), { headers: Object.assign({ 'Idempotency-Key': key() }, ifMatch) });
    if (published.status !== 201) fail(`publish: HTTP ${published.status} ${published.body}`);
  };
  release(entries, 0);
  const edit = (ids, text) => {
    for (const id of ids) {
      const path = `${base}/collections/articles/entries/${id}`;
      const current = http.get(path, { headers });
      const saved = http.put(path, JSON.stringify(body(current.json('data.content.title'), text)), { headers: Object.assign({ 'If-Match': current.headers.Etag, 'Idempotency-Key': key() }, headers) });
      if (saved.status !== 200) fail(`save: HTTP ${saved.status}`);
    }
  };
  edit(entries.slice(0, 5), 'second');
  release(entries.slice(0, 5), 1);
  edit(entries.slice(5, 10), 'third');
  release(entries.slice(5, 10), 2);
  return { content: `${BASE_URL}/content/v1/${projectId}/production/articles`, entries };
}

export default function (data) {
  const slot = __ITER % 10;
  const params = (endpoint) => ({ redirects: 0, tags: { endpoint } });
  if (slot < 6) {
    const res = http.get(data.content, params('latest'));
    check(res, { 'latest: 20 at generation 3': (r) => r.status === 200 && r.headers['Colophon-Generation'] === '3' && r.json('data').length === 20 });
  } else if (slot < 8) {
    const generation = 1 + (__ITER % 3);
    const res = http.get(`${data.content}?generation=${generation}`, params('pinned'));
    check(res, { 'pinned: the generation asked for': (r) => r.status === 200 && r.headers['Colophon-Generation'] === String(generation) && r.json('data').length === 20 });
  } else {
    const res = http.get(`${data.content}/${data.entries[__ITER % 20]}`, params('entry'));
    check(res, { 'entry 200': (r) => r.status === 200 && r.json('data.published_in') >= 1 });
  }
}
