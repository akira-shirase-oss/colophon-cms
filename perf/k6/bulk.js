// Load scenario for bulk jobs (step P5, D-116). setup() signs in the administrator
// (dev-owner), makes a project, fills `articles` with drafts and submits a sealed bulk job —
// so reads have real jobs and items to name while the worker drives the manifest in the
// background. Every measured request is one of: 45 in 100 list a project's jobs (some
// filtered by state), 20 read one job, 20 page its items (some outcome-filtered), and 15
// submit a fresh one-item sealed job — the write arm, a manifest insert plus the job and
// document reservations. Cancels are left to the stack tests: a cancelled job under load
// would measure refusals, not work.
// Same settings as migrations.js; scripts/load.sh gives this endpoint RATE 250 and P99_MS 250.

import http from 'k6/http';
import { check, fail } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '250', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '250');

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
for (const endpoint of ['list', 'get', 'items', 'submit']) {
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

const BODY = { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } };
let sequence = 0;
const nextKey = (prefix) => `${prefix}-${Date.now().toString(36)}-${sequence += 1}`;

export function setup() {
  const signedIn = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }), { headers: { 'Content-Type': 'application/json' } });
  if (signedIn.status !== 204) fail(`sign-in failed: HTTP ${signedIn.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const headers = {
    Cookie: `colophon_session=${jar.colophon_session[0]}`, 'X-CSRF-Token': jar.colophon_csrf[0], Origin: PUBLIC_URL,
    'Content-Type': 'application/json',
  };
  const write = (key) => Object.assign({ 'Idempotency-Key': key }, headers);

  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-${Date.now().toString(36)}`, name: 'k6 bulk' }), { headers });
  if (project.status !== 201) fail(`project: HTTP ${project.status}`);
  const base = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;

  const drafts = [];
  for (let i = 0; i < 4; i += 1) {
    const entry = http.post(`${base}/collections/articles/entries`, JSON.stringify({ title: `bulk ${i}`, body: BODY }), { headers: write(`k6-bulk-entry-setup-${i}`) });
    if (entry.status !== 201) fail(`entry ${i}: HTTP ${entry.status} ${entry.body}`);
    drafts.push({ id: entry.json('data.id'), revision: entry.json('data.revision.id') });
  }
  const items = drafts.map((draft, i) => ({
    key: `k${i}`, entry: draft.id, expected_revision: draft.revision,
    op: { op: 'set', path: 'title', value: `bulk ${i} (job)` },
  }));
  const job = http.post(`${base}/bulk`, JSON.stringify({ collection: 'articles', items, seal: true }), { headers: write('k6-bulk-job-0000') });
  if (job.status !== 202) fail(`job: HTTP ${job.status} ${job.body}`);
  return { headers, base, job: job.json('data.id'), drafts };
}

export default function (data) {
  const slot = __ITER % 100;
  const params = (endpoint, extra) => ({ headers: Object.assign({}, data.headers, extra || {}), redirects: 0, tags: { endpoint } });
  if (slot < 45) {
    const filtered = slot % 3 === 0 ? '?state=done' : '';
    const res = http.get(`${data.base}/bulk${filtered}`, params('list'));
    check(res, { 'list 200': (r) => r.status === 200 && r.body.indexOf('{"data":[') === 0 });
  } else if (slot < 65) {
    const res = http.get(`${data.base}/bulk/${data.job}`, params('get'));
    check(res, { 'get 200': (r) => r.status === 200 && r.json('data.id') === data.job });
  } else if (slot < 85) {
    const outcome = slot % 2 === 0 ? '?outcome=applied' : '?limit=50';
    const res = http.get(`${data.base}/bulk/${data.job}/items${outcome}`, params('items'));
    check(res, { 'items 200': (r) => r.status === 200 && Array.isArray(r.json('data')) });
  } else {
    const draft = data.drafts[__ITER % data.drafts.length];
    const res = http.post(`${data.base}/bulk`, JSON.stringify({
      collection: 'articles', seal: true,
      items: [{ key: 'a', entry: draft.id, expected_revision: draft.revision, op: { op: 'set', path: 'title', value: 'loaded' } }],
    }), params('submit', { 'Idempotency-Key': nextKey('k6-bulk') }));
    // A stale base revision still submits: the conflict is the worker's to find, per item.
    check(res, { 'submit 202': (r) => r.status === 202 });
  }
}
