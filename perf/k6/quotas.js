// Load scenario for the request quota (D-118): a token's `requests_per_minute` checked in the
// admission limiter where its scoped transaction opens. setup() signs the owner in through
// Keycloak, makes a project and two read-only tokens: `bounded`, ceiling 100,000 a minute and
// provider `tokens`/`cost` ceilings (D-122) the scenario only stores —
// the arm asks for a fifth of the run's requests, under the ceiling on any machine this runs
// on, so its requests prove a token inside its budget keeps its shape and its latency — and
// `throttled`, ceiling 2 a minute with the whole burst spent in setup(), so every measured
// request it makes answers 429 with `Retry-After` and `rate_limited` (the ~one a half minute
// the window earns back is allowed, and the `quota_refusals` gate tolerates it). The measured
// mix: two iterations in ten GET the entries list with `bounded`, eight in ten with
// `throttled`. scripts/load.sh gives this endpoint RATE 1000 and P99_MS 250.

import http from 'k6/http';
import { check, fail } from 'k6';
import { Rate } from 'k6/metrics';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '1000', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '250');

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const scenarios = {
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '20m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);

// The throttled arm's share that is refused: over a two-a-minute window at load rates almost
// every request is; the window's own earnings (~2 a minute) admit the rest.
const refusals = new Rate('quota_refusals');

const thresholds = {
  http_req_failed: ['rate==0'],
  checks: ['rate==1'],
  quota_refusals: ['rate>0.9'],
  http_req_duration: [`p(99)<${P99_MS}`],
};
// Reported per kind of request, so a tail can be traced to its cause; only the overall p99 fails a run.
for (const endpoint of ['under-quota', 'over-quota']) {
  thresholds[`http_req_duration{endpoint:${endpoint}}`] = ['p(99)<60000'];
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

/** Signs `username` in with a cookie jar of its own; the headers a browser tab would send. */
function signIn(username) {
  const jar = new http.CookieJar();
  const page = http.get(`${PUBLIC_URL}/auth/login?return_to=/me`, { jar });
  const action = parseHTML(page.body).find('#kc-form-login').attr('action');
  if (!action) fail(`no Keycloak login form for ${username}`);
  const me = http.post(action.replace(/&amp;/g, '&'), { username, password: 'colophon-dev' }, { jar });
  if (me.status !== 200) fail(`sign-in as ${username} failed: HTTP ${me.status}`);
  const cookies = jar.cookiesForURL(PUBLIC_URL);
  return {
    Cookie: `colophon_session=${cookies.colophon_session[0]}`, 'X-CSRF-Token': cookies.colophon_csrf[0], Origin: PUBLIC_URL,
    'Content-Type': 'application/json',
  };
}

function token(owner, base, body) {
  const made = http.post(`${base}/tokens`, JSON.stringify(body), { headers: owner });
  if (made.status !== 201) fail(`token ${body.name}: HTTP ${made.status} ${made.body}`);
  return made.json('data.secret');
}

export function setup() {
  const owner = signIn('dev-owner');
  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 quotas' }), { headers: owner });
  if (project.status !== 201) fail(`project: HTTP ${project.status}`);
  const base = `${BASE_URL}/api/v1/projects/${project.json('data.id')}`;
  const entries = `${base}/environments/production/collections/articles/entries`;
  // Provider ceilings too (D-122): the run writes and reads the wider limits row, and the
  // admission check proves ceilings it doesn't consume cost nothing.
  const bounded = token(owner, base, {
    name: 'Under quota', kind: 'read-only', expires_in_days: 1,
    limits: { requests_per_minute: 100000, tokens_per_day: 1000000, cost_per_day_micros: 50000000 },
  });
  const throttled = token(owner, base, {
    name: 'Over quota', kind: 'read-only', expires_in_days: 1,
    limits: { requests_per_minute: 2 },
  });
  // Spend the small window's whole burst before measurement starts: what the arm measures
  // afterwards is the refusal path, not the two free requests.
  for (let n = 0; n < 2; n += 1) {
    const warm = http.get(entries, { headers: { Authorization: `Bearer ${throttled}` } });
    if (warm.status !== 200) fail(`throttled warm-up ${n}: HTTP ${warm.status} ${warm.body}`);
  }
  return { owner, entries, bounded, throttled };
}

export default function (data) {
  if (__ITER % 10 < 2) {
    // Under its ceiling the quota costs the check and nothing more: the list it asked for.
    const res = http.get(data.entries, {
      headers: { Authorization: `Bearer ${data.bounded}` },
      redirects: 0,
      tags: { endpoint: 'under-quota' },
    });
    check(res, { 'under quota 200': (r) => r.status === 200 });
  } else {
    const res = http.get(data.entries, {
      headers: { Authorization: `Bearer ${data.throttled}` },
      redirects: 0,
      tags: { endpoint: 'over-quota' },
      responseCallback: http.expectedStatuses(200, 429),
    });
    refusals.add(res.status === 429);
    check(res, {
      'over quota: refused, or the window’s rare admit': (r) => r.status === 429 || r.status === 200,
      'a refusal says rate_limited with Retry-After': (r) =>
        r.status !== 429 || (r.headers['Retry-After'] !== undefined && r.json('error.details.reason') === 'rate_limited'),
    });
  }
}
