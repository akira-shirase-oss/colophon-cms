// Load scenario for the reference site (step 3.4, D-052): readers on the static site while
// releases keep coming. setup() signs in the administrator, creates a project with 20 articles
// and a webhook destination at the site runner scripts/load.sh starts, publishes them as release
// 1 and waits for the site to show it. Then readers read (TOTAL requests back to back, or RATE a
// second steady), each the home page or one article, while the first virtual user also edits one
// article and publishes it as a new release every two seconds, so the runner builds and promotes
// underneath the readers the whole time. Every page read must be whole: 200, with exactly one
// release marker; only reads are measured. teardown() waits for the site to show the last release.
// scripts/load.sh gives this endpoint RATE 2000 and P99_MS 100.

import http from 'k6/http';
import { check, fail, sleep } from 'k6';
import { parseHTML } from 'k6/html';
import { Counter } from 'k6/metrics';

const releasesPublished = new Counter('releases_published');

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SITE_URL = __ENV.SITE_URL || 'http://127.0.0.1:58099';
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
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '30m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);

// Reads are what's measured; the publishing requests have their own tag and no limit.
const thresholds = {
  'http_req_failed{kind:read}': ['rate==0'],
  checks: ['rate==1'],
  'http_req_duration{kind:read}': [`p(99)<${P99_MS}`],
  'http_req_duration{page:home}': ['p(99)<60000'],
  'http_req_duration{page:article}': ['p(99)<60000'],
  releases_published: ['count>=2'],
};
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '120s',
  teardownTimeout: '120s',
};

const body = (title, text) => ({
  title,
  body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] } },
});

/** The release the live site is, from its generation.txt. */
function live() {
  const response = http.get(`${SITE_URL}/generation.txt`, { tags: { page: 'generation' } });
  return response.status === 200 ? parseInt(response.body, 10) : 0;
}

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
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-site-${run}`, name: 'k6 site' }), { headers });
  if (project.status !== 201) fail(`project: HTTP ${project.status}`);
  const base = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;
  const hook = http.post(`${base}/destinations`, JSON.stringify({ name: 'site', url: `${SITE_URL}/hook` }), { headers });
  if (hook.status !== 201) fail(`destination: HTTP ${hook.status} ${hook.body}`);
  const entries = [];
  for (let i = 0; i < 20; i += 1) {
    const created = http.post(`${base}/collections/articles/entries`, JSON.stringify(body(`Article ${i}`, 'first')), { headers: Object.assign({ 'Idempotency-Key': `k6-site-${run}-e${i}` }, headers) });
    if (created.status !== 201) fail(`entry: HTTP ${created.status}`);
    entries.push(created.json('data.id'));
  }
  const data = { headers, base, entries, run };
  releaseOf(data, entries, 0, 'setup');
  for (let waited = 0; live() < 1; waited += 1) {
    if (waited > 300) fail('the site never showed release 1');
    sleep(0.1);
  }
  return data;
}

/** Publishes `ids` as the release after `generation`; the new generation. */
function releaseOf(data, ids, generation, tag) {
  const params = (extra) => ({ headers: Object.assign({}, data.headers, extra || {}), tags: { page: 'publish' } });
  const key = (step) => `k6-site-${data.run}-${tag}-${step}`;
  const set = http.post(`${data.base}/changesets`, JSON.stringify({ title: `release ${generation + 1}`, entries: ids }), params({ 'Idempotency-Key': key('c') }));
  const path = `${data.base}/changesets/${set.json('data.id')}`;
  const ifMatch = { 'If-Match': `"${set.json('data.hash')}"` };
  http.post(`${path}/submit`, null, params(ifMatch));
  http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), params(ifMatch));
  const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: generation }), params(Object.assign({ 'Idempotency-Key': key('p') }, ifMatch)));
  if (published.status !== 201) fail(`publish: HTTP ${published.status} ${published.body}`);
  releasesPublished.add(1);
  return published.json('data.generation');
}

// The first virtual user's publishing: the release it last published, and when.
let generation = 1;
let lastPublish = 0;

function publish(data) {
  const id = data.entries[generation % data.entries.length];
  const path = `${data.base}/collections/articles/entries/${id}`;
  const current = http.get(path, { headers: data.headers, tags: { page: 'publish' } });
  const saved = http.put(path, JSON.stringify(body(current.json('data.content.title'), `edit ${generation}`)), {
    headers: Object.assign({ 'If-Match': current.headers.Etag, 'Idempotency-Key': `k6-site-${data.run}-s${generation}` }, data.headers),
    tags: { page: 'publish' },
  });
  if (saved.status !== 200) fail(`save: HTTP ${saved.status}`);
  generation = releaseOf(data, [id], generation, `r${generation}`);
}

export default function (data) {
  if (__VU === 1 && Date.now() - lastPublish >= 2000) {
    lastPublish = Date.now();
    publish(data);
  }
  const article = __ITER % 2 === 1;
  const path = article ? `/articles/${data.entries[__ITER % data.entries.length]}/` : '/';
  const response = http.get(`${SITE_URL}${path}`, { tags: { page: article ? 'article' : 'home', kind: 'read' } });
  check(response, {
    'a whole page, marked with one release': (r) =>
      r.status === 200 && (r.body.match(/<meta name="colophon-generation" content="\d+">/g) || []).length === 1,
  });
}

export function teardown(data) {
  const listed = http.get(`${data.base}/releases`, { headers: data.headers });
  const latest = parseInt(listed.headers['Colophon-Generation'], 10);
  for (let waited = 0; live() < latest; waited += 1) {
    if (waited > 600) fail(`the site never showed release ${latest}; it shows ${live()}`);
    sleep(0.1);
  }
  console.log(`site: shows release ${live()} of ${latest}`);
}
