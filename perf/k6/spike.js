// Load scenario for L06 (spec §29): the spike — anonymous delivery reads at the baseline
// RATE for DURATION, then SPIKE_SECONDS at five times that, then DURATION at the baseline
// again. scripts/load.sh gives this endpoint RATE 500 and P99_MS 250 for the calm phases,
// and starts its server with a real delivery limiter (COLOPHON_RATE_LIMIT_DELIVERY
// 1000/2000 — the production default the other runs raise to a million): a 500/s baseline
// sits under the ceiling, a 2,500/s spike is refused by the cell — which is exactly what
// L06 wants measured: controlled 429s (or pool-bound 503s), not silent queueing.
//
// Every spike answer is classified, not just failed-or-not: 200s are served reads,
// 429s and 503s are the refusals the spec names (each must be a shaped envelope —
// `rate_limited`/`unavailable` — never a bare 500, never a hang past the server's own
// deadline), and anything else is `l06_other`, which fails the run. Recovery is measured,
// not assumed: each post-spike read is tagged into 30-second windows since the spike
// ended, and the verdict finds the first window back inside the baseline's own p99 —
// that must happen within RECOVER_SECONDS (the spec's two minutes) and hold for every
// window after it.
//
// The arms are time-boxed, not iteration-counted: TOTAL and VUS are unused. `steady` and
// `throughput` run the same plan. handleSummary() computes the verdict into `l06`;
// scripts/load.sh stores it and fails when `held` is false — and samples server RSS, CPU
// and Postgres connections into `resources` beside it.

import http from 'k6/http';
import { check, fail } from 'k6';
import exec from 'k6/execution';
import { Counter, Trend } from 'k6/metrics';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const RATE = parseInt(__ENV.RATE || '500', 10);
const MULTIPLE = parseFloat(__ENV.MULTIPLE || '5');
const DURATION = __ENV.DURATION || '60s';
const SPIKE_SECONDS = parseInt(__ENV.SPIKE_SECONDS || '120', 10);
const P99_MS = parseFloat(__ENV.P99_MS || '250');
// The bound a queued request may still come back inside — past it is an unbounded queue,
// whatever status it carried.
const SPIKE_P99_MS = parseFloat(__ENV.SPIKE_P99_MS || '15000');
// The spec's recovery bound: back to baseline within two minutes of the load ending.
const RECOVER_SECONDS = parseInt(__ENV.RECOVER_SECONDS || '120', 10);
const EPSILON_MS = 2; // the same scheduler-jitter epsilon the L08 gate uses

const servedReads = new Counter('l06_served');
const refused429 = new Counter('l06_refused_429');
const refused503 = new Counter('l06_refused_503');
const other = new Counter('l06_other');
const errors = new Counter('l06_errors');
const spikeLatency = new Trend('l06_spike_latency', true);
// Post-spike latency, one root Trend per 30-second window since the spike ended. Root
// metrics, not tagged submetrics: k6 only exports a tagged submetric in the summary when
// it carries a threshold, and the window names can't be declared ahead. Six windows
// cover the spec's two-minute recovery bound plus margin.
const RECOVERY_WINDOWS = 6;
const recoveryTrends = [];
for (let w = 0; w < RECOVERY_WINDOWS; w += 1) recoveryTrends.push(new Trend(`l06_recovery_w${w}`, true));

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}
const PHASE = durationSeconds(DURATION);
const GAP = 2; // a breath between phases, so nothing in flight bleeds across

if (!(SCENARIO === 'throughput' || SCENARIO === 'steady')) {
  throw new Error('SCENARIO must be throughput or steady');
}

const thresholds = {
  checks: ['rate==1'],
  http_req_failed: ['rate==0'],
  l06_errors: ['count==0'],
  l06_other: ['count==0'],
  l06_refused_429: ['count>0'], // under the ceiling the spike MUST be refused — none means it wasn't
  'http_req_duration{arm:before}': [`p(99)<${P99_MS}`],
  'http_req_duration{arm:after}': [`p(99)<${P99_MS}`],
  'http_req_duration{arm:spike}': [`p(99)<${SPIKE_P99_MS}`],
  'http_reqs{arm:before}': ['count>0'],
  'http_reqs{arm:spike}': ['count>0'],
  'http_reqs{arm:after}': ['count>0'],
};

