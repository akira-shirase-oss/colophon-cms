// Load scenario for deleting collections (D-083): every request is signed in, and every
// deletion carries If-Match and an Idempotency-Key. setup() signs in on the built-in password card (D-146), creates a
// project, and makes three collections that can never go: `full` holds a live entry, `linked` is
// what `linker`'s reference field points at, and `history` held an entry that is now a tombstone,
// its revisions still pinning the type's versions. Each iteration of a VU then either creates a
// collection and deletes it (60%: the create, the delete with If-Match "v1", and on every fifth
// such iteration the same delete again with the same key, which must replay), tries to delete one
// of the kept collections (30%: 409 `collection_not_empty`, `still_referenced` or
// `collection_has_history` in turn; once in four with a stale If-Match, 409 `stale_version`), or
// lists the content types (10%), which must never show the collection this VU deleted last.
//
// Refusals are expected answers, so those requests declare 409 as their expected status and
// http_req_failed still counts only what went wrong. Every request is named by its route (a
// `name` tag), so k6 keeps one series per route.
// Same SCENARIO, TOTAL, VUS, RATE, DURATION and P99_MS settings as deletion.js; RATE counts
// iterations (about 2.2 requests each), and scripts/load.sh gives this endpoint RATE 100 and
// P99_MS 250.

