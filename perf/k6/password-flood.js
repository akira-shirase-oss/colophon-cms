// Load scenario for D-213: one client address floods password sign-in while anonymous readers
// read delivery beside it. Sign-in is unauthenticated and costs Argon2id, so without a limit of
// its own one address could keep every password slot busy; the password admission class refuses
// past its burst before any hashing (COLOPHON_RATE_LIMIT_PASSWORD, 5 a second and a burst of
// 10 — scripts/load.sh leaves production's default in place for this scenario alone).
//
// Two phases of DURATION each, GAP seconds apart: anonymous list reads alone at RATE (the
// baseline), then the same reads while the flood sends FLOOD_RATE sign-ins a second with a
// wrong password, every one from this machine's address. Every request is tagged by arm
// (`baseline`, `reads`, `flood`). The run holds when, under the flood:
//   - the reads keep their p99 budget (P99_MS) — the evidence that a flood can't starve them;
//   - the flood got through no more than its burst plus what the rate earns back over the
//     phase (`flood_admitted`), so it never held more password slots than that;
//   - every refused attempt was 429 with Retry-After, and every admitted one answered as a
//     wrong password does (400 sign_in_failed).
// handleSummary() also reports the readers' p95 before and during the flood, for the record.
// scripts/load.sh gives this endpoint RATE 200 and P99_MS 250.

import http from 'k6/http';
import { check, fail } from 'k6';
import { Counter } from 'k6/metrics';
import { idempotencyKey } from './lib/idempotency-key.js';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const RATE = parseInt(__ENV.RATE || '200', 10);
const FLOOD_RATE = parseInt(__ENV.FLOOD_RATE || '200', 10);
const DURATION = __ENV.DURATION || '30s';
const P99_MS = parseFloat(__ENV.P99_MS || '250');
// The limit under test (production's default) — what the flood may get through.
const PER_SECOND = parseFloat(__ENV.PASSWORD_PER_SECOND || '5');
const BURST = parseInt(__ENV.PASSWORD_BURST || '10', 10);
const VUS = parseInt(__ENV.VUS || `${Math.max(16, Math.ceil(RATE / 10))}`, 10);
const FLOOD_VUS = parseInt(__ENV.FLOOD_VUS || `${Math.max(16, Math.ceil(FLOOD_RATE / 10))}`, 10);
const GAP = 2;

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 30s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}
const PHASE = durationSeconds(DURATION);

if (!(SCENARIO === 'throughput' || SCENARIO === 'steady')) {
  throw new Error('SCENARIO must be throughput or steady');
}

const floodAdmitted = new Counter('flood_admitted');

function arm(rate, start, vus, execFn) {
  return {
    executor: 'constant-arrival-rate',
    rate,
    timeUnit: '1s',
    duration: DURATION,
    startTime: `${start}s`,
    preAllocatedVUs: vus,
    maxVUs: vus * 2,
    exec: execFn,
  };
}

// One second of slack on the earned rate: the phase's edges and the limiter's clock.
const ALLOWED = BURST + Math.ceil(PER_SECOND * (PHASE + 1));

export const options = {
  scenarios: {
    baseline: arm(RATE, 0, VUS, 'baseline'),
    reads: arm(RATE, PHASE + GAP, VUS, 'reads'),
    flood: arm(FLOOD_RATE, PHASE + GAP, FLOOD_VUS, 'flood'),
  },
  thresholds: {
    checks: ['rate==1'],
    'http_req_duration{arm:reads}': [`p(99)<${P99_MS}`],
    'http_req_duration{arm:baseline}': ['p(99)<60000'],
    'http_req_duration{arm:flood}': ['p(99)<60000'],
    flood_admitted: [`count<=${ALLOWED}`],
    'http_reqs{arm:flood}': ['count>0'],
  },
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '120s',
};

const body = (title, text) => ({
  title,
  body: {
    schema: 'colophon/rich-text',
    version: 1,
    doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] },
  },
});

