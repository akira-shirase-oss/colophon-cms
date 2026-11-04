// Load scenario for L11 (spec §30; D-127): measured proof that the delivery cache is
// disposable — Valkey can be dead for minutes while anonymous /content/v1 traffic runs,
// and the run sees every read still answered (from Postgres, marked `Colophon-Cache:
// bypass` or `coalesced`), writes still accepted, no refusal storm from the admission
// limiter it shares with the cache, bounded origin pressure, and cache hits and
// latency back to normal afterwards.
//
// The outage is real, not simulated: a one-iteration `conductor` arm asks the receiver
// scripts/load.sh starts to `docker compose pause valkey` at DURATION and `unpause`
// it near the end of the outage window — a mid-run freeze, TCP connections open and
// hung, the way a dependency actually dies. The scenario's own clock decides when: each
// iteration works out which of the five windows it is in from
// exec.instance.currentTestRunDuration and tags every request `phase`
// (baseline|margin|outage|recovery). The two `margin` windows absorb the seconds around
// the pause and unpause calls where either answer is honest; only the clean windows are
// gated, so nothing about the verdict depends on how fast docker froze the process.
//
// Arms: `reads` (anonymous delivery reads — latest list and entry), `writes` (signed-in
// If-Match saves, one lazily-created article per VU), `readyz` (readiness polled
// through the run: a dead cache degrades, it must never say not_ready), `sentinel`
// (creates one article mid-outage, then reads it back in the recovery window — accepted
// work is not lost), `conductor` (the pause/unpause calls, each checked 200).
// handleSummary() computes the verdict into `l11`; scripts/load.sh stores it and fails
// the run when `held` is false.
//
// The arms are time-boxed, not iteration-counted: TOTAL and VUS are unused. RATE is the
// delivery-read rate, WRITE_RATE the save rate; DURATION is the length of EACH of the
// three measured phases, so a run lasts about 3x DURATION. `steady` and `throughput`
// run the same phased plan; CI's smoke passes a short DURATION. scripts/load.sh gives
// this endpoint RATE 400 and P99_MS 250.

import http from 'k6/http';
import { check, fail, sleep } from 'k6';
import exec from 'k6/execution';
import { Counter } from 'k6/metrics';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const RECEIVER_URL = __ENV.RECEIVER_URL || 'http://127.0.0.1:58096';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const RATE = parseInt(__ENV.RATE || '400', 10);
const WRITE_RATE = parseInt(__ENV.WRITE_RATE || `${Math.max(5, Math.ceil(RATE / 40))}`, 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '250');
// Each bypassed read can spend a beat waiting out the dead-connection deadline before
// the backoff learns the cache is gone: VUs need headroom so the arrival rate holds.
const R_VUS = parseInt(__ENV.R_VUS || `${Math.max(32, Math.ceil(RATE / 5))}`, 10);
const W_VUS = parseInt(__ENV.W_VUS || `${Math.max(8, Math.ceil(WRITE_RATE / 2))}`, 10);

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const PHASE = durationSeconds(DURATION);
const GAP = 3; // seconds: the pause (and later the unpause) land inside the margins
const TOTAL_SECONDS = PHASE * 3 + GAP * 3;

if (!(SCENARIO === 'throughput' || SCENARIO === 'steady')) {
  throw new Error('SCENARIO must be throughput or steady');
}

// The window `now` ms into the run falls in. The conductor pauses Valkey at PHASE and
// unpauses it at 2*PHASE + 2*GAP, so [PHASE+GAP, 2*PHASE+GAP) is certainly dead and
// [2*PHASE+3*GAP, ...) certainly alive again; the margins are where either is honest.
function phaseOf(nowMs) {
  const w = nowMs / 1000;
  if (w < PHASE) return 'baseline';
  if (w < PHASE + GAP) return 'margin';
  if (w < PHASE * 2 + GAP) return 'outage';
  if (w < PHASE * 2 + GAP * 3) return 'margin';
  return 'recovery';
}

function phase() {
  const w = exec.instance.currentTestRunDuration;
  if (typeof w !== 'number') fail('k6 does not report currentTestRunDuration');
  return phaseOf(w);
}

