// Load scenario for L02 (spec §29, gates §28): the warm read baseline — anonymous
// /content/v1 traffic at the spec's warm-origin shape, 300 requests a second for a
// thirty-minute DURATION in the evidence run. setup() signs in the administrator, makes a
// project with 20 articles and publishes four releases, exactly like delivery.js: 50 in
// 100 reads list the latest release, 20 list a pinned one, 10 read a live entry, 10 read
// an article pinned at generation 1 (which must serve the revision published then) and 10
// read the one taken down (410, never cached). Every read runs as colophon_delivery.
//
// What the spec asks this run to prove: p95 at most 150 ms and p99 at most 350 ms with an
// unexpected failure share under 0.1% — the thresholds enforce that — plus the warm
// numbers worth keeping: the cache-hit share (cache_hits, the run's own rate metric —
// the 23 distinct answers must mostly come from Valkey), a p99 per five-minute window
// (`window` tag, so drift inside the run is visible), and the server's RSS, CPU and pool
// use, which scripts/load.sh samples beside it into the result's `resources` (and the
// database's own transaction count into `database_transactions_during_run`).
//
// handleSummary() computes the verdict into `l02`: per-window p99s, the hit share, the
// failure share, and whether every gate held for the seconds it ran. A short DURATION
// (CI's smoke) produces fewer windows — `held` speaks for what ran, `run_seconds` says
// how much. scripts/load.sh stores it and fails when `held` is false.
// scripts/load.sh gives this endpoint RATE 300 and P99_MS 350 — the spec's warm gate.

import http from 'k6/http';
import { check, fail } from 'k6';
import exec from 'k6/execution';
import { Counter, Rate } from 'k6/metrics';

const cacheHits = new Rate('cache_hits');
const served = {
  hit: new Counter('l02_hit'),
  miss: new Counter('l02_miss'),
  coalesced: new Counter('l02_coalesced'),
  bypass: new Counter('l02_bypass'),
};

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '300', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '350');
const P95_MS = parseFloat(__ENV.P95_MS || '150');
// The window a request's latency is filed under: five minutes each in the evidence run.
const WINDOW_SECONDS = parseInt(__ENV.WINDOW_SECONDS || '300', 10);

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

// The spec's warm-origin gate (§28): p95 150, p99 350, unexpected failures under a
// thousandth. Per-window thresholds make each five minutes answer for itself; a short
// run simply produces fewer windows.
const thresholds = {
  http_req_failed: ['rate<0.001'],
  checks: ['rate==1'],
  cache_hits: ['rate>0.95'],
  http_req_duration: [`p(95)<${P95_MS}`, `p(99)<${P99_MS}`],
};
for (let w = 0; w < 8; w += 1) {
  thresholds[`http_req_duration{window:w${w}}`] = [`p(99)<${P99_MS}`];
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
  const key = () => `k6-warm-${run}-${(n += 1)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-${run}`, name: 'k6 warm baseline' }), { headers });
  if (project.status !== 201) fail(`project: HTTP ${project.status}`);
  const projectId = project.json('data.id');
  const base = `${BASE_URL}/api/v1/projects/${projectId}/environments/production`;
  const entries = [];
  for (let i = 0; i < 20; i += 1) {
    const created = http.post(`${base}/collections/articles/entries`, JSON.stringify(body(`Article ${i}`, 'first')), { headers: Object.assign({ 'Idempotency-Key': key() }, headers) });
    if (created.status !== 201) fail(`entry: HTTP ${created.status}`);
    entries.push(created.json('data.id'));
  }
  const release = (ids, generation, unpublish = []) => {
    const changes = { title: `release ${generation + 1}`, entries: ids, unpublish };
    const set = http.post(`${base}/changesets`, JSON.stringify(changes), { headers: Object.assign({ 'Idempotency-Key': key() }, headers) });
    const path = `${base}/changesets/${set.json('data.id')}`;
    const ifMatch = Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, headers);
    http.post(`${path}/submit`, null, { headers: ifMatch });
    http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch });
    const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: generation }), { headers: Object.assign({ 'Idempotency-Key': key() }, ifMatch) });
    if (published.status !== 201) fail(`publish: HTTP ${published.status} ${published.body}`);
  };
  release(entries, 0);
  const edit = (ids, text) => {
    for (const id of ids) {
      const path = `${base}/collections/articles/entries/${id}`;
      const current = http.get(path, { headers });
      const saved = http.put(path, JSON.stringify(body(current.json('data.content.title'), text)), { headers: Object.assign({ 'If-Match': current.headers.Etag, 'Idempotency-Key': key() }, headers) });
      if (saved.status !== 200) fail(`save: HTTP ${saved.status}`);
    }
  };
  edit(entries.slice(0, 5), 'second');
  release(entries.slice(0, 5), 1);
  edit(entries.slice(5, 10), 'third');
  release(entries.slice(5, 10), 2);
  release([], 3, [entries[19]]);
  return { content: `${BASE_URL}/content/v1/${projectId}/production/articles`, live: entries.slice(0, 19), edited: entries.slice(0, 5), gone: entries[19] };
}

// The five-minute bucket the request lands in, so the exported summary holds a p99 per
// window instead of one blind average for half an hour.
function windowTag() {
  const now = exec.instance.currentTestRunDuration;
  return `w${Math.floor((typeof now === 'number' ? now : 0) / 1000 / WINDOW_SECONDS)}`;
}

