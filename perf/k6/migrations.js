// Load scenario for content-schema migrations (step M.5c, D-100, D-101). setup() signs in the
// administrator (dev-owner), makes a project whose built-in `articles` collection and a custom
// `notes` collection each hold drafts a required-field change can't take as they are, and
// plans the change on each — so reads have real plans and items to name. Every measured
// request is one of: 45 in 100 list a project's plans (ten of them filtered by collection or
// state), 20 read one plan, 15 page its items, 10 read just the `needs_input` items, and 10
// plan the change again — the write arm, which takes the collection's change lock, dry-runs
// every stored draft against the target and supersedes the collection's open plans (its reads
// keep answering: a superseded plan is still there to read). Approving and applying are left
// to the worker-driven tests: under load a second plan supersedes the approver's hash, so
// measuring them here would measure refusals, not work.
// Same settings as health.js; scripts/load.sh gives this endpoint RATE 250 and P99_MS 250.

import http from 'k6/http';
import { check, fail } from 'k6';
import { parseHTML } from 'k6/html';

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
// Reported per kind of request, so a tail can be traced to its cause; only the overall p99 fails a run.
for (const endpoint of ['list', 'get', 'items', 'plan']) {
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
  const write = (key) => Object.assign({ 'Idempotency-Key': key }, headers);

  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-${Date.now().toString(36)}`, name: 'k6 migrations' }), { headers });
  if (project.status !== 201) fail(`project: HTTP ${project.status}`);
  const base = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;

  // A second collection, so plans and reads spread over two change locks.
  const notes = { name: 'Notes', title_field: 'title', fields: [{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false }] };
  const made = http.post(`${base}/content-types`, JSON.stringify({ slug: 'notes', definition: notes }), { headers: write('k6-setup-notes-0000') });
  if (made.status !== 201) fail(`notes type: HTTP ${made.status} ${made.body}`);

  const collections = [];
  for (const slug of ['articles', 'notes']) {
    for (let i = 0; i < 2; i += 1) {
      // Every field named must be declared: articles has body, notes only its title.
      const content = slug === 'articles' ? { title: `${slug} ${i}`, body: BODY } : { title: `${slug} ${i}` };
      const entry = http.post(`${base}/collections/${slug}/entries`, JSON.stringify(content), { headers: write(`k6-setup-entry-${slug}-${i}`) });
      if (entry.status !== 201) fail(`entry ${slug}: HTTP ${entry.status} ${entry.body}`);
    }
    const type = http.get(`${base}/content-types/${slug}`, { headers });
    if (type.status !== 200) fail(`type ${slug}: HTTP ${type.status}`);
    // The target adds a required field every stored draft lacks: each item is `needs_input`.
    // res.json() hands back a frozen object, so parse the body to get a definition push() takes.
    const target = JSON.parse(type.body).data.definition;
    target.fields.push({ id: 'subtitle', label: 'Subtitle', type: 'text', required: true, max_chars: 100, multiline: false });
    const planned = http.post(`${base}/migrations`, JSON.stringify({ collection: slug, definition: target }), { headers: write(`k6-setup-plan-${slug}`) });
    if (planned.status !== 201) fail(`plan ${slug}: HTTP ${planned.status} ${planned.body}`);
    collections.push({ slug, target, plan: planned.json('data.id') });
  }
  return { headers, base, collections };
}

export default function (data) {
  const slot = __ITER % 100;
  const collection = data.collections[(__VU + __ITER) % data.collections.length];
  const params = (endpoint, extra) => ({ headers: Object.assign({}, data.headers, extra || {}), redirects: 0, tags: { endpoint } });
  if (slot < 35) {
    const res = http.get(`${data.base}/migrations?limit=100`, params('list'));
    check(res, { 'list 200': (r) => r.status === 200 && r.body.indexOf('{"data":[') === 0 });
  } else if (slot < 45) {
    const filtered = slot % 2 === 0 ? `collection=${collection.slug}` : 'state=proposed';
    const res = http.get(`${data.base}/migrations?${filtered}`, params('list'));
    check(res, { 'filtered list 200': (r) => r.status === 200 && r.body.indexOf('{"data":[') === 0 });
  } else if (slot < 65) {
    const res = http.get(`${data.base}/migrations/${collection.plan}`, params('get'));
    check(res, { 'get 200': (r) => r.status === 200 && r.json('data.id') === collection.plan });
  } else if (slot < 80) {
    const res = http.get(`${data.base}/migrations/${collection.plan}/items?limit=50`, params('items'));
    check(res, { 'items 200': (r) => r.status === 200 && Array.isArray(r.json('data')) && r.json('data').length === 2 });
  } else if (slot < 90) {
    const res = http.get(`${data.base}/migrations/${collection.plan}/items?outcome=needs_input`, params('items'));
    check(res, { 'needs_input items 200': (r) => r.status === 200 && Array.isArray(r.json('data')) });
  } else {
    const res = http.post(`${data.base}/migrations`, JSON.stringify({ collection: collection.slug, definition: collection.target }), params('plan', { 'Idempotency-Key': `k6-${__VU}-${__ITER}-migrations` }));
    check(res, { 'plan 201': (r) => r.status === 201 && r.json('data.state') === 'proposed' });
  }
}
