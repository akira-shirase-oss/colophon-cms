// Load scenario for content types (step 1.5): entries of a seven-field type, so every save runs
// the full schema validation (decimal and whole numbers, a choice, a flag, a UTC time, text and
// rich text) and every read can fetch the type's definition. setup() signs in, defines the type,
// and gives each VU its own entry. Each VU reads its entry (60%), reads the content type (10%),
// lists the collection (10%) or saves new values with If-Match and a fresh key (20%); every
// read of its entry must return the ETag of its last accepted save.
// Same settings as health.js; scripts/load.sh gives this endpoint RATE 500 and P99_MS 250.

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
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '40m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);
const ENTRIES = SCENARIO === 'steady' ? VUS * 2 : VUS;

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

const PRODUCTS = {
  name: 'Products',
  title_field: 'name',
  fields: [
    { id: 'name', label: 'Name', type: 'text', required: true, max_chars: 120 },
    { id: 'price', label: 'Price', type: 'number', min: 0, max: 100000 },
    { id: 'stock', label: 'Stock', type: 'number', integer: true, min: 0 },
    { id: 'category', label: 'Category', type: 'enum', required: true, values: ['Books', 'Tools', 'Games'] },
    { id: 'on_sale', label: 'On sale', type: 'boolean' },
    { id: 'launch', label: 'Launch', type: 'datetime' },
    { id: 'description', label: 'Description', type: 'rich_text' },
  ],
};

function product(name, n) {
  return JSON.stringify({
    name,
    price: (n % 100000) / 100,
    stock: n % 5000,
    category: ['Books', 'Tools', 'Games'][n % 3],
    on_sale: n % 2 === 0,
    launch: `2027-0${1 + (n % 9)}-1${n % 10}T12:00:00Z`,
    description: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: `Edition ${n}` }] }] } },
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
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 content types' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const env = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;
  const type = http.post(`${env}/content-types`, JSON.stringify({ slug: 'products', definition: PRODUCTS }), {
    headers: Object.assign({ 'Idempotency-Key': `k6-type-${slug}` }, write),
  });
  if (type.status !== 201) fail(`content type: HTTP ${type.status} ${type.body}`);
  const base = `${env}/collections/products/entries`;

  const entries = [];
  for (let i = 0; i < ENTRIES; i++) {
    const created = http.post(base, product(`Product ${i}`, i), { headers: Object.assign({ 'Idempotency-Key': `k6-setup-${slug}-${i}` }, write) });
    if (created.status !== 201) fail(`entry ${i}: HTTP ${created.status} ${created.body}`);
    entries.push({ id: created.json('data.id'), etag: created.headers['Etag'] });
  }
  return { session, base, type: `${env}/content-types/products`, entries };
}

let etag = null;

export default function (data) {
  const mine = data.entries[(__VU - 1) % data.entries.length];
  if (etag === null) etag = mine.etag;
  const url = `${data.base}/${mine.id}`;
  const read = { headers: { Cookie: data.session.cookie }, redirects: 0 };
  const roll = __ITER % 10;

  if (roll < 2) {
    const headers = {
      Cookie: data.session.cookie, 'X-CSRF-Token': data.session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json',
      'If-Match': etag, 'Idempotency-Key': key(),
    };
    const res = http.put(url, product(`Product by VU ${__VU}`, __ITER + __VU), { headers, redirects: 0, tags: { endpoint: 'save' } });
    const saved = check(res, {
      'save 200': (r) => r.status === 200,
      'save gives a new ETag': (r) => typeof r.headers['Etag'] === 'string' && r.headers['Etag'] !== etag,
    });
    if (saved) etag = res.headers['Etag'];
  } else if (roll < 3) {
    const res = http.get(data.type, Object.assign({ tags: { endpoint: 'type' } }, read));
    check(res, {
      'type 200': (r) => r.status === 200,
      'type is version 1': (r) => r.headers['Etag'] === '"v1"',
    });
  } else if (roll < 4) {
    const res = http.get(`${data.base}?limit=20`, Object.assign({ tags: { endpoint: 'list' } }, read));
    check(res, { 'list 200': (r) => r.status === 200 });
  } else {
    const res = http.get(url, Object.assign({ tags: { endpoint: 'read' } }, read));
    check(res, {
      'read 200': (r) => r.status === 200,
      'read returns the last accepted save': (r) => r.headers['Etag'] === etag,
    });
  }
}
