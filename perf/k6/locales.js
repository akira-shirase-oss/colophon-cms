// Load scenario for locales (D-087): each VU owns an English note and its French sibling in
// one localization group. Each iteration either saves the French draft with its locale
// riding the write (40%: a revision plus the locale UPDATE in one transaction), reads the
// source and checks its group lists both members exactly once (30%), or lists the collection
// filtered to the group's locales (30%). Requests are named by route, so k6 keeps one series
// per route. scripts/load.sh gives this endpoint RATE 250 and P99_MS 700 (END_GOAL's
// editorial write budget: a save writes a revision plus the locale row). The pool covers
// maxVUs, not VUS: a slow patch spawns extra VUs, and two sharing one entry would race its
// ETag forever.

import http from 'k6/http';
import { check, fail } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '250', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '700');

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

const NOTES = {
  name: 'Notes',
  title_field: 'title',
  fields: [{ id: 'title', label: 'Title', type: 'text', required: true }],
};

function key() {
  return `k6-${__VU}-${__ITER}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export function setup() {
  const signedIn = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }), { headers: { 'Content-Type': 'application/json' } });
  if (signedIn.status !== 204) fail(`sign-in failed: HTTP ${signedIn.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const session = { cookie: `colophon_session=${jar.colophon_session[0]}`, csrf: jar.colophon_csrf[0] };
  const write = { Cookie: session.cookie, 'X-CSRF-Token': session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };

  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 locales' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const env = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;
  const made = http.post(`${env}/content-types`, JSON.stringify({ slug: 'notes', definition: NOTES }), {
    headers: Object.assign({ 'Idempotency-Key': `k6-type-${slug}` }, write),
  });
  if (made.status !== 201) fail(`content type notes: HTTP ${made.status} ${made.body}`);
  const entries = `${env}/collections/notes/entries`;

  const vus = [];
  for (let i = 0; i < VUS * 2; i++) {
    const source = http.post(`${entries}?locale=en`, JSON.stringify({ title: `Welcome ${i}` }), {
      headers: Object.assign({ 'Idempotency-Key': `k6-setup-${slug}-${i}-en` }, write),
    });
    if (source.status !== 201) fail(`note: HTTP ${source.status} ${source.body}`);
    const sibling = http.post(`${entries}/${source.json('data.id')}/localizations?locale=fr`, null, {
      headers: Object.assign({ 'Idempotency-Key': `k6-setup-${slug}-${i}-fr` }, write),
    });
    if (sibling.status !== 201) fail(`localize: HTTP ${sibling.status} ${sibling.body}`);
    vus.push({ source: source.json('data.id'), sibling: sibling.json('data.id'), etag: sibling.headers['Etag'] });
  }
  return { session, entries, vus };
}

let etag = null;

export default function (data) {
  const mine = data.vus[(__VU - 1) % data.vus.length];
  if (etag === null) etag = mine.etag;
  const read = { headers: { Cookie: data.session.cookie }, redirects: 0 };
  const roll = __ITER % 10;

  if (roll < 4) {
    const headers = {
      Cookie: data.session.cookie, 'X-CSRF-Token': data.session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json',
      'If-Match': etag, 'Idempotency-Key': key(),
    };
    const res = http.put(`${data.entries}/${mine.sibling}?locale=fr`, JSON.stringify({ title: `Bienvenue ${__ITER}` }), {
      headers, redirects: 0, tags: { endpoint: 'save', name: 'notes/{entry}' },
    });
    const saved = check(res, {
      'save 200': (r) => r.status === 200,
      'save keeps its locale': (r) => r.json('data.locale') === 'fr',
      'save gives a new ETag': (r) => typeof r.headers['Etag'] === 'string' && r.headers['Etag'] !== etag,
    });
    if (saved) etag = res.headers['Etag'];
  } else if (roll < 7) {
    const res = http.get(`${data.entries}/${mine.source}`, Object.assign({ tags: { endpoint: 'read', name: 'notes/{entry}' } }, read));
    check(res, {
      'read 200': (r) => r.status === 200,
      'group holds each member exactly once': (r) => {
        const rows = r.json('data.localizations') || [];
        const ids = rows.map((row) => row.id);
        return rows.length === 2 && ids.includes(mine.source) && ids.includes(mine.sibling);
      },
    });
  } else {
    const res = http.get(`${data.entries}?limit=100&locale=fr`, Object.assign({ tags: { endpoint: 'filter', name: 'notes' } }, read));
    check(res, {
      'filter 200': (r) => r.status === 200,
      'filter keeps only French rows': (r) => (r.json('data') || []).every((row) => row.locale === 'fr'),
    });
  }
}
