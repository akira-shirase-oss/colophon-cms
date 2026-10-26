// Load scenario for the editorial pipeline and the worker (step 2.3, D-046). setup() signs in the
// administrator and creates one project per virtual user, each with a webhook destination: the
// receiver scripts/load.sh starts. Each iteration publishes one new article in the virtual
// user's own project: create the entry, pin it into a changeset, submit, approve, publish on the
// generation it knows (five requests; nobody else publishes there). Meanwhile the worker
// announces every release to the receiver; a release committed before the previous one was sent
// supersedes it. teardown() waits for every project's latest release to be delivered.
// TOTAL counts requests, as elsewhere: a full run is 200,000 publishes. RATE counts publishes;
// scripts/load.sh gives this endpoint RATE 100 (500 requests a second) and P99_MS 250.

import http from 'k6/http';
import { check, fail, sleep } from 'k6';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const RECEIVER_URL = __ENV.RECEIVER_URL || 'http://127.0.0.1:58096';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '100', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '250');
const REQUESTS_PER_PUBLISH = 5;

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const scenarios = {
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: Math.ceil(TOTAL / REQUESTS_PER_PUBLISH), maxDuration: '30m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);
const projects = SCENARIO === 'steady' ? VUS * 2 : VUS;

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
for (const endpoint of ['entry', 'changeset', 'submit', 'review', 'publish']) {
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
  teardownTimeout: '120s',
  // Every request has its own URL here; without this, a million of them are a million series.
  systemTags: ['status', 'method', 'name', 'scenario', 'check', 'error', 'error_code', 'expected_response'],
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
  const run = Date.now().toString(36);
  const bases = [];
  for (let i = 0; i < projects; i += 1) {
    const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-${run}-${i}`, name: `k6 publishing ${i}` }), { headers });
    if (project.status !== 201) fail(`project: HTTP ${project.status}`);
    const base = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;
    const destination = http.post(`${base}/destinations`, JSON.stringify({ name: 'receiver', url: `${RECEIVER_URL}/hook` }), { headers });
    if (destination.status !== 201) fail(`destination: HTTP ${destination.status} ${destination.body}`);
    bases.push(base);
  }
  return { headers, bases, run };
}

// Each virtual user publishes in its own project, so it knows the generation it publishes on.
let generation = 0;

const article = (title) => ({
  title,
  body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: title }] }] } },
});

export default function (data) {
  const base = data.bases[(__VU - 1) % data.bases.length];
  const key = (step) => `k6-${data.run}-${__VU}-${__ITER}-${step}`;
  const params = (endpoint, extra) => ({ headers: Object.assign({}, data.headers, extra || {}), redirects: 0, tags: { endpoint, name: endpoint } });

  const entry = http.post(`${base}/collections/articles/entries`, JSON.stringify(article(`Article ${__VU}-${__ITER}`)), params('entry', { 'Idempotency-Key': key('e') }));
  if (!check(entry, { 'entry 201': (r) => r.status === 201 })) return;
  const set = http.post(`${base}/changesets`, JSON.stringify({ title: `k6 ${__ITER}`, entries: [entry.json('data.id')] }), params('changeset', { 'Idempotency-Key': key('c') }));
  if (!check(set, { 'changeset 201': (r) => r.status === 201 })) return;
  const path = `${base}/changesets/${set.json('data.id')}`;
  const ifMatch = { 'If-Match': `"${set.json('data.hash')}"` };
  const submitted = http.post(`${path}/submit`, null, params('submit', ifMatch));
  if (!check(submitted, { 'submit 200': (r) => r.status === 200 })) return;
  const approved = http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), params('review', ifMatch));
  if (!check(approved, { 'approved': (r) => r.status === 200 && r.json('data.status.state') === 'approved' })) return;
  const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: generation }), params('publish', Object.assign({ 'Idempotency-Key': key('p') }, ifMatch)));
  if (check(published, { 'publish 201': (r) => r.status === 201 })) {
    generation = published.json('data.generation');
  }
}

export function teardown(data) {
  // Every project's latest release must reach the receiver; older ones may have been superseded.
  const deadline = Date.now() + 90000;
  for (const base of data.bases) {
    for (;;) {
      const jobs = http.get(`${base}/jobs`, { headers: data.headers });
      const latest = jobs.json('data')[0];
      if (!latest || latest.status === 'succeeded') break;
      if (latest.status === 'failed') fail(`the latest release of ${base} failed: ${JSON.stringify(latest)}`);
      if (Date.now() > deadline) fail(`the latest release of ${base} was not delivered in time: ${JSON.stringify(latest)}`);
      sleep(0.2);
    }
  }
  const stats = http.get(`${RECEIVER_URL}/stats`).json();
  console.log(`receiver: ${stats.requests} announcements, ${stats.distinct_keys} distinct keys`);
}
