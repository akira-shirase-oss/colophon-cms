// Load scenario for L04 (spec §29): the hot-key stampede — about a thousand callers landing
// on one cache key at its expiry must regenerate it once, coalesce everyone who arrived
// during the fill, and never let one project's burst touch another's answer.
//
// How the expiry is made to land inside the run's seconds: scripts/load.sh starts this
// endpoint's server with COLOPHON_CACHE_TTL_MS=4000 and COLOPHON_CACHE_JITTER_MS=0 (D-128),
// so every stored answer dies exactly four seconds after it was written. The bursts are
// aligned to four-second ROUNDs on the run's shared clock — a burst's own fills die just
// before the next burst arrives, so the thousand callers are genuinely at expiry, not
// beside it.
//
// Arms: `burst` (20 VUs x 50 parallel GETs — project A's latest posts list, the hot key),
// `burst_b` (4 VUs x 50 — project B's, the permission-crossing check: every B answer must
// carry B-marked titles), `warm` (a few reads a second across both lists and an entry of
// each — between bursts they are the ordinary traffic that keeps the cache honest), and
// `churn` (republishes one A entry every 12 s — a new generation is a NEW key, so a churn
// flip is a second, sharper stampede that needs no TTL). Every answer is counted by its
// Colophon-Cache value: `miss` counts the fills the run forced; `hit`, `coalesced`,
// `bypass` the rest; `l04_stale` counts a read that came back at an older generation than
// the same VU already saw; `l04_crossed` counts a list carrying the other project's mark.
//
// The verdict's fill bound is what single-flight makes possible: each of the four warm
// keys can die at most once per TTL, plus a few fills per churn generation — a run with
// no coalescing would show thousands of misses, so the bound is generous and still tiny.
// `steady` and `throughput` run the same plan; the arms are time-boxed — TOTAL and VUS
// are unused. scripts/load.sh gives this endpoint RATE 10 (the warm arm) and P99_MS 250.

import http from 'k6/http';
import { check, fail, sleep } from 'k6';
import exec from 'k6/execution';
import { Counter } from 'k6/metrics';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const RATE = parseInt(__ENV.RATE || '10', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '250');
// Must equal the COLOPHON_CACHE_TTL_MS scripts/load.sh gives this endpoint's server — the
// round the bursts align to, and the fill bound's unit.
const TTL_MS = parseInt(__ENV.CACHE_TTL_MS || '4000', 10);
const ROUND_MS = TTL_MS;
const CHURN_SECONDS = parseInt(__ENV.CHURN_SECONDS || '12', 10);
const BURST_A_VUS = parseInt(__ENV.BURST_A_VUS || '20', 10);
const BURST_B_VUS = parseInt(__ENV.BURST_B_VUS || '4', 10);
const CALLERS = 50; // a burst VU's parallel asks: A bursts 20x50 = 1,000 callers at expiry

const fills = new Counter('l04_miss');
const coalesced = new Counter('l04_coalesced');
const hits = new Counter('l04_hit');
const bypassed = new Counter('l04_bypass');
const stale = new Counter('l04_stale');
const crossed = new Counter('l04_crossed');
const errors = new Counter('l04_errors');
const flips = new Counter('l04_flips');

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}
const RUN_SECONDS = durationSeconds(DURATION);

if (!(SCENARIO === 'throughput' || SCENARIO === 'steady')) {
  throw new Error('SCENARIO must be throughput or steady');
}

const thresholds = {
  checks: ['rate==1'],
  http_req_failed: ['rate==0'],
  l04_errors: ['count==0'],
  l04_stale: ['count==0'],
  l04_crossed: ['count==0'],
  l04_bypass: ['count==0'], // the cache is up all run — an uncached fill would be a bug here
  http_req_duration: [`p(99)<${P99_MS}`],
  'http_reqs{arm:warm}': ['count>0'],
  'http_reqs{arm:burst}': ['count>0'],
};

