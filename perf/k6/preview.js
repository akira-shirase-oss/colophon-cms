// Load scenario for draft preview reads (gap 1; D-171, D-172). setup() signs in the
// administrator, makes a project with a preview address and 20 published articles, saves new
// drafts of five, and puts ten in a publish request; it opens an entry preview, a publish-request
// preview and one it revokes, exchanging each code for a bearer token. Every measured request
// carries a preview token (or a bad one) and is checked:
//   4 in 10 list the collection through the entry preview (20 rows, the draft among them),
//   2 in 10 read the drafted entry (source "draft", the draft's title),
//   1 in 10 query the collection through the entry preview,
//   1 in 10 list through the publish-request preview (ten "changeset" rows),
//   1 in 10 send a token nobody issued (401 preview_invalid, no database write),
//   1 in 10 send the revoked token (401 preview_revoked) — and one iteration in 200 opens a
//     fresh preview, exchanges its code, reads with it and revokes it (the exchange arm).
// Every answer must be `private, no-store` and `noindex`. A preview read runs as the issuer in
// one snapshot and never touches the delivery cache, so nothing here is a cache hit.
// The server bounds preview reads at 32 in flight (D-172); VUS defaults below that in
// scripts/load.sh so a throughput run measures the reads, not the bound — `stress` is where the
// bound's 503s are the expected failure mode.

import http from 'k6/http';
import { check, fail } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '24', 10);
const RATE = parseInt(__ENV.RATE || '200', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '150');

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const scenarios = {
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '60m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
  stress: {
    executor: 'ramping-arrival-rate', startRate: 50, timeUnit: '1s', preAllocatedVUs: 64, maxVUs: 512,
    stages: [{ target: 200, duration: '20s' }, { target: 1000, duration: '40s' }, { target: 2000, duration: '40s' }],
  },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
for (const endpoint of ['list', 'get', 'query', 'changeset', 'invalid', 'revoked', 'exchange']) {
  thresholds[`http_req_duration{endpoint:${endpoint}}`] = ['p(99)<60000'];
}
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}
if (SCENARIO === 'stress') {
  // The failure mode must be the bound's 503 (or admission's 429), never a timeout.
  delete thresholds.http_req_failed;
  delete thresholds.checks;
  thresholds['checks{kind:shed}'] = ['rate==1'];
}

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '180s',
};

const body = (title, text) => ({
  title,
  body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] } },
});