import http from 'k6/http';
import { check, fail } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '100', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '250');

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const scenarios = {
  // TOTAL counts iterations here, as everywhere: a million of them send about 2.2 million
  // requests, three in four of them writes.
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '90m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
// Per endpoint too, so a slow tail names its endpoint in the summary.
for (const endpoint of ['create', 'delete', 'replay', 'refused', 'list']) {
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

const SIMPLE = {
  name: 'Short-lived',
  title_field: 'title',
  fields: [{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 120 }],
};

function key() {
  return `k6-${__VU}-${__ITER}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

/** Creates the `slug` collection with `definition`; fails the run when it is not accepted. */
function collection(types, write, slug, definition) {
  const made = http.post(types, JSON.stringify({ slug, definition }), {
    headers: Object.assign({ 'Idempotency-Key': `k6-setup-${slug}-${Date.now().toString(36)}` }, write),
  });
  if (made.status !== 201) fail(`collection ${slug}: HTTP ${made.status} ${made.body}`);
}

export function setup() {
  const signedIn = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }), { headers: { 'Content-Type': 'application/json' } });
  if (signedIn.status !== 204) fail(`sign-in failed: HTTP ${signedIn.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const session = { cookie: `colophon_session=${jar.colophon_session[0]}`, csrf: jar.colophon_csrf[0] };
  const write = { Cookie: session.cookie, 'X-CSRF-Token': session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };

  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 collection delete' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const environment = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;
  const types = `${environment}/content-types`;

  // `full` always has a live entry; `linked` is what `linker` points at; `history` held an entry
  // that is now a tombstone. All three refuse deletion forever, each for its own reason.
  collection(types, write, 'full', SIMPLE);
  const entry = http.post(`${environment}/collections/full/entries`, JSON.stringify({ title: 'Kept' }), {
    headers: Object.assign({ 'Idempotency-Key': `k6-setup-entry-${slug}` }, write),
  });
  if (entry.status !== 201) fail(`entry: HTTP ${entry.status} ${entry.body}`);
  collection(types, write, 'linked', SIMPLE);
  collection(types, write, 'linker', {
    name: 'Linker',
    title_field: 'title',
    fields: [
      { id: 'title', label: 'Title', type: 'text', required: true, max_chars: 120 },
      { id: 'to', label: 'To', type: 'reference', to: 'linked' },
    ],
  });
  collection(types, write, 'history', SIMPLE);
  const ghost = http.post(`${environment}/collections/history/entries`, JSON.stringify({ title: 'Gone' }), {
    headers: Object.assign({ 'Idempotency-Key': `k6-setup-ghost-${slug}` }, write),
  });
  if (ghost.status !== 201) fail(`ghost entry: HTTP ${ghost.status} ${ghost.body}`);
  const tombstoned = http.del(`${environment}/collections/history/entries/${ghost.json('data.id')}`, null, {
    headers: { 'If-Match': ghost.headers['Etag'], 'Idempotency-Key': `k6-setup-tombstone-${slug}`, 'X-CSRF-Token': session.csrf, Cookie: session.cookie, Origin: PUBLIC_URL },
  });
  if (tombstoned.status !== 200) fail(`tombstone: HTTP ${tombstoned.status} ${tombstoned.body}`);
  return { session, types };
}

// Per VU: the collection it deleted last, which no list may show again.
let lastDeleted = null;

export default function (data) {
  const write = {
    Cookie: data.session.cookie, 'X-CSRF-Token': data.session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json',
  };
  const roll = __ITER % 10;

  if (roll < 6) {
    const slug = `vu-${__VU}-${__ITER.toString(36)}`;
    const created = http.post(data.types, JSON.stringify({ slug, definition: SIMPLE }), {
      headers: Object.assign({ 'Idempotency-Key': key() }, write), redirects: 0, tags: { endpoint: 'create', name: 'content-types' },
    });
    if (!check(created, { 'create 201': (r) => r.status === 201 })) return;
    const url = `${data.types}/${slug}`;
    const once = { 'If-Match': '"v1"', 'Idempotency-Key': key(), 'X-CSRF-Token': data.session.csrf, Cookie: data.session.cookie, Origin: PUBLIC_URL };
    const deleted = http.del(url, null, { headers: once, redirects: 0, tags: { endpoint: 'delete', name: 'content-types/{slug}' } });
    const ok = check(deleted, {
      'delete 200': (r) => r.status === 200,
      'delete names the collection': (r) => r.status === 200 && r.json('data.slug') === slug && r.json('data.removed') === true,
    });
    if (!ok) return;
    lastDeleted = slug;
    if (__ITER % 5 === 0) {
      const again = http.del(url, null, { headers: once, redirects: 0, tags: { endpoint: 'replay', name: 'content-types/{slug}' } });
      check(again, {
        'replay 200': (r) => r.status === 200,
        'replay is marked': (r) => r.headers['Idempotent-Replayed'] === 'true',
        'replay is the same answer': (r) => r.body === deleted.body,
      });
    }
  } else if (roll < 9) {
    const kept = ['full', 'linked', 'history'][__ITER % 3];
    const stale = __ITER % 4 === 0;
    const headers = {
      'If-Match': stale ? '"v99"' : '"v1"', 'Idempotency-Key': key(),
      'X-CSRF-Token': data.session.csrf, Cookie: data.session.cookie, Origin: PUBLIC_URL,
    };
    // A schema referrer refuses before the version is even read (D-073's lock order puts it
    // first), so `linked` says still_referenced however stale the If-Match.
    const expected = kept === 'linked' ? 'still_referenced' : stale ? 'stale_version' : kept === 'full' ? 'collection_not_empty' : 'collection_has_history';
    const refused = http.del(`${data.types}/${kept}`, null, { headers, redirects: 0, responseCallback: REFUSED, tags: { endpoint: 'refused', name: 'content-types/{slug}' } });
    check(refused, {
      'refused 409': (r) => r.status === 409,
      'refused for the right reason': (r) => r.status === 409 && r.json('error.details.reason') === expected,
    });
  } else {
    const listed = http.get(data.types, { headers: { Cookie: data.session.cookie }, redirects: 0, tags: { endpoint: 'list', name: 'content-types' } });
    check(listed, {
      'list 200': (r) => r.status === 200,
      'list never shows a deleted collection': (r) => lastDeleted === null || r.body.indexOf(`"slug":"${lastDeleted}"`) === -1,
    });
  }
}