export default function (data) {
  const slot = __ITER % 10;
  const params = (endpoint) => ({ redirects: 0, tags: { endpoint, window: windowTag() } });
  let res;
  if (slot < 5) {
    res = http.get(data.content, params('latest'));
    check(res, { 'latest: 19 at generation 4': (r) => r.status === 200 && r.headers['Colophon-Generation'] === '4' && r.json('data').length === 19 });
  } else if (slot < 7) {
    const generation = 1 + (__ITER % 3);
    res = http.get(`${data.content}?generation=${generation}`, params('pinned'));
    check(res, { 'pinned: the generation asked for, without the one taken down': (r) => r.status === 200 && r.headers['Colophon-Generation'] === String(generation) && r.json('data').length === 19 });
  } else if (slot < 8) {
    res = http.get(`${data.content}/${data.live[__ITER % 19]}`, params('entry'));
    check(res, { 'entry 200': (r) => r.status === 200 && r.json('data.published_in') >= 1 });
  } else if (slot < 9) {
    res = http.get(`${data.content}/${data.edited[__ITER % 5]}?generation=1`, params('pinned_entry'));
    check(res, {
      'pinned entry: generation 1 serves the revision published then': (r) =>
        r.status === 200 && r.headers['Colophon-Generation'] === '1' && r.json('data.content.body.doc.content')[0].content[0].text === 'first',
    });
  } else {
    const pinned = __ITER % 20 < 10 ? '' : '?generation=1';
    res = http.get(`${data.content}/${data.gone}${pinned}`, Object.assign(params('gone'), { responseCallback: http.expectedStatuses(410) }));
    check(res, { 'taken down: 410 at every generation': (r) => r.status === 410 && r.json('error.code') === 'gone' });
    return;
  }
  const how = res.headers['Colophon-Cache'];
  cacheHits.add(how === 'hit' || how === 'coalesced');
  if (how in served) served[how].add(1);
}

/** Computes the L02 verdict: the spec's warm gate against every window the run produced,
 *  the hit share, the failure share — `held` when all of it stayed inside §28. */
export function handleSummary(data) {
  const metric = (name) => (data.metrics[name] ? data.metrics[name].values : null);
  const duration = metric('http_req_duration') || {};
  const failed = metric('http_req_failed') || {};
  const hits = metric('cache_hits') || {};
  const windows = [];
  for (const name of Object.keys(data.metrics)) {
    const match = /^http_req_duration\{window:(w\d+)\}$/.exec(name);
    if (!match) continue;
    const m = data.metrics[name].values;
    // Thresholded-but-empty windows export with all-zero values — a real window's max is
    // never 0 ms — so they don't count as measured.
    if (!(m.max > 0)) continue;
    windows.push({ window: match[1], p95_ms: m['p(95)'], p99_ms: m['p(99)'], max_ms: m.max });
  }
  windows.sort((a, b) => a.window.localeCompare(b.window));
  const requests = metric('http_reqs') || {};
  const gates = {
    p95_within_gate: duration['p(95)'] < P95_MS,
    p99_within_gate: duration['p(99)'] < P99_MS,
    failures_under_0_1_pct: (failed.rate !== undefined ? failed.rate : 1) < 0.001,
    hits_at_least_95_pct: (hits.rate !== undefined ? hits.rate : 0) > 0.95,
    every_window_within_p99_gate: windows.every((w) => w.p99_ms < P99_MS),
    windows_measured: windows.length > 0,
  };
  data.l02 = {
    spec: 'L02: warm read baseline — warm-origin gates: p95 <=150 ms, p99 <=350 ms, unexpected failures <0.1%; record hits, pools, CPU, RSS',
    gate: `overall p95 <${P95_MS} ms and p99 <${P99_MS} ms, failure share <0.1%, cache-hit share >95%, and every five-minute window's p99 inside the same gate`,
    run_seconds: durationSeconds(DURATION),
    windows_seconds: WINDOW_SECONDS,
    requests: requests.count || 0,
    rps: requests.rate || 0,
    latency_ms: { p50: duration.med, p95: duration['p(95)'], p99: duration['p(99)'], max: duration.max },
    windows_p99: windows,
    cache: {
      hit_share: hits.rate !== undefined ? hits.rate : null,
      served: {
        hit: metric('l02_hit')?.count || 0,
        miss: metric('l02_miss')?.count || 0,
        coalesced: metric('l02_coalesced')?.count || 0,
        bypass: metric('l02_bypass')?.count || 0,
      },
    },
    failures: failed.passes || 0,
    gates,
    held: Object.values(gates).every(Boolean),
  };
  return {
    stdout: [
      '',
      '  L02 — warm read baseline',
      `    ${requests.count || 0} requests at ${(requests.rate || 0).toFixed(0)}/s: p95 ${(duration['p(95)'] || 0).toFixed(1)} ms (gate ${P95_MS}), p99 ${(duration['p(99)'] || 0).toFixed(1)} ms (gate ${P99_MS}), ${failed.passes || 0} failed`,
      `    cache hits ${((hits.rate || 0) * 100).toFixed(1)}% over ${windows.length} window(s)`,
      `    verdict: ${data.l02.held ? 'HELD' : 'BROKE'}`,
      '',
    ].join('\n'),
  };
}