// A project with one published public article, and its delivery list: what readers read.
export function setup() {
  const signedIn = http.post(
    `${PUBLIC_URL}/auth/signin`,
    JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }),
    { headers: { 'Content-Type': 'application/json', Origin: PUBLIC_URL } },
  );
  if (signedIn.status !== 204) fail(`sign-in failed: HTTP ${signedIn.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const headers = {
    Cookie: `__Host-colophon_session=${jar['__Host-colophon_session'][0]}`,
    'X-CSRF-Token': jar['__Host-colophon_csrf'][0],
    Origin: PUBLIC_URL,
    'Content-Type': 'application/json',
  };
  const run = Date.now().toString(36);
  let n = 0;
  const key = () => ({ 'Idempotency-Key': idempotencyKey(`k6-flood-${run}-${(n += 1)}`, 'password-flood') });
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-flood-${run}`, name: 'k6 password flood' }), { headers });
  if (project.status !== 201) fail(`project: HTTP ${project.status}`);
  const projectId = project.json('data.id');
  const base = `${BASE_URL}/api/v1/projects/${projectId}`;
  const role = http.post(`${base}/audience/roles`, JSON.stringify({ name: 'member', self_assign: true }), { headers: Object.assign(key(), headers) });
  if (role.status !== 201) fail(`role: HTTP ${role.status} ${role.body}`);
  const entries = `${base}/environments/production/collections/articles/entries`;
  const created = http.post(`${entries}?visibility=public`, JSON.stringify(body('For everyone', 'text')), { headers: Object.assign(key(), headers) });
  if (created.status !== 201) fail(`entry: HTTP ${created.status} ${created.body}`);
  const set = http.post(`${base}/environments/production/changesets`, JSON.stringify({ title: 'Out', entries: [created.json('data.id')] }), { headers: Object.assign(key(), headers) });
  const path = `${base}/environments/production/changesets/${set.json('data.id')}`;
  const ifMatch = Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, headers);
  http.post(`${path}/submit`, null, { headers: ifMatch });
  http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch });
  const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: 0 }), { headers: Object.assign(key(), ifMatch) });
  if (published.status !== 201) fail(`publish: HTTP ${published.status} ${published.body}`);
  return {
    list: `${BASE_URL}/content/v1/${projectId}/production/articles`,
    signin: `${BASE_URL}/audience/v1/${projectId}/signin`,
  };
}

function read(data, armName) {
  const res = http.get(data.list, { redirects: 0, tags: { arm: armName } });
  check(res, { 'reads: the public article, 200': (r) => r.status === 200 && r.json('data').length === 1 });
}

export function baseline(data) {
  read(data, 'baseline');
}

export function reads(data) {
  read(data, 'reads');
}

// A wrong password from this machine's address: admitted ones hash (and answer 400
// sign_in_failed, the same as any wrong password); past the burst, 429 before any work.
export function flood(data) {
  const res = http.post(
    data.signin,
    JSON.stringify({ email: 'nobody@example.test', password: 'a guess at it' }),
    {
      headers: { 'Content-Type': 'application/json' },
      tags: { arm: 'flood' },
      responseCallback: http.expectedStatuses(400, 429),
    },
  );
  if (res.status !== 429) floodAdmitted.add(1);
  check(res, {
    'flood: 429 with Retry-After, or a wrong password': (r) =>
      (r.status === 429 && Number(r.headers['Retry-After']) >= 1)
      || (r.status === 400 && r.json('error.code') === 'sign_in_failed'),
  });
}

function p95(data, armName) {
  const metric = data.metrics[`http_req_duration{arm:${armName}}`];
  return metric ? Math.round(metric.values['p(95)'] * 10) / 10 : null;
}

export function handleSummary(data) {
  const admitted = data.metrics.flood_admitted ? data.metrics.flood_admitted.values.count : 0;
  const attempts = data.metrics['http_reqs{arm:flood}'] ? data.metrics['http_reqs{arm:flood}'].values.count : 0;
  const fmt = (v) => (v == null ? 'n/a' : `${v}ms`);
  return {
    stdout: [
      '',
      '  D-213 — one address floods password sign-in beside anonymous readers',
      `    flood: ${admitted} of ${attempts} admitted (allowed ${ALLOWED}: burst ${BURST} + ${PER_SECOND}/s over ${PHASE} s), the rest 429`,
      `    readers p95: ${fmt(p95(data, 'baseline'))} alone -> ${fmt(p95(data, 'reads'))} under the flood (gate: p99 < ${P99_MS} ms)`,
      '',
    ].join('\n'),
  };
}