export function setup() {
  const signedIn = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }), { headers: { 'Content-Type': 'application/json' } });
  if (signedIn.status !== 204) fail(`sign-in failed: HTTP ${signedIn.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const headers = {
    Cookie: `colophon_session=${jar.colophon_session[0]}`, 'X-CSRF-Token': jar.colophon_csrf[0], Origin: PUBLIC_URL,
    'Content-Type': 'application/json',
  };
  const run = Date.now().toString(36);
  let n = 0;
  const key = () => `k6-preview-${run}-${(n += 1)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6p-${run}`, name: 'k6 preview' }), { headers });
  if (project.status !== 201) fail(`project: HTTP ${project.status}`);
  const projectId = project.json('data.id');
  const base = `${BASE_URL}/api/v1/projects/${projectId}/environments/production`;
  const target = http.put(`${base}/preview-target`, JSON.stringify({ url_template: 'http://localhost:58099/preview?code={code}&path=/blog/{id}' }), { headers });
  if (target.status !== 200) fail(`preview target: HTTP ${target.status} ${target.body}`);
  const entries = [];
  for (let i = 0; i < 20; i += 1) {
    const created = http.post(`${base}/collections/articles/entries`, JSON.stringify(body(`Article ${i}`, 'published')), { headers: Object.assign({ 'Idempotency-Key': key() }, headers) });
    if (created.status !== 201) fail(`entry: HTTP ${created.status}`);
    entries.push(created.json('data.id'));
  }
  const changeset = (ids) => {
    const set = http.post(`${base}/changesets`, JSON.stringify({ title: `set ${n}`, entries: ids }), { headers: Object.assign({ 'Idempotency-Key': key() }, headers) });
    if (set.status !== 201) fail(`changeset: HTTP ${set.status} ${set.body}`);
    return set;
  };
  const set = changeset(entries);
  const path = `${base}/changesets/${set.json('data.id')}`;
  const ifMatch = Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, headers);
  http.post(`${path}/submit`, null, { headers: ifMatch });
  http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch });
  const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: 0 }), { headers: Object.assign({ 'Idempotency-Key': key() }, ifMatch) });
  if (published.status !== 201) fail(`publish: HTTP ${published.status} ${published.body}`);
  for (const id of entries.slice(0, 10)) {
    const entryPath = `${base}/collections/articles/entries/${id}`;
    const current = http.get(entryPath, { headers });
    const saved = http.put(entryPath, JSON.stringify(body(`Draft ${id.slice(-6)}`, 'draft')), { headers: Object.assign({ 'If-Match': current.headers.Etag, 'Idempotency-Key': key() }, headers) });
    if (saved.status !== 200) fail(`save: HTTP ${saved.status}`);
  }
  const pending = changeset(entries.slice(0, 10));
  const exchange = (scope) => {
    const issued = http.post(`${base}/previews`, JSON.stringify(Object.assign({ ttl_seconds: 3600 }, scope)), { headers });
    if (issued.status !== 201) fail(`issue: HTTP ${issued.status} ${issued.body}`);
    const swapped = http.post(`${BASE_URL}/preview/v1/${projectId}/production/exchange`, JSON.stringify({ code: issued.json('data.code') }), { headers: { 'Content-Type': 'application/json' } });
    if (swapped.status !== 200) fail(`exchange: HTTP ${swapped.status} ${swapped.body}`);
    return { id: issued.json('data.id'), token: swapped.json('data.token') };
  };
  const entry = exchange({ entry: entries[0] });
  const request = exchange({ changeset: pending.json('data.id') });
  const revoked = exchange({ entry: entries[1] });
  const gone = http.del(`${base}/previews/${revoked.id}`, null, { headers });
  if (gone.status !== 204) fail(`revoke: HTTP ${gone.status}`);
  return {
    base, headers, projectId, entries,
    reads: `${BASE_URL}/preview/v1/${projectId}/production/articles`,
    drafted: entries[0], draftTitle: `Draft ${entries[0].slice(-6)}`,
    entryToken: entry.token, entryGrant: entry.id, changesetToken: request.token, revokedToken: revoked.token,
  };
}

const sealed = (r) => r.headers['Cache-Control'] === 'private, no-store' && (r.headers['X-Robots-Tag'] || '').startsWith('noindex');

export default function (data) {
  const slot = __ITER % 10;
  const as = (token, endpoint, extra = {}) => Object.assign({ headers: { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' }, tags: { endpoint } }, extra);
  const shed = (r) => r.status === 503 || r.status === 429;
  const judge = (res, name, ok) => {
    if (SCENARIO === 'stress') {
      check(res, { [`${name} or shed`]: (r) => ok(r) || shed(r) }, { kind: 'shed' });
    } else {
      check(res, { [name]: ok });
    }
  };
  // Keep the long-lived grant open past its hour on a full run: the Studio renews the same way.
  if (__ITER % 50000 === 0 && __ITER > 0) http.post(`${data.base}/previews/${data.entryGrant}/renew`, null, { headers: data.headers, tags: { endpoint: 'renew' } });
  if (slot < 4) {
    const res = http.get(data.reads, as(data.entryToken, 'list'));
    judge(res, 'list: 20 rows, the draft laid over', (r) => r.status === 200 && sealed(r) && r.json('data').length === 20 && r.json('data').some((row) => row.preview.source === 'draft'));
  } else if (slot < 6) {
    const res = http.get(`${data.reads}/${data.drafted}`, as(data.entryToken, 'get'));
    judge(res, 'get: the draft', (r) => r.status === 200 && sealed(r) && r.json('data.title') === data.draftTitle && r.json('data.preview.source') === 'draft');
  } else if (slot < 7) {
    const res = http.post(`${data.reads}/query`, JSON.stringify({ limit: 5 }), as(data.entryToken, 'query'));
    judge(res, 'query: a page of five', (r) => r.status === 200 && sealed(r) && r.json('data').length === 5);
  } else if (slot < 8) {
    const res = http.get(data.reads, as(data.changesetToken, 'changeset'));
    judge(res, 'changeset: ten pinned rows', (r) => r.status === 200 && sealed(r) && r.json('data').filter((row) => row.preview.source === 'changeset').length === 10);
  } else if (slot < 9) {
    const res = http.get(data.reads, as('cpv_nobodyissuedthistokenatallnobodyissuedthis', 'invalid', { responseCallback: http.expectedStatuses(401, 429, 503) }));
    judge(res, 'invalid: 401 preview_invalid', (r) => r.status === 401 && sealed(r) && r.json('error.details.reason') === 'preview_invalid');
  } else if (__ITER % 200 === 9) {
    const issued = http.post(`${data.base}/previews`, JSON.stringify({ entry: data.entries[__ITER % 20] }), { headers: data.headers, tags: { endpoint: 'exchange' } });
    const swapped = http.post(`${data.reads.replace('/articles', '')}/exchange`, JSON.stringify({ code: issued.json('data.code') }), { headers: { 'Content-Type': 'application/json' }, tags: { endpoint: 'exchange' } });
    judge(swapped, 'exchange: a token', (r) => r.status === 200 && sealed(r) && r.json('data.token').startsWith('cpv_'));
    http.del(`${data.base}/previews/${issued.json('data.id')}`, null, { headers: data.headers, tags: { endpoint: 'exchange' } });
  } else {
    const res = http.get(data.reads, as(data.revokedToken, 'revoked', { responseCallback: http.expectedStatuses(401, 429, 503) }));
    judge(res, 'revoked: 401 preview_revoked', (r) => r.status === 401 && sealed(r) && r.json('error.details.reason') === 'preview_revoked');
  }
}