export const options = {
  scenarios: {
    before: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: 48, maxVUs: 96, exec: 'before' },
    spike: { executor: 'constant-arrival-rate', rate: Math.round(RATE * MULTIPLE), timeUnit: '1s', duration: `${SPIKE_SECONDS}s`, startTime: `${PHASE + GAP}s`, preAllocatedVUs: 128, maxVUs: 256, exec: 'spike' },
    after: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, startTime: `${PHASE + GAP + SPIKE_SECONDS + GAP}s`, preAllocatedVUs: 48, maxVUs: 96, exec: 'after' },
  },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '300s',
};

const article = (title, text) => JSON.stringify({
  title,
  body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] } },
});

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
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 spike' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const id = project.json('data.id');
  const env = `${BASE_URL}/api/v1/projects/${id}/environments/production`;

  // One hundred articles, one release: warm reads for the whole run.
  let file = '';
  const ids = [];
  for (let i = 0; i < 100; i += 1) {
    file += article(`Article ${i}`, 'first') + '\n';
  }
  const imported = http.post(`${env}/collections/articles/import`, file, {
    headers: Object.assign({}, write, { 'Content-Type': 'application/jsonlines', 'Idempotency-Key': `k6-seed-${slug}` }),
  });
  if (imported.status !== 201) fail(`seed import: HTTP ${imported.status} ${imported.body}`);
  for (const outcome of imported.json('data.outcomes')) ids.push(outcome.entry);
  const set = http.post(`${env}/changesets`, JSON.stringify({ title: 'release 1', entries: ids }), {
    headers: Object.assign({ 'Idempotency-Key': `k6-cs-${slug}` }, write),
  });
  if (set.status !== 201) fail(`changeset: HTTP ${set.status} ${set.body}`);
  const path = `${env}/changesets/${set.json('data.id')}`;
  const ifMatch = Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, write);
  http.post(`${path}/submit`, null, { headers: ifMatch });
  http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch });
  const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: 0 }), {
    headers: Object.assign({ 'Idempotency-Key': `k6-pub-${slug}` }, ifMatch),
  });
  if (published.status !== 201) fail(`publish: HTTP ${published.status} ${published.body}`);
  return { delivery: `${BASE_URL}/content/v1/${id}/production/articles`, ids };
}

function read(data, arm) {
  // 80% the latest list, 20% a random entry — the same warm shape L02 measures.
  const url = __ITER % 5 === 4
    ? `${data.delivery}/${data.ids[__ITER % data.ids.length]}`
    : `${data.delivery}?limit=50`;
  const res = http.get(url, {
    redirects: 0,
    // The spike's refusals are evidence, not harness failures — 200, 429 and 503 are all
    // answers this run classifies.
    responseCallback: http.expectedStatuses(200, 429, 503),
    tags: { arm, endpoint: 'read', name: 'spike-read' },
  });
  if (res.status === 200) {
    servedReads.add(1);
    check(res, { 'read 200': (r) => r.status === 200 });
  } else if (res.status === 429) {
    refused429.add(1);
    if (!check(res, {
      '429 is a shaped rate_limited refusal with Retry-After': (r) =>
        r.json('error.details.reason') === 'rate_limited' && r.headers['Retry-After'] !== undefined,
    })) errors.add(1);
  } else if (res.status === 503) {
    refused503.add(1);
    if (!check(res, {
      '503 is a shaped refusal': (r) => typeof r.json('error.code') === 'string',
    })) errors.add(1);
  } else {
    other.add(1); // a 500, a 404, a hang — anything but the answers the spec names
  }
  return res;
}

export function before(data) { read(data, 'before'); }

export function spike(data) {
  const res = read(data, 'spike');
  spikeLatency.add(res.timings.duration);
}

// The recovery window this post-spike read falls in: 30 seconds each, counted from when
// the spike arm ended. handleSummary() walks the windows' own p99 series.
export function after(data) {
  const res = read(data, 'after');
  const now = exec.instance.currentTestRunDuration;
  if (typeof now === 'number') {
    const since = now - (PHASE + GAP + SPIKE_SECONDS + GAP) * 1000;
    const index = Math.min(RECOVERY_WINDOWS - 1, Math.max(0, Math.floor(since / 30000)));
    recoveryTrends[index].add(res.timings.duration);
  }
}

