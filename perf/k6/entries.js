// Load scenario for drafts (step 1.2): every request is signed in, and writes are real saves.
// setup() signs in through Keycloak, creates a project, and gives every virtual user its own
// article. Each VU then reads its article (70%), lists the collection (10%), or saves a new
// revision with If-Match and a fresh Idempotency-Key (20%).
//
// "No lost accepted revision" (END_GOAL) is a check, not a hope: a VU is the only writer of its
// article, so every read must return exactly the revision that VU's last save was given.
// Same SCENARIO, TOTAL, VUS, RATE, DURATION and P99_MS settings as health.js; scripts/load.sh
// gives this endpoint RATE 500 and P99_MS 250.

import http from 'k6/http';
import { check, fail } from 'k6';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '500', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '250');

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const scenarios = {
  // Writes are several round trips each, so a million requests take longer than a read-only run.
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '40m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);
// One article per VU that can exist, so no two VUs ever save the same article.
const ARTICLES = SCENARIO === 'steady' ? VUS * 2 : VUS;

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '120s',
};

function article(title, text) {
  return JSON.stringify({
    title,
    body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] } },
  });
}

function key() {
  return `k6-${__VU}-${__ITER}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export function setup() {
  const page = http.get(`${PUBLIC_URL}/auth/login?return_to=/me`);
  const action = parseHTML(page.body).find('#kc-form-login').attr('action');
  if (!action) fail('no Keycloak login form');
  const me = http.post(action.replace(/&amp;/g, '&'), { username: 'dev-editor', password: 'colophon-dev' });
  if (me.status !== 200) fail(`sign-in failed: HTTP ${me.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const session = { cookie: `colophon_session=${jar.colophon_session[0]}`, csrf: jar.colophon_csrf[0] };
  const write = { Cookie: session.cookie, 'X-CSRF-Token': session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };

  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 drafts' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const base = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production/collections/articles/entries`;

  const articles = [];
  for (let i = 0; i < ARTICLES; i++) {
    const created = http.post(base, article(`Article ${i}`, 'Start'), { headers: Object.assign({ 'Idempotency-Key': `k6-setup-${slug}-${i}` }, write) });
    if (created.status !== 201) fail(`article ${i}: HTTP ${created.status} ${created.body}`);
    articles.push({ id: created.json('data.id'), etag: created.headers['Etag'] });
  }
  return { session, base, articles };
}

// Per VU: the ETag of the last revision this VU saved (each VU has its own copy).
let etag = null;

export default function (data) {
  const mine = data.articles[(__VU - 1) % data.articles.length];
  if (etag === null) etag = mine.etag;
  const url = `${data.base}/${mine.id}`;
  const read = { headers: { Cookie: data.session.cookie }, redirects: 0 };
  const roll = __ITER % 10;

  if (roll < 2) {
    const headers = {
      Cookie: data.session.cookie, 'X-CSRF-Token': data.session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json',
      'If-Match': etag, 'Idempotency-Key': key(),
    };
    const res = http.put(url, article(`Article by VU ${__VU}`, `Iteration ${__ITER}`), { headers, redirects: 0, tags: { endpoint: 'save' } });
    const saved = check(res, {
      'save 200': (r) => r.status === 200,
      'save gives a new ETag': (r) => typeof r.headers['Etag'] === 'string' && r.headers['Etag'] !== etag,
    });
    if (saved) etag = res.headers['Etag'];
  } else if (roll < 3) {
    const res = http.get(`${data.base}?limit=20`, Object.assign({ tags: { endpoint: 'list' } }, read));
    check(res, {
      'list 200': (r) => r.status === 200,
      'list is a page': (r) => r.body.indexOf('{"data":[') === 0,
    });
  } else {
    const res = http.get(url, Object.assign({ tags: { endpoint: 'read' } }, read));
    check(res, {
      'read 200': (r) => r.status === 200,
      'read returns the last accepted save': (r) => r.headers['Etag'] === etag,
    });
  }
}
