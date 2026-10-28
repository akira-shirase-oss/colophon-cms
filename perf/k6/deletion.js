// Load scenario for deleting entries (D-080): every request is signed in, and every deletion is a
// real tombstone with If-Match and an Idempotency-Key. setup() signs in through Keycloak, creates
// a project, publishes 8 articles in one release and pins 8 more in a changeset that is never
// published: deleting those is always refused. Each iteration of a VU then either creates an
// article and deletes it (70%: the create, the delete with the ETag the create gave, and on every
// third such iteration a read that must now be 404, on every fifth the same delete again with the
// same key, which must replay), tries to delete a published or a pinned article (20%: 409
// `entry_published` or `entry_in_changeset`, alternately; once in four with a stale If-Match,
// 409 `stale_revision`), or lists the collection (10%), which must never show the article this VU
// deleted last.
//
// Refusals are expected answers, so those requests declare 409 as their expected status and
// http_req_failed still counts only what went wrong. Every request is named by its route (a
// `name` tag), so k6 keeps one series per route.
// Same SCENARIO, TOTAL, VUS, RATE, DURATION and P99_MS settings as entries.js; RATE counts
// iterations (about 2.3 requests each), and scripts/load.sh gives this endpoint RATE 200 and
// P99_MS 250.

import http from 'k6/http';
import { check, fail } from 'k6';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '200', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '250');
// Articles that readers get, and articles a changeset holds: deleting either is refused.
const KEPT = 8;

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const scenarios = {
  // TOTAL counts iterations here, as everywhere: a million of them send about 2.3 million requests.
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '60m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
// Per endpoint too, so a slow tail names its endpoint in the summary.
for (const endpoint of ['create', 'delete', 'gone', 'replay', 'refused', 'list']) {
  thresholds[`http_req_duration{endpoint:${endpoint}}`] = [`p(99)<${P99_MS}`];
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

const REFUSED = http.expectedStatuses(409);
const GONE = http.expectedStatuses(404);

function article(title) {
  return JSON.stringify({
    title,
    body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Short-lived' }] }] } },
  });
}

function key() {
  return `k6-${__VU}-${__ITER}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

/** Creates `count` articles; their ids and ETags. */
function createArticles(base, write, prefix, count) {
  const made = [];
  for (let i = 0; i < count; i++) {
    const created = http.post(base, article(`${prefix} ${i}`), { headers: Object.assign({ 'Idempotency-Key': `k6-setup-${prefix}-${Date.now().toString(36)}-${i}` }, write) });
    if (created.status !== 201) fail(`${prefix} ${i}: HTTP ${created.status} ${created.body}`);
    made.push({ id: created.json('data.id'), etag: created.headers['Etag'] });
  }
  return made;
}

/** A changeset holding `entries`; its path and If-Match. */
function changeset(environment, write, title, entries) {
  const set = http.post(`${environment}/changesets`, JSON.stringify({ title, entries: entries.map((entry) => entry.id) }), {
    headers: Object.assign({ 'Idempotency-Key': `k6-setup-${title.replace(/\W/g, '-')}-${Date.now().toString(36)}` }, write),
  });
  if (set.status !== 201) fail(`changeset ${title}: HTTP ${set.status} ${set.body}`);
  return { path: `${environment}/changesets/${set.json('data.id')}`, ifMatch: `"${set.json('data.hash')}"` };
}

export function setup() {
  const page = http.get(`${PUBLIC_URL}/auth/login?return_to=/me`);
  const action = parseHTML(page.body).find('#kc-form-login').attr('action');
  if (!action) fail('no Keycloak login form');
  const me = http.post(action.replace(/&amp;/g, '&'), { username: 'dev-owner', password: 'colophon-dev' });
  if (me.status !== 200) fail(`sign-in failed: HTTP ${me.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const session = { cookie: `colophon_session=${jar.colophon_session[0]}`, csrf: jar.colophon_csrf[0] };
  const write = { Cookie: session.cookie, 'X-CSRF-Token': session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };

  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 deletion' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const environment = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;
  const base = `${environment}/collections/articles/entries`;

  // Published: one release with KEPT articles, self-approved.
  const published = createArticles(base, write, 'Published', KEPT);
  const release = changeset(environment, write, 'k6 published', published);
  for (const [step, body] of [['submit', null], ['reviews', JSON.stringify({ decision: 'approve' })]]) {
    const done = http.post(`${release.path}/${step}`, body, { headers: Object.assign({ 'If-Match': release.ifMatch }, write) });
    if (done.status >= 300) fail(`${step}: HTTP ${done.status} ${done.body}`);
  }
  const releases = http.get(`${environment}/releases`, { headers: { Cookie: session.cookie } });
  const generation = parseInt(releases.headers['Colophon-Generation'], 10);
  const publish = http.post(`${release.path}/publish`, JSON.stringify({ base_generation: generation }), {
    headers: Object.assign({ 'If-Match': release.ifMatch, 'Idempotency-Key': `k6-setup-publish-${slug}` }, write),
  });
  if (publish.status !== 201) fail(`publish: HTTP ${publish.status} ${publish.body}`);
  // Pinned: a changeset holds them and is never published.
  const pinned = createArticles(base, write, 'Pinned', KEPT);
  changeset(environment, write, 'k6 pinned', pinned);
  return { session, base, published, pinned };
}

// Per VU: the article it deleted last, which no list may show again.
let lastDeleted = null;

export default function (data) {
  const write = {
    Cookie: data.session.cookie, 'X-CSRF-Token': data.session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json',
  };
  const roll = __ITER % 10;

  if (roll < 7) {
    const created = http.post(data.base, article(`Doomed by VU ${__VU}`), {
      headers: Object.assign({ 'Idempotency-Key': key() }, write), redirects: 0, tags: { endpoint: 'create', name: 'entries' },
    });
    if (!check(created, { 'create 201': (r) => r.status === 201 })) return;
    const id = created.json('data.id');
    const url = `${data.base}/${id}`;
    const once = { 'If-Match': created.headers['Etag'], 'Idempotency-Key': key(), 'X-CSRF-Token': data.session.csrf, Cookie: data.session.cookie, Origin: PUBLIC_URL };
    const deleted = http.del(url, null, { headers: once, redirects: 0, tags: { endpoint: 'delete', name: 'entries/{entry}' } });
    const ok = check(deleted, {
      'delete 200': (r) => r.status === 200,
      'delete names the entry': (r) => r.status === 200 && r.json('data.id') === id,
    });
    if (!ok) return;
    lastDeleted = id;
    if (__ITER % 3 === 0) {
      const gone = http.get(url, { headers: { Cookie: data.session.cookie }, redirects: 0, responseCallback: GONE, tags: { endpoint: 'gone', name: 'entries/{entry}' } });
      check(gone, { 'a deleted entry is 404': (r) => r.status === 404 });
    }
    if (__ITER % 5 === 0) {
      const again = http.del(url, null, { headers: once, redirects: 0, tags: { endpoint: 'replay', name: 'entries/{entry}' } });
      check(again, {
        'replay 200': (r) => r.status === 200,
        'replay is marked': (r) => r.headers['Idempotent-Replayed'] === 'true',
        'replay is the same answer': (r) => r.body === deleted.body,
      });
    }
  } else if (roll < 9) {
    const kept = roll === 7 ? data.published : data.pinned;
    const target = kept[__VU % kept.length];
    const stale = __ITER % 4 === 0;
    const headers = {
      'If-Match': stale ? '"00000000-0000-7000-8000-000000000000"' : target.etag, 'Idempotency-Key': key(),
      'X-CSRF-Token': data.session.csrf, Cookie: data.session.cookie, Origin: PUBLIC_URL,
    };
    const expected = stale ? 'stale_revision' : roll === 7 ? 'entry_published' : 'entry_in_changeset';
    const refused = http.del(`${data.base}/${target.id}`, null, { headers, redirects: 0, responseCallback: REFUSED, tags: { endpoint: 'refused', name: 'entries/{entry}' } });
    check(refused, {
      'refused 409': (r) => r.status === 409,
      'refused for the right reason': (r) => r.status === 409 && r.json('error.details.reason') === expected,
    });
  } else {
    const listed = http.get(`${data.base}?limit=100`, { headers: { Cookie: data.session.cookie }, redirects: 0, tags: { endpoint: 'list', name: 'entries?limit' } });
    check(listed, {
      'list 200': (r) => r.status === 200,
      'list never shows a deleted entry': (r) => lastDeleted === null || r.body.indexOf(lastDeleted) === -1,
    });
  }
}
