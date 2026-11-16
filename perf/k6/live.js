// Load scenario for live lists (E3, D-108): SUBSCRIBERS virtual users each hold a
// `GET {P}/events` stream open to its clean end — the load-test server runs with a short
// COLOPHON_EVENTS_LIFETIME_MS (load.sh sets 15 s; the default 30 min is longer than the
// client's 90 s timeout, so without it every hold would abort) — while WRITERS virtual users
// save entries steadily, every save a real commit that emits the hint the streams carry.
//
// An iteration is one stream held from open to the server's clean end (or one entry save,
// on the writers). A stream that answers non-2xx, is aborted before its end, or carries no
// `hello` is a failure — and under sustained writes one that never forwards a `change` is
// too. The 8 subscriber VUs sit exactly at COLOPHON_EVENTS_PER_CREDENTIAL's default, the cap
// one session may hold; SUBS raises it only with a matching server setting. Same SCENARIO,
// TOTAL, VUS, RATE, DURATION and P99_MS settings as entries.js; scripts/load.sh gives this
// endpoint RATE 500 and P99_MS 250.

import http from 'k6/http';
import { check, fail } from 'k6';
import exec from 'k6/execution';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const SUBS = parseInt(__ENV.SUBS || '8', 10);
const RATE = parseInt(__ENV.RATE || '500', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '250');

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

// Writes for throughput; the same cadence the entries scenario is provisioned for.
const writers = {
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '40m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
};
if (!(SCENARIO in writers)) throw new Error(`SCENARIO must be one of ${Object.keys(writers).join(', ')}`);
// Holds are bounded by the server's stream lifetime: enough iterations to cover the run —
// each is seconds long, never the 40-minute writer bound.
const HOLDS =
  SCENARIO === 'steady'
    ? SUBS * Math.max(1, Math.ceil(durationSeconds(DURATION) / 20))
    : Math.max(SUBS, Math.ceil(TOTAL / 5000));
const subscribers = {
  executor: 'shared-iterations',
  vus: SUBS,
  iterations: HOLDS,
  maxDuration: '40m',
  exec: 'subscribe',
};

const thresholds = {
  checks: ['rate==1'],
  // Zero stream errors: a non-2xx or an aborted hold is a failure.
  'http_req_failed{endpoint:stream}': ['rate==0'],
  'http_req_failed{endpoint:write}': ['rate==0'],
  'http_req_duration{endpoint:write}': [`p(99)<${P99_MS}`],
};
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}

export const options = {
  scenarios: { writers: writers[SCENARIO], subscribers },
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
  const signedIn = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }), { headers: { 'Content-Type': 'application/json' } });
  if (signedIn.status !== 204) fail(`sign-in failed: HTTP ${signedIn.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const session = { cookie: `colophon_session=${jar.colophon_session[0]}`, csrf: jar.colophon_csrf[0] };
  const write = { Cookie: session.cookie, 'X-CSRF-Token': session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };

  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 live' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const env = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;

  // One article per VU id that can exist — ids are global across scenarios (`__VU` reaches
  // VUS+SUBS), so subscribers' ids count too; writers then never share an article or an etag.
  const count = (SCENARIO === 'steady' ? VUS * 2 : VUS) + SUBS;
  const entries = [];
  for (let i = 0; i < count; i += 1) {
    const created = http.post(`${env}/collections/articles/entries`, article(`Article ${i}`, 'Start'), {
      headers: Object.assign({ 'Idempotency-Key': `k6-setup-${slug}-${i}` }, write),
    });
    if (created.status !== 201) fail(`article ${i}: HTTP ${created.status} ${created.body}`);
    entries.push({ id: created.json('data.id'), etag: created.headers['Etag'] });
  }
  return { session, base: `${env}/collections/articles/entries`, events: `${env}/events`, write, entries };
}

/** One held stream: it must end on the server's say — a 200 with `hello`, and under this
 *  run's writes at least one forwarded `change` — never on a client abort. */
export function subscribe(data) {
  const res = http.get(data.events, {
    headers: { Cookie: data.session.cookie },
    timeout: '90s',
    redirects: 0,
    tags: { endpoint: 'stream', name: 'events' },
  });
  check(res, {
    'stream 200': (r) => r.status === 200,
    'stream says hello': (r) => r.body.indexOf('event: hello') !== -1,
    'stream forwarded a change': (r) => r.body.indexOf('event: change') !== -1,
  });
}

// Per VU: the ETag of the last revision this VU saved (each VU has its own article), and a
// bound on how many failures it prints — enough to name a status without flooding the run.
let etag = null;
let logged = 0;

/** One entry save — a commit, so the streams have something to carry. */
export default function (data) {
  // idInTest is global across the run's scenarios (writers reach past VUS because the
  // subscriber VUs share its numbering); entries has one per possible id, so no two writer
  // VUs ever save the same article or race each other's etag.
  const mine = data.entries[(exec.vu.idInTest - 1) % data.entries.length];
  if (etag === null) etag = mine.etag;
  const headers = {
    Cookie: data.session.cookie,
    'X-CSRF-Token': data.session.csrf,
    Origin: PUBLIC_URL,
    'Content-Type': 'application/json',
    'If-Match': etag,
    'Idempotency-Key': key(),
  };
  const res = http.put(`${data.base}/${mine.id}`, article(`Article by VU ${__VU}`, `Iteration ${__ITER}`), {
    headers,
    redirects: 0,
    tags: { endpoint: 'write', name: 'entries/{entry}' },
  });
  if (res.status !== 200 && logged < 3) {
    logged += 1;
    console.log(`VU ${__VU}: save HTTP ${res.status}: ${res.body.slice(0, 300)}`);
  }
  const saved = check(res, {
    'save 200': (r) => r.status === 200,
    'save gives a new ETag': (r) => typeof r.headers['Etag'] === 'string' && r.headers['Etag'] !== etag,
  });
  if (saved) etag = res.headers['Etag'];
}