export const options = {
  scenarios: {
    burst: { executor: 'constant-vus', vus: BURST_A_VUS, duration: DURATION, exec: 'burst' },
    burst_b: { executor: 'constant-vus', vus: BURST_B_VUS, duration: DURATION, exec: 'burstB' },
    warm: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: 8, maxVUs: 16, exec: 'warm' },
    // One VU, one publisher: constant-arrival-rate can spread its iterations across several
    // VUs, and a second churn VU's own churnState races this one's etag and generation.
    churn: { executor: 'constant-vus', vus: 1, duration: DURATION, exec: 'churn' },
  },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '300s',
};

const POSTS = {
  name: 'Posts',
  title_field: 'headline',
  fields: [
    { id: 'headline', label: 'Headline', type: 'text', required: true, index: true },
    { id: 'dek', label: 'Dek', type: 'text' },
  ],
};

function key() {
  return `k6-l04-${__VU}-${__ITER}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
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
  const make = (mark) => {
    const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `${slug}-${mark.toLowerCase()}`, name: `k6 stampede ${mark}` }), { headers: write });
    if (project.status !== 201) fail(`project ${mark}: HTTP ${project.status} ${project.body}`);
    const id = project.json('data.id');
    const env = `${BASE_URL}/api/v1/projects/${id}/environments/production`;
    const made = http.post(`${env}/content-types`, JSON.stringify({ slug: 'posts', definition: POSTS }), {
      headers: Object.assign({ 'Idempotency-Key': `k6-type-${slug}-${mark}` }, write),
    });
    if (made.status !== 201) fail(`type ${mark}: HTTP ${made.status} ${made.body}`);
    let file = '';
    for (let i = 0; i < 100; i += 1) {
      file += JSON.stringify({ headline: `${mark}-Post ${i}`, dek: `dek ${i}` }) + '\n';
    }
    const imported = http.post(`${env}/collections/posts/import`, file, {
      headers: Object.assign({}, write, { 'Content-Type': 'application/jsonlines', 'Idempotency-Key': `k6-seed-${slug}-${mark}` }),
    });
    if (imported.status !== 201) fail(`seed ${mark}: HTTP ${imported.status} ${imported.body}`);
    const ids = imported.json('data.outcomes').map((o) => o.entry);
    const set = http.post(`${env}/changesets`, JSON.stringify({ title: 'release 1', entries: ids }), {
      headers: Object.assign({ 'Idempotency-Key': `k6-cs-${slug}-${mark}` }, write),
    });
    if (set.status !== 201) fail(`changeset ${mark}: HTTP ${set.status} ${set.body}`);
    const path = `${env}/changesets/${set.json('data.id')}`;
    const ifMatch = Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, write);
    http.post(`${path}/submit`, null, { headers: ifMatch });
    http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch });
    const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: 0 }), {
      headers: Object.assign({ 'Idempotency-Key': `k6-pub-${slug}-${mark}` }, ifMatch),
    });
    if (published.status !== 201) fail(`publish ${mark}: HTTP ${published.status} ${published.body}`);
    return {
      id, env, mark,
      list: `${BASE_URL}/content/v1/${id}/production/posts?limit=100`,
      entries: `${env}/collections/posts/entries`,
      entry: `${BASE_URL}/content/v1/${id}/production/posts/${ids[0]}`,
      entryId: ids[0],
      generation: 1,
    };
  };
  const a = make('A');
  const b = make('B');
  return { session, a, b };
}

// Where a read's answer came from, and whether it told the truth. `mark` is the letter
// this project's titles all start with — a list answering with the other project's mark
// is permission-crossing reuse.
function account(res, mark, seen) {
  const how = res.headers['Colophon-Cache'];
  if (how === 'miss') fills.add(1);
  else if (how === 'coalesced') coalesced.add(1);
  else if (how === 'hit') hits.add(1);
  else if (how === 'bypass') bypassed.add(1);
  const generation = parseInt(res.headers['Colophon-Generation'] || '-1', 10);
  if (generation >= 0 && generation < seen.g) stale.add(1);
  if (generation > seen.g) seen.g = generation;
  if (res.status === 200) {
    const rows = res.json('data');
    const list = Array.isArray(rows) ? rows : [rows];
    for (const row of list) {
      const title = row && (row.title || (row.data && row.data.title));
      if (typeof title === 'string' && !title.startsWith(`${mark}-`)) { crossed.add(1); break; }
    }
  }
}

// The shared run clock: every burst VU sleeps to the same ROUND boundary, then lets a
// CALLERS-wide batch fly — the bursts all land inside the same few hundred milliseconds.
function sleepToRound() {
  const now = exec.instance.currentTestRunDuration;
  if (typeof now !== 'number') fail('k6 does not report currentTestRunDuration');
  const wait = (ROUND_MS - (now % ROUND_MS)) / 1000;
  if (wait > 0.001) sleep(wait);
}

export function burst(data) {
  const seen = { g: 0 };
  const params = { redirects: 0, tags: { arm: 'burst', endpoint: 'list', name: 'burst-list' } };
  for (;;) {
    sleepToRound();
    const replies = http.batch(new Array(CALLERS).fill(['GET', data.a.list, null, params]));
    for (const res of replies) {
      if (!check(res, { 'burst read 200': (r) => r.status === 200 })) { errors.add(1); console.log(`burst read: ${res.status} ${String(res.body).slice(0, 160)}`); }
      account(res, 'A', seen);
    }
    sleep(0.05);
  }
}

export function burstB(data) {
  const seen = { g: 0 };
  const params = { redirects: 0, tags: { arm: 'burst', endpoint: 'list', name: 'burst-list' } };
  for (;;) {
    sleepToRound();
    const replies = http.batch(new Array(CALLERS).fill(['GET', data.b.list, null, params]));
    for (const res of replies) {
      if (!check(res, { 'burst B read 200': (r) => r.status === 200 })) { errors.add(1); console.log(`burst B read: ${res.status} ${String(res.body).slice(0, 160)}`); }
      account(res, 'B', seen);
    }
    sleep(0.05);
  }
}

// A warm VU's own truth about the newest generation each project served it (module state
// is per-VU in k6 — no cross-VU leakage). A later read that answers at an older one is
// a stale refill, which the verdict fails.
const warmSeen = { A: { g: 0 }, B: { g: 0 } };

export function warm(data) {
  const picks = [
    [data.a.list, 'A', warmSeen.A], [data.b.list, 'B', warmSeen.B],
    [data.a.entry, 'A', warmSeen.A], [data.b.entry, 'B', warmSeen.B],
  ];
  const [url, mark, seen] = picks[__ITER % picks.length];
  const res = http.get(url, { redirects: 0, tags: { arm: 'warm', endpoint: 'warm', name: 'warm-read' } });
  if (!check(res, { 'warm read 200': (r) => r.status === 200 })) { errors.add(1); console.log(`warm read: ${res.status} ${String(res.body).slice(0, 160)}`); }
  account(res, mark, seen);
}

// The churn VU's own truth: the generation it published last and the etag of the entry
// it keeps revising. It is project A's only publisher (one VU, one loop), so
// base_generation never conflicts.
let churnState = null;
export function churn(data) {
  for (;;) {
    churnOnce(data);
    sleep(CHURN_SECONDS);
  }
}
function churnOnce(data) {
  if (!churnState) {
    const current = http.get(`${data.a.entries}/${data.a.entryId}`, { headers: { Cookie: data.session.cookie }, redirects: 0 });
    churnState = { etag: current.headers['Etag'], generation: 1, round: 0 };
  }
  churnState.round += 1;
  const write = { Cookie: data.session.cookie, 'X-CSRF-Token': data.session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };
  const text = `churn ${churnState.round}`;
  const saved = http.put(`${data.a.entries}/${data.a.entryId}`, JSON.stringify({
    headline: 'A-Post 0', dek: text,
  }), {
    headers: Object.assign({ 'If-Match': churnState.etag, 'Idempotency-Key': key() }, write),
    redirects: 0, tags: { arm: 'churn', endpoint: 'save', name: 'entries/{entry}' },
  });
  if (!check(saved, { 'churn save 200': (r) => r.status === 200 })) { errors.add(1); console.log(`churn save: ${saved.status} ${String(saved.body).slice(0, 160)}`); return; }
  churnState.etag = saved.headers['Etag'];
  const set = http.post(`${data.a.env}/changesets`, JSON.stringify({ title: `churn ${churnState.round}`, entries: [data.a.entryId] }), {
    headers: Object.assign({ 'Idempotency-Key': key() }, write), redirects: 0, tags: { arm: 'churn', endpoint: 'changeset', name: 'changesets' },
  });
  if (!check(set, { 'churn changeset 201': (r) => r.status === 201 })) { errors.add(1); console.log(`churn changeset: ${set.status} ${String(set.body).slice(0, 160)}`); return; }
  const path = `${data.a.env}/changesets/${set.json('data.id')}`;
  const ifMatch = Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, write);
  http.post(`${path}/submit`, null, { headers: ifMatch, redirects: 0, tags: { arm: 'churn', endpoint: 'submit', name: 'changesets/{changeset}/submit' } });
  http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch, redirects: 0, tags: { arm: 'churn', endpoint: 'review', name: 'changesets/{changeset}/reviews' } });
  const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: churnState.generation }), {
    headers: Object.assign({ 'Idempotency-Key': key() }, ifMatch), redirects: 0, tags: { arm: 'churn', endpoint: 'publish', name: 'changesets/{changeset}/publish' },
  });
  if (!check(published, { 'churn publish 201': (r) => r.status === 201 && r.json('data.generation') === churnState.generation + 1 })) { errors.add(1); console.log(`churn publish: ${published.status} ${String(published.body).slice(0, 160)}`); return; }
  churnState.generation = published.json('data.generation');
  flips.add(1);
}

/** Computes the L04 verdict: the fills the run forced against what single-flight permits,
 *  the coalesced waiters, and the stale/crossed/error counters that must all be zero. */
export function handleSummary(data) {
  const count = (name) => (data.metrics[name] ? data.metrics[name].values.count || 0 : 0);
  const fillCount = count('l04_miss');
  const flipsCount = count('l04_flips');
  // Each of the four warm keys can expire at most once a TTL; a churn generation's key
  // lives one CHURN_SECONDS, so about CHURN_SECONDS/TTL expiries plus its first fill;
  // the slack covers requests straddling a boundary.
  const perGenerationFills = Math.ceil(CHURN_SECONDS / (TTL_MS / 1000)) + 1;
  const fillBound = Math.ceil(RUN_SECONDS / (TTL_MS / 1000)) * 4 + flipsCount * perGenerationFills + 8;
  const gates = {
    bounded_regeneration: fillCount > 0 && fillCount <= fillBound,
    coalescing_seen: count('l04_coalesced') > 0,
    no_stale_refills: count('l04_stale') === 0,
    no_permission_crossing: count('l04_crossed') === 0,
    no_uncached_fills: count('l04_bypass') === 0,
    no_errors: count('l04_errors') === 0,
  };
  data.l04 = {
    spec: 'L04: hot-key stampede — 1,000 callers at expiry; bounded regeneration and coalescing; no permission-crossing reuse',
    gate: `fills <= expiries-per-key x keys + churn-generation fills (${fillBound} this run); coalesced > 0; zero stale, crossed, bypass or errors`,
    cache_ttl_ms: TTL_MS,
    round_ms: ROUND_MS,
    burst_callers: BURST_A_VUS * CALLERS,
    churn_generations: flipsCount,
    served: { hit: count('l04_hit'), miss: fillCount, coalesced: count('l04_coalesced'), bypass: count('l04_bypass') },
    fill_bound: fillBound,
    stale_reads: count('l04_stale'),
    crossed_reads: count('l04_crossed'),
    errors: count('l04_errors'),
    gates,
    held: Object.values(gates).every(Boolean),
  };
  return {
    stdout: [
      '',
      '  L04 — hot-key stampede',
      `    ${fillCount} fills (bound ${fillBound}), ${count('l04_coalesced')} coalesced waiters, ${count('l04_hit')} hits, ${flipsCount} generation flips`,
      `    stale ${count('l04_stale')}, crossed ${count('l04_crossed')}, bypass ${count('l04_bypass')}, errors ${count('l04_errors')}`,
      `    verdict: ${data.l04.held ? 'HELD' : 'BROKE'}`,
      '',
    ].join('\n'),
  };
}