// Purpose-named counters: top-level metric names land in the exported summary on their
// own, where tagged submetrics only appear when a threshold names them.
const readsByPhase = {
  baseline: new Counter('l11_reads_baseline'),
  margin: new Counter('l11_reads_margin'),
  outage: new Counter('l11_reads_outage'),
  recovery: new Counter('l11_reads_recovery'),
};
const outageServed = {
  hit: new Counter('l11_outage_hit'),
  miss: new Counter('l11_outage_miss'),
  coalesced: new Counter('l11_outage_coalesced'),
  bypass: new Counter('l11_outage_bypass'),
  absent: new Counter('l11_outage_absent'),
};
const recoveryHit = new Counter('l11_recovery_hit');
const readErrors = new Counter('l11_read_errors');
const writeErrors = new Counter('l11_write_errors');
const refusals = new Counter('l11_refusals');
const readyzOutageDegraded = new Counter('l11_readyz_outage_degraded');
const readyzOutageReady = new Counter('l11_readyz_outage_ready'); // probe lag, not a lie
const readyzOffDegraded = new Counter('l11_readyz_off_degraded'); // degraded outside the outage
const readyzWrong = new Counter('l11_readyz_wrong'); // not_ready or unreadable, any window
const readyzReady = new Counter('l11_readyz_ready');
const sentinelCreated = new Counter('l11_sentinel_created');
const sentinelVerified = new Counter('l11_sentinel_verified');
const sentinelFailed = new Counter('l11_sentinel_failed');
const controlErrors = new Counter('l11_control_errors');

const thresholds = {
  checks: ['rate==1'],
  http_req_failed: ['rate==0'],
  l11_read_errors: ['count==0'],
  l11_write_errors: ['count==0'],
  // The outage must not manufacture refusals: the shared limiter degrades, it does not refuse (D-076, D-118).
  l11_refusals: ['count==0'],
  // A hit or a miss while the cache is dead would mean the outage never happened.
  l11_outage_hit: ['count==0'],
  l11_outage_miss: ['count==0'],
  l11_outage_absent: ['count==0'],
  l11_readyz_wrong: ['count==0'],
  l11_control_errors: ['count==0'],
  'http_reqs{phase:outage}': ['count>0'],
  'http_reqs{phase:baseline}': ['count>0'],
  'http_reqs{phase:recovery}': ['count>0'],
  'http_req_duration{phase:baseline}': [`p(99)<${P99_MS}`],
  'http_req_duration{phase:recovery}': [`p(99)<${P99_MS}`],
  // Bounded, not fast: a bypassed read pays a Postgres fill, and once a second at most a
  // 100 ms dead-connection attempt (D-048). Two seconds is the outage's honest bound.
  'http_req_duration{phase:outage}': ['p(99)<2000'],
};
if (SCENARIO === 'steady') {
  const scheduled = (RATE + WRITE_RATE + 2) * TOTAL_SECONDS;
  thresholds.dropped_iterations = [`count<=${Math.max(2, Math.floor(scheduled * 0.001))}`];
}

export const options = {
  scenarios: {
    reads: {
      executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: `${TOTAL_SECONDS}s`,
      preAllocatedVUs: R_VUS, maxVUs: R_VUS * 2, exec: 'read',
    },
    writes: {
      executor: 'constant-arrival-rate', rate: WRITE_RATE, timeUnit: '1s', duration: `${TOTAL_SECONDS}s`,
      preAllocatedVUs: W_VUS, maxVUs: W_VUS * 2, exec: 'write',
    },
    readyz: {
      executor: 'constant-arrival-rate', rate: 2, timeUnit: '1s', duration: `${TOTAL_SECONDS}s`,
      preAllocatedVUs: 2, maxVUs: 4, exec: 'probe',
    },
    sentinel: {
      executor: 'per-vu-iterations', vus: 1, iterations: 1,
      startTime: `${PHASE + GAP + Math.floor(PHASE / 3)}s`, exec: 'sentinel',
    },
    conductor: {
      executor: 'per-vu-iterations', vus: 1, iterations: 1,
      startTime: `${PHASE}s`, exec: 'conductor',
    },
  },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '300s',
};

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

const article = (title, text) => ({
  title,
  body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] } },
});