/** Computes the L06 verdict: the spike's answers were all served reads or shaped
 *  refusals, its p99 stayed bounded, and the first window back inside the baseline's own
 *  p99 came within two minutes and held. */
export function handleSummary(data) {
  const metric = (name) => (data.metrics[name] ? data.metrics[name].values : null);
  const count = (name) => (metric(name) ? metric(name).count || 0 : 0);
  const before = metric('http_req_duration{arm:before}') || {};
  const after = metric('http_req_duration{arm:after}') || {};
  const spike = metric('http_req_duration{arm:spike}') || {};
  const windows = [];
  for (let index = 0; index < RECOVERY_WINDOWS; index += 1) {
    const m = metric(`l06_recovery_w${index}`);
    if (!m) continue; // a Trend with no samples isn't exported at all
    windows.push({ window: `w${index}`, index, p95_ms: m['p(95)'], p99_ms: m['p(99)'] });
  }
  // "Back to baseline": a recovery window inside the baseline arm's own p99 (times a
  // bound for spill-over, plus the scheduler-jitter epsilon), then every later window too.
  const baselineP99 = before['p(99)'] || Infinity;
  const bound = baselineP99 * 1.5 + EPSILON_MS;
  let recoveredAt = null; let holdsAfter = true;
  for (const w of windows) {
    if (recoveredAt === null && w.p99_ms <= bound) recoveredAt = w.index;
    if (recoveredAt !== null && w.p99_ms > bound) holdsAfter = false;
  }
  const recoveredSeconds = recoveredAt === null ? null : recoveredAt * 30;
  const gates = {
    refusals_controlled: count('l06_refused_429') > 0 && count('l06_other') === 0,
    no_errors: count('l06_errors') === 0,
    spike_p99_bounded: (spike['p(99)'] || Infinity) < SPIKE_P99_MS,
    recovery_within_2_minutes: recoveredAt !== null && recoveredSeconds < RECOVER_SECONDS && holdsAfter,
    baseline_held: (before['p(99)'] || Infinity) < P99_MS && (after['p(99)'] || Infinity) < P99_MS,
  };
  data.l06 = {
    spec: 'L06: spike from 1x to 5x load for 2 min — controlled 429/503, bounded queues; back to baseline within 2 min after load ends',
    gate: `every spike answer a served read or a shaped 429/503 (some 429s must occur — the limiter set real); spike p99 <${SPIKE_P99_MS} ms; first recovery window inside 1.5x baseline p99 +${EPSILON_MS} ms by second ${RECOVER_SECONDS}, holding after`,
    baseline_per_second: RATE,
    spike_per_second: Math.round(RATE * MULTIPLE),
    phases_seconds: { baseline: PHASE, gap: GAP, spike: SPIKE_SECONDS, recovery: PHASE },
    answers: {
      served: count('l06_served'),
      refused_429: count('l06_refused_429'),
      refused_503: count('l06_refused_503'),
      other: count('l06_other'),
      errors: count('l06_errors'),
    },
    latency_ms: {
      before_p99: before['p(99)'],
      spike_p99: spike['p(99)'],
      after_p99: after['p(99)'],
      recovery_bound: bound,
    },
    recovery_windows: windows,
    recovered_after_seconds: recoveredSeconds,
    gates,
    held: Object.values(gates).every(Boolean),
  };
  return {
    stdout: [
      '',
      '  L06 — 1x to 5x spike and recovery',
      `    spike at ${Math.round(RATE * MULTIPLE)}/s: ${count('l06_served')} served, ${count('l06_refused_429')} shaped 429s, ${count('l06_refused_503')} shaped 503s, ${count('l06_other')} other, p99 ${(spike['p(99)'] || 0).toFixed(1)} ms (bound ${SPIKE_P99_MS})`,
      `    baseline p99 ${(baselineP99 === Infinity ? 'n/a' : baselineP99.toFixed(1))} ms -> recovery bound ${bound.toFixed(1)} ms; first window inside it at ${recoveredAt === null ? 'never' : `w${recoveredAt} (~${recoveredSeconds}s)`} (gate <${RECOVER_SECONDS}s, must hold after)`,
      `    verdict: ${data.l06.held ? 'HELD' : 'BROKE'}`,
      '',
    ].join('\n'),
  };
}
