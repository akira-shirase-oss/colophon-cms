// Load scenario for L03 (spec §29, gates §28): cold cache — every single read lands on a
// cache key nobody has asked before, so every answer is a Postgres fill and nothing is a
// hit. setup() signs in the administrator, makes a project with a `posts` collection of
// 2,048 entries (imported in four 512-item chunks) and publishes them in 100-entry
// releases — the corpus is real, not a count. Each measured request is an anonymous
// `POST /content/v1/.../posts/query` — a `query` response is keyed by the query's own
// hash, and the document carries a per-request decoy term, so the key is unique
// by construction:
//
//   - `cold` (4 in 5): `headline in [<two real seeded titles>, <unique decoy>]` — answers
//     rows, so the fill is a Keep the cache stores and never serves again: `miss`.
//   - `bypass` (1 in 5): `headline in [<unique decoy>]` — an empty page, which the cache
//     deliberately never stores (a made-up name must not fill the cache with junk):
//     `bypass`.
//
// The spec's cold gate is p95 <= 300 ms, p99 <= 750 ms, with bounded pools and no
// unbounded database queue. The run measures that every cold read is exactly the one
// fill it asked for (zero `hit`, zero `coalesced`, zero errors), and scripts/load.sh
// records the database's own transaction count beside the request count in the result —
// `requests` vs `database_transactions_during_run` is the amplification evidence. The
// server's RSS and pool size are sampled into `resources` too.
//
// handleSummary() computes the verdict into `l03`; scripts/load.sh stores it and fails
// when `held` is false. scripts/load.sh gives this endpoint RATE 100 and P99_MS 750 —
// the spec's cold/cache-bypass gate.

import http from 'k6/http';
import { check, fail } from 'k6';
import { Counter } from 'k6/metrics';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '100', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '750');
const P95_MS = parseFloat(__ENV.P95_MS || '300');
const CORPUS = parseInt(__ENV.CORPUS || '2048', 10);

const fills = {
  hit: new Counter('l03_hit'),
  miss: new Counter('l03_miss'),
  coalesced: new Counter('l03_coalesced'),
  bypass: new Counter('l03_bypass'),
};
const coldReads = new Counter('l03_cold_reads');
const bypassReads = new Counter('l03_bypass_reads');
// A cold read that came back `bypass` with rows: still one Postgres fill for a unique key,
// but the cache was flapping (a >100 ms Valkey call drops the link for about a second —
// valkey.rs) so the answer wasn't stored. Counted and bounded: a cold run must still
// fill every read from origin, but a large uncached share would mean the cache isn't
// keeping up with fills, which is a different defect than a stale answer.
const uncachedKeeps = new Counter('l03_uncached_keeps');
const errors = new Counter('l03_errors');

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

const thresholds = {
  http_req_failed: ['rate==0'],
  checks: ['rate==1'],
  l03_errors: ['count==0'],
  l03_hit: ['count==0'],
  l03_coalesced: ['count==0'],
  http_req_duration: [`p(95)<${P95_MS}`, `p(99)<${P99_MS}`],
  'http_req_duration{endpoint:cold}': [`p(95)<${P95_MS}`, `p(99)<${P99_MS}`],
  'http_req_duration{endpoint:bypass}': ['p(99)<60000'],
};
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '600s', // 2,048 imported entries, then twenty-one 100-entry releases
};

const POSTS = {
  name: 'Posts',
  title_field: 'headline',
  fields: [
    { id: 'headline', label: 'Headline', type: 'text', required: true, index: true },
    { id: 'dek', label: 'Dek', type: 'text' },
    { id: 'section', label: 'Section', type: 'enum', values: ['News', 'Guides'], index: true },
  ],
};