export function setup() {
  const owner = signIn('dev-owner');
  const run = Date.now().toString(36);
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-${run}`, name: 'k6 L11' }), { headers: owner });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const base = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;
  const entries = `${base}/collections/articles/entries`;

  // A release behind the delivery reads: 20 published articles.
  const live = [];
  for (let i = 0; i < 20; i += 1) {
    const created = http.post(entries, JSON.stringify(article(`Public ${i}`, 'published')), { headers: Object.assign({ 'Idempotency-Key': `k6-l11-${run}-p${i}` }, owner) });
    if (created.status !== 201) fail(`public article ${i}: HTTP ${created.status}`);
    live.push(created.json('data.id'));
  }
  const changes = { title: 'release 1', entries: live };
  const set = http.post(`${base}/changesets`, JSON.stringify(changes), { headers: Object.assign({ 'Idempotency-Key': `k6-l11-${run}-c` }, owner) });
  if (set.status !== 201) fail(`changeset: HTTP ${set.status} ${set.body}`);
  const path = `${base}/changesets/${set.json('data.id')}`;
  const ifMatch = Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, owner);
  http.post(`${path}/submit`, null, { headers: ifMatch });
  http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch });
  const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: 0 }), { headers: Object.assign({ 'Idempotency-Key': `k6-l11-${run}-p` }, ifMatch) });
  if (published.status !== 201) fail(`publish: HTTP ${published.status} ${published.body}`);

  return { owner, entries, content: `${BASE_URL}/content/v1/${project.json('data.id')}/production/articles`, live, run };
}

function note(res) {
  if (res.status === 429) refusals.add(1);
}

/** One anonymous delivery read: the latest list, or a published entry. */
export function read(data) {
  const tags = { phase: phase() };
  let res;
  if (__ITER % 4 === 0) {
    res = http.get(`${data.content}/${data.live[__ITER % data.live.length]}`, { redirects: 0, tags: Object.assign({ name: 'content/{entry}' }, tags) });
  } else {
    res = http.get(data.content, { redirects: 0, tags: Object.assign({ name: 'content' }, tags) });
  }
  readsByPhase[tags.phase].add(1);
  note(res);
  const served = res.headers['Colophon-Cache'] || 'absent';
  if (tags.phase === 'outage' && served in outageServed) outageServed[served].add(1);
  if (tags.phase === 'recovery' && served === 'hit') recoveryHit.add(1);
  const ok = check(res, { 'read 200': (r) => r.status === 200, 'read names a generation': (r) => r.headers['Colophon-Generation'] !== undefined });
  if (!ok) readErrors.add(1);
}

// Per write-arm VU: its own article, created lazily, and the last etag a save of it was
// acknowledged with. Lazy creation, not a setup pool: exec.vu.idInTest is global across
// every scenario, so a fixed pool could hand one article to two VUs and their saves
// would conflict — a fake failure the scenario would manufacture itself.
let mine = null;
let etag = null;

/** One signed-in iteration: a save on this VU's own article, or a read-back of it. The
 *  reads check the last save the server acknowledged is the last save it serves. */
export function write(data) {
  const tags = { phase: phase() };
  const headers = {
    Cookie: data.owner.Cookie, 'X-CSRF-Token': data.owner['X-CSRF-Token'], Origin: PUBLIC_URL, 'Content-Type': 'application/json',
  };
  if (mine === null) {
    const created = http.post(data.entries, JSON.stringify(article(`Draft by VU ${exec.vu.idInTest}`, 'start')), {
      headers: Object.assign({ 'Idempotency-Key': `k6-l11-${data.run}-w${exec.vu.idInTest}` }, headers),
      redirects: 0, tags: Object.assign({ name: 'entries', endpoint: 'create' }, tags),
    });
    note(created);
    if (check(created, { 'write-arm article 201': (r) => r.status === 201 })) {
      mine = created.json('data.id');
      etag = created.headers.Etag;
    } else {
      writeErrors.add(1);
    }
    return;
  }
  const url = `${data.entries}/${mine}`;
  if (__ITER % 3 === 0) {
    const res = http.put(url, JSON.stringify(article(`Article by VU ${exec.vu.idInTest}`, `iteration ${__ITER}`)), {
      headers: Object.assign({ 'If-Match': etag, 'Idempotency-Key': `k6-l11-${data.run}-${exec.vu.idInTest}-${__ITER}` }, headers),
      redirects: 0, tags: Object.assign({ name: 'entries/{entry}', endpoint: 'save' }, tags),
    });
    note(res);
    const ok = check(res, {
      'save 200': (r) => r.status === 200,
      'save gives a new ETag': (r) => typeof r.headers.Etag === 'string' && r.headers.Etag !== etag,
    });
    if (ok) etag = res.headers.Etag;
    else writeErrors.add(1);
  } else {
    const res = http.get(url, { headers, redirects: 0, tags: Object.assign({ name: 'entries/{entry}', endpoint: 'read' }, tags) });
    note(res);
    const ok = check(res, {
      'read 200': (r) => r.status === 200,
      'read returns the last accepted save': (r) => r.headers.Etag === etag,
    });
    if (!ok) writeErrors.add(1);
  }
}

/** One readiness poll: a dead cache is a degraded server, never a dead one. */
export function probe() {
  const tags = { phase: phase() };
  const res = http.get(`${BASE_URL}/readyz`, { redirects: 0, tags: Object.assign({ name: 'readyz' }, tags) });
  note(res);
  let state = 'error';
  if (res.status === 200) state = res.json('status') === 'degraded' ? 'degraded' : 'ready';
  else if (res.status === 503) state = 'not_ready';
  if (state === 'not_ready' || state === 'error') readyzWrong.add(1);
  else if (state === 'degraded' && tags.phase === 'outage') readyzOutageDegraded.add(1);
  else if (state === 'degraded' && (tags.phase === 'baseline' || tags.phase === 'recovery')) readyzOffDegraded.add(1);
  else if (state === 'ready' && tags.phase === 'outage') readyzOutageReady.add(1);
  else if (state === 'ready') readyzReady.add(1);
  check(res, {
    'readyz answers 200': (r) => r.status === 200,
    'readyz is never not_ready for a dead cache': () => state !== 'not_ready',
  });
}

/** One article created while the cache is dead, then read back in the recovery window:
 *  accepted work is not lost. */
export function sentinel(data) {
  const created = http.post(data.entries, JSON.stringify(article(`Sentinel ${data.run}`, 'written while the cache was down')), {
    headers: Object.assign({ 'Idempotency-Key': `k6-l11-${data.run}-sentinel` }, data.owner),
    redirects: 0, tags: { name: 'entries', phase: 'outage' },
  });
  note(created);
  const id = check(created, { 'sentinel create 201': (r) => r.status === 201 }) && created.json('data.id');
  if (!id) {
    sentinelFailed.add(1);
    return;
  }
  sentinelCreated.add(1);
  // Land inside the recovery window: created at ~PHASE+GAP+PHASE/3, read after 2*PHASE+3*GAP.
  sleep(PHASE * 0.7 + GAP * 2);
  const res = http.get(`${data.entries}/${id}`, { headers: { Cookie: data.owner.Cookie }, redirects: 0, tags: { name: 'entries/{entry}', phase: 'recovery' } });
  note(res);
  const verified = check(res, {
    'sentinel read-back 200': (r) => r.status === 200,
    'the outage write is the served write': (r) => r.json('data.revision.title') === `Sentinel ${data.run}`,
  });
  if (verified) sentinelVerified.add(1);
  else sentinelFailed.add(1);
}

/** The fault injector's clock: pause Valkey at the phase boundary, unpause it inside the
 *  second margin. Both calls are to the receiver's /control endpoint (D-127). */
export function conductor() {
  const down = http.post(`${RECEIVER_URL}/control`, JSON.stringify({ fault: 'valkey', action: 'down' }), {
    headers: { 'Content-Type': 'application/json' }, tags: { name: 'control', phase: 'margin' },
  });
  if (!check(down, { 'valkey pause acknowledged': (r) => r.status === 200 })) controlErrors.add(1);
  sleep(PHASE + GAP * 2);
  const up = http.post(`${RECEIVER_URL}/control`, JSON.stringify({ fault: 'valkey', action: 'up' }), {
    headers: { 'Content-Type': 'application/json' }, tags: { name: 'control', phase: 'margin' },
  });
  if (!check(up, { 'valkey unpause acknowledged': (r) => r.status === 200 })) controlErrors.add(1);
}

const count = (data, name) => (data.metrics[name] ? data.metrics[name].values.count : 0);
// Rate metrics carry {rate, passes, fails} — `passes` counts the TRUE samples (for
// http_req_failed, the failed requests), not a `.count` it doesn't have.
const passes = (data, name) => (data.metrics[name] ? data.metrics[name].values.passes || 0 : 0);
const p99 = (data, phase) => (data.metrics[`http_req_duration{phase:${phase}}`] || { values: {} }).values['p(99)'];

/** Computes the L11 verdict and attaches it to the exported summary as `l11`. */
export function handleSummary(data) {
  const outage = {
    reads: count(data, 'l11_reads_outage'),
    hit: count(data, 'l11_outage_hit'),
    miss: count(data, 'l11_outage_miss'),
    coalesced: count(data, 'l11_outage_coalesced'),
    bypass: count(data, 'l11_outage_bypass'),
    absent: count(data, 'l11_outage_absent'),
    p99_ms: p99(data, 'outage'),
  };
  const recovery = {
    reads: count(data, 'l11_reads_recovery'),
    hit: count(data, 'l11_recovery_hit'),
    p99_ms: p99(data, 'recovery'),
  };
  const windows = {
    baseline: { reads: count(data, 'l11_reads_baseline'), p99_ms: p99(data, 'baseline') },
    margin: { reads: count(data, 'l11_reads_margin') },
    outage,
    recovery,
  };
  const totalReads = windows.baseline.reads + windows.margin.reads + outage.reads + recovery.reads;
  const readFailed = count(data, 'l11_read_errors');
  const refused = count(data, 'l11_refusals');
  const hitShare = recovery.reads > 0 ? recovery.hit / recovery.reads : 0;

  const gates = {
    outage_ran: outage.reads > 0,
    // Every read in every window answered: a dead cache never surfaces as an error.
    reads_answered: readFailed === 0 && passes(data, 'http_req_failed') === 0,
    // The outage was real: while Valkey was paused nothing was a hit or a miss, and every
    // read was served by the Postgres fallback (bypass or a coalesced fill) — nobody served
    // an answer without saying how.
    fallback_honest: outage.hit === 0 && outage.miss === 0 && outage.absent === 0 && outage.bypass + outage.coalesced === outage.reads,
    // Bounded origin pressure: each bypassed leader did its one Postgres fill — no retry
    // storm can have amplified reads into fills.
    origin_bounded: outage.bypass <= outage.reads,
    // Writes were accepted in every window, and the write made mid-outage reads back.
    writes_held: count(data, 'l11_write_errors') === 0 && count(data, 'l11_sentinel_created') === 1 && count(data, 'l11_sentinel_verified') === 1 && count(data, 'l11_sentinel_failed') === 0,
    // Degrade, not refuse: the shared admission limiter did not manufacture a single 429.
    degrade_not_refuse: refused === 0,
    // The server itself said the truth: readiness went degraded, never not_ready — and
    // only the probe's first beats of lag get a 'ready' during the outage window.
    readyz_honest: count(data, 'l11_readyz_outage_degraded') > 0 && count(data, 'l11_readyz_wrong') === 0 && count(data, 'l11_readyz_outage_ready') <= 8 && count(data, 'l11_readyz_off_degraded') <= 8,
    // The cache is back: recovery reads are hits again, and inside the p99 budget.
    recovered: recovery.hit > 0 && hitShare >= 0.5 && recovery.p99_ms != null && recovery.p99_ms < P99_MS,
    controlled: count(data, 'l11_control_errors') === 0,
  };
  data.l11 = {
    spec: 'L11: cache unavailable for five minutes — rate-limited fallback, no lost accepted work, bounded origin pressure, correct recovery',
    gate: 'outage reads all 200 and served bypass|coalesced only; zero 429; bypass fills <= reads; writes and sentinel held; readyz degraded but never not_ready; recovery hit share >= 50% and p99 inside budget',
    windows_seconds: { phase: PHASE, margins: GAP, outage_spans: [PHASE + GAP, PHASE * 2 + GAP], injected: `docker compose pause valkey at ~${PHASE}s; unpause at ~${PHASE * 2 + GAP * 2}s` },
    reads: { ...windows, total: totalReads, errors: readFailed },
    writes: { errors: count(data, 'l11_write_errors') },
    refusals_429: refused,
    readyz: { degraded_in_outage: count(data, 'l11_readyz_outage_degraded'), ready_in_outage: count(data, 'l11_readyz_outage_ready'), ready_elsewhere: count(data, 'l11_readyz_ready') },
    sentinel: { created: count(data, 'l11_sentinel_created'), verified: count(data, 'l11_sentinel_verified') },
    gates,
    held: Object.values(gates).every(Boolean),
  };
  return {
    stdout: [
      '',
      '  L11 — delivery cache unavailable mid-run (Valkey paused, then unpaused)',
      `    outage reads: ${outage.reads} (${outage.bypass} bypass fills + ${outage.coalesced} coalesced; ${outage.hit} hits — must be 0), errors ${readFailed}`,
      `    recovery: ${recovery.reads} reads, hit share ${(hitShare * 100).toFixed(1)}%, p99 ${recovery.p99_ms == null ? 'n/a' : recovery.p99_ms.toFixed(1)}ms (budget ${P99_MS}ms)`,
      `    refusals 429: ${refused}; readyz degraded in outage: ${count(data, 'l11_readyz_outage_degraded')} polls`,
      `    verdict: ${data.l11.held ? 'HELD' : 'BROKE'}`,
      '',
    ].join('\n'),
  };
}