function key() {
  return `k6-l03-${__VU}-${__ITER}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
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
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 cold cache' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const id = project.json('data.id');
  const env = `${BASE_URL}/api/v1/projects/${id}/environments/production`;

  const made = http.post(`${env}/content-types`, JSON.stringify({ slug: 'posts', definition: POSTS }), {
    headers: Object.assign({ 'Idempotency-Key': `k6-type-${slug}` }, write),
  });
  if (made.status !== 201) fail(`content type: HTTP ${made.status} ${made.body}`);

  // The corpus: CORPUS real drafts, imported 512 items a chunk (the endpoint's cap).
  const ids = [];
  for (let c = 0; c * 512 < CORPUS; c += 1) {
    let file = '';
    for (let i = 0; i < 512 && c * 512 + i < CORPUS; i += 1) {
      const n = c * 512 + i;
      file += JSON.stringify({ headline: `Post ${n}`, dek: `dek ${n}`, section: n % 2 === 0 ? 'News' : 'Guides' }) + '\n';
    }
    const imported = http.post(`${env}/collections/posts/import`, file, {
      headers: Object.assign({}, write, { 'Content-Type': 'application/jsonlines', 'Idempotency-Key': `k6-seed-${slug}-${c}` }),
    });
    if (imported.status !== 201) fail(`seed import ${c}: HTTP ${imported.status} ${imported.body}`);
    for (const outcome of imported.json('data.outcomes')) ids.push(outcome.entry);
  }
  // And published: the changeset cap is 100 entries, so the corpus lands in 100-entry
  // releases — real generations, not a fixture.
  let generation = 0;
  for (let c = 0; c * 100 < ids.length; c += 1) {
    const set = http.post(`${env}/changesets`, JSON.stringify({ title: `release ${c}`, entries: ids.slice(c * 100, c * 100 + 100) }), {
      headers: Object.assign({ 'Idempotency-Key': `k6-cs-${slug}-${c}` }, write),
    });
    if (set.status !== 201) fail(`changeset ${c}: HTTP ${set.status} ${set.body}`);
    const path = `${env}/changesets/${set.json('data.id')}`;
    const ifMatch = Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, write);
    http.post(`${path}/submit`, null, { headers: ifMatch });
    http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch });
    const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: generation }), {
      headers: Object.assign({ 'Idempotency-Key': `k6-pub-${slug}-${c}` }, ifMatch),
    });
    if (published.status !== 201) fail(`publish ${c}: HTTP ${published.status} ${published.body}`);
    generation = published.json('data.generation');
  }
  return { query: `${BASE_URL}/content/v1/${id}/production/posts/query`, generation, corpus: ids.length };
}

export default function (data) {
  // The decoy is unique per request — the query hash differs, so the cache key is new.
  const decoy = `decoy-${__VU}-${__ITER}-${Math.random().toString(36).slice(2, 10)}`;
  const params = (endpoint) => ({ headers: { 'Content-Type': 'application/json' }, redirects: 0, tags: { endpoint } });
  if (__ITER % 5 === 4) {
    // A fill that finds nothing: an empty page is served once and never cached — bypass.
    const res = http.post(data.query, JSON.stringify({ filters: [{ field: 'headline', op: 'in', value: [decoy] }], limit: 5 }), params('bypass'));
    bypassReads.add(1);
    const ok = check(res, {
      'bypass 200': (r) => r.status === 200,
      'bypass answers empty': (r) => r.json('data').length === 0,
      'bypass never cached': (r) => r.headers['Colophon-Cache'] === 'bypass',
    });
    if (!ok) { errors.add(1); console.log(`bypass read: ${res.status} cache=${res.headers['Colophon-Cache']} gen=${res.headers['Colophon-Generation']} ${String(res.body).slice(0, 200)}`); }
    const how = res.headers['Colophon-Cache'];
    if (how in fills) fills[how].add(1); else if (how) errors.add(1);
  } else {
    // A fill that finds two real posts beside its unique decoy — a Keep the cache would
    // store under a key no other request will ever ask (`miss`), unless a Valkey call
    // flapped and the answer is served once without storing (`bypass`, counted below).
    const a = __ITER % data.corpus;
    const b = (__ITER * 7 + 3) % data.corpus;
    const res = http.post(data.query, JSON.stringify({ filters: [{ field: 'headline', op: 'in', value: [`Post ${a}`, `Post ${b}`, decoy] }], limit: 5 }), params('cold'));
    coldReads.add(1);
    const ok = check(res, {
      'cold 200': (r) => r.status === 200,
      'cold answers the real posts': (r) => r.json('data').length === 2,
      'cold is a fresh fill': (r) => r.headers['Colophon-Cache'] === 'miss' || r.headers['Colophon-Cache'] === 'bypass',
      'cold stays at the published generation': (r) => r.headers['Colophon-Generation'] === String(data.generation),
    });
    if (!ok) { errors.add(1); console.log(`cold read [${a},${b}]: ${res.status} cache=${res.headers['Colophon-Cache']} gen=${res.headers['Colophon-Generation']} rows=${Array.isArray(res.json('data')) ? res.json('data').length : '?'} ${String(res.body).slice(0, 200)}`); }
    const how = res.headers['Colophon-Cache'];
    if (how === 'bypass') uncachedKeeps.add(1);
    if (how in fills) fills[how].add(1); else if (how) errors.add(1);
  }
}

/** Computes the L03 verdict: every cold read a fill of its own (miss or bypass, never a
 *  hit or a shared fill), each response right, the cold gate on latency. */
export function handleSummary(data) {
  const metric = (name) => (data.metrics[name] ? data.metrics[name].values : null);
  const count = (name) => (metric(name) ? metric(name).count || 0 : 0);
  const cold = metric('http_req_duration{endpoint:cold}') || {};
  const all = metric('http_req_duration') || {};
  const coldReads_ = count('l03_cold_reads');
  const bypassReads_ = count('l03_bypass_reads');
  const uncached = count('l03_uncached_keeps');
  const gates = {
    every_cold_read_a_fill: count('l03_miss') + uncached === coldReads_ && coldReads_ > 0,
    every_bypass_read_uncached: count('l03_bypass') === bypassReads_ + uncached && bypassReads_ > 0,
    nothing_served_from_cache: count('l03_hit') === 0 && count('l03_coalesced') === 0,
    // The Valkey flap bound: one >100 ms call drops the link for about a second. Under a
    // pure unique-fill flood a small share of Keeps is served once without being stored —
    // honest degrade (origin still filled them) — but a run where more than a few percent
    // went uncached is the cache failing to keep up, which this gate names.
    cache_kept_up: uncached * 50 <= coldReads_,
    no_errors: count('l03_errors') === 0,
    cold_p95: (cold['p(95)'] || Infinity) < P95_MS,
    cold_p99: (cold['p(99)'] || Infinity) < P99_MS,
  };
  data.l03 = {
    spec: 'L03: cold cache / 100% unique keys — cold-origin gate (p95 <=300 ms, p99 <=750 ms), bounded pools, no unbounded database queue',
    gate: `every request a unique key: all cold reads origin fills ('miss' or a counted 'bypass' flap, <=2%), all empty reads 'bypass', zero hit/coalesced/errors; cold arm p95 <${P95_MS} ms, p99 <${P99_MS} ms; db transactions per request sit beside this in the result`,
    corpus_entries: CORPUS,
    reads: { cold_fills: coldReads_, uncached_fills: bypassReads_, uncached_keeps: uncached },
    served: { hit: count('l03_hit'), miss: count('l03_miss'), coalesced: count('l03_coalesced'), bypass: count('l03_bypass') },
    errors: count('l03_errors'),
    latency_ms: {
      cold: { p50: cold.med, p95: cold['p(95)'], p99: cold['p(99)'], max: cold.max },
      all: { p95: all['p(95)'], p99: all['p(99)'] },
    },
    gates,
    held: Object.values(gates).every(Boolean),
  };
  return {
    stdout: [
      '',
      '  L03 — cold cache, 100% unique keys',
      `    ${coldReads_} cold fills (${count('l03_miss')} miss, ${uncached} uncached keeps), ${bypassReads_} empty fills (bypass), ${count('l03_hit')} hits, ${count('l03_coalesced')} coalesced, ${count('l03_errors')} errors`,
      `    cold latency p95 ${(cold['p(95)'] || 0).toFixed(1)} ms (gate ${P95_MS}), p99 ${(cold['p(99)'] || 0).toFixed(1)} ms (gate ${P99_MS})`,
      `    verdict: ${data.l03.held ? 'HELD' : 'BROKE'}`,
      '',
    ].join('\n'),
  };
}
