// Load scenario for L12 (spec §30; D-127): measured proof that when PostgreSQL stalls
// mid-run and the worker is killed at the same moment, the API tells the truth — writes
// fail with real errors, never a fake commit — and everything resumes when the
// dependency comes back.
//
// The outage is real, not simulated: a one-iteration `conductor` arm asks the receiver
// scripts/load.sh starts to `docker compose pause postgres` at DURATION, then to
// SIGKILL the separate worker process load.sh spawned for this endpoint; near the end
// of the window it unpauses and respawns the worker. A paused container keeps its TCP
// connections open and hung — the honest shape of "Postgres slow/unavailable", not the
// easier refuse-connection version — and a worker SIGKILLed mid-claim leaves leased
// jobs for the restarted worker to pick up when its lease lapses.
//
// Each iteration works out which of the five windows it is in from
// exec.instance.currentTestRunDuration and tags every request `phase`
// (baseline|margin|outage|recovery). The margins absorb the seconds around the
// pause/unpause/kill calls where either answer is honest; only the clean windows are
// gated. What the verdict checks, per window:
//
//   - every write the API called a success (200) must later read back — a 200 without
//     the revision would be a fake commit (F02's class of bug; the run stops here);
//   - writes and reads during the outage end in 5xx or a client timeout — honest
//     failure classes. A delivery read can still answer 200 from the cache, marked
//     `Colophon-Cache: hit` (the disposable-accelerator contract); a 200 claiming a
//     Postgres fill while it was frozen would be impossible, and any other 200 is a
//     buffered pre-freeze straggler, which stays bounded;
//   - /readyz says not_ready while the database is gone (never 'degraded' for a dead
//     database — that's the cache's word) and comes back after;
//   - jobs that were pending or mid-attempt when the worker died finish after the
//     restart (the watch arm polls {P}/jobs until every one is terminal);
//   - a write acknowledged before the kill reads back after recovery (the sentinel arm).
//
// Arms: `reads` (delivery + management reads, all Postgres-bound), `writes` (signed-in
// If-Match saves, one lazily-created article per VU), `readyz`, `publishes` (a slow
// publish arm feeding the outbox), `sentinel`, `watch`, `conductor`.
// handleSummary() computes the verdict into `l12`; scripts/load.sh stores it and fails
// the run when `held` is false.
//
// The arms are time-boxed, not iteration-counted: TOTAL and VUS are unused. RATE is the
// read rate, WRITE_RATE the save rate; DURATION is the length of EACH of the three
// measured phases (a run lasts about 3x DURATION + margins). `steady` and `throughput`
// run the same plan; CI's smoke passes a short DURATION. scripts/load.sh gives this
// endpoint RATE 100 and P99_MS 250; the outage-phase p99 gate is the request deadline,
// not the budget — an honest failure answers in about a pool-acquire timeout.

import http from 'k6/http';
import { check, fail, sleep } from 'k6';
import exec from 'k6/execution';
import { Counter, Gauge } from 'k6/metrics';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const RECEIVER_URL = __ENV.RECEIVER_URL || 'http://127.0.0.1:58096';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const RATE = parseInt(__ENV.RATE || '100', 10);
const WRITE_RATE = parseInt(__ENV.WRITE_RATE || `${Math.max(5, Math.ceil(RATE / 5))}`, 10);
const PUBLISH_SECONDS = parseInt(__ENV.PUBLISH_SECONDS || '8', 10); // one publish every N seconds
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '250');
// Every request during the outage waits out the pool's acquire timeout (~2 s): an arm
// needs about three VUs per request a second it offers.
const R_VUS = parseInt(__ENV.R_VUS || `${Math.max(32, Math.ceil(RATE * 3))}`, 10);
const W_VUS = parseInt(__ENV.W_VUS || `${Math.max(8, Math.ceil(WRITE_RATE * 3))}`, 10);

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const PHASE = durationSeconds(DURATION);
const GAP = 4; // seconds: the pause, kill, unpause and respawn land inside the margins
const TOTAL_SECONDS = PHASE * 3 + GAP * 4;

if (!(SCENARIO === 'throughput' || SCENARIO === 'steady')) {
  throw new Error('SCENARIO must be throughput or steady');
}

// The conductor pauses Postgres and kills the worker at PHASE, and unpauses and
// respawns at 2*PHASE + 2*GAP, so [PHASE+GAP, 2*PHASE+GAP) is certainly dead and
// [2*PHASE+4*GAP, ...) certainly back; the margins are where either is honest.
function phaseOf(nowMs) {
  const w = nowMs / 1000;
  if (w < PHASE) return 'baseline';
  if (w < PHASE + GAP) return 'margin';
  if (w < PHASE * 2 + GAP) return 'outage';
  if (w < PHASE * 2 + GAP * 4) return 'margin';
  return 'recovery';
}

// Purpose-named counters: top-level metric names land in the exported summary on their
// own, where tagged submetrics only appear when a threshold names them.
const PHASES = ['baseline', 'margin', 'outage', 'recovery'];
const CLASSES = ['ok', 'unavailable', 'timeout', 'refused', 'weird'];
const readCounters = {};
const writeCounters = {};
for (const ph of PHASES) {
  for (const cls of CLASSES) {
    readCounters[`${ph}_${cls}`] = new Counter(`l12_reads_${ph}_${cls}`);
    writeCounters[`${ph}_${cls}`] = new Counter(`l12_writes_${ph}_${cls}`);
  }
}
const readyzCounters = {}; // l12_readyz_{phase}_{ready|degraded|not_ready|error}
for (const ph of PHASES) {
  for (const st of ['ready', 'degraded', 'not_ready', 'error']) readyzCounters[`${ph}_${st}`] = new Counter(`l12_readyz_${ph}_${st}`);
}
const publishCounters = {}; // l12_publishes_{phase}_{ok|failed}
for (const ph of PHASES) {
  for (const result of ['ok', 'failed']) publishCounters[`${ph}_${result}`] = new Counter(`l12_publishes_${ph}_${result}`);
}
const sentinelStages = {}; // created|acknowledged|lost|published|publish_failed|create_failed
for (const stage of ['created', 'acknowledged', 'lost', 'published', 'publish_failed', 'create_failed']) {
  sentinelStages[stage] = new Counter(`l12_sentinel_${stage}`);
}
const jobResults = {};
for (const result of ['resumed', 'stuck', 'failed']) jobResults[result] = new Counter(`l12_jobs_${result}`);
const corrupt = new Counter('l12_corrupt'); // a write the API acknowledged but can't serve back
// While Postgres is dead a delivery read can still answer 200 — honestly, from the
// cache, marked Colophon-Cache: hit (D-048: the cache is a disposable accelerator). A
// 200 claiming a Postgres fill in the same window (miss|bypass|absent) is impossible,
// and the header saying so would be the lie L12 exists to catch.
const cacheHitOutage = new Counter('l12_cache_hit_outage');
const cacheFillOutage = new Counter('l12_cache_fill_outage'); // 200 claiming a Postgres fill while dead
const maxAttempts = new Gauge('l12_job_max_attempts');
const refusals = new Counter('l12_refusals');
const controlErrors = new Counter('l12_control_errors');

function phase() {
  const w = exec.instance.currentTestRunDuration;
  if (typeof w !== 'number') fail('k6 does not report currentTestRunDuration');
  return phaseOf(w);
}

const clean = (phaseName) => phaseName === 'baseline' || phaseName === 'recovery';
// An honest answer to a Postgres-dependent request while Postgres is frozen: a 5xx
// (usually 503 unavailable after the pool's acquire timeout) or a client timeout.
const honest = (r) => r.status === 200 || r.status === 0 || r.status >= 500;
// A 409 (and a 412) is a definitive refusal — a stale If-Match writes nothing (entries.rs:
// refused with the current ETag), so its fate is known: it is not an outage symptom.
const refusal = (r) => r.status === 429 || r.status === 412 || r.status === 409;
const classOf = (res) => {
  if (res.status === 0) return 'timeout';
  if (res.status >= 500) return 'unavailable';
  if (res.status === 200 || res.status === 201) return 'ok';
  if (refusal(res)) return 'refused';
  return 'weird';
};

const thresholds = {
  checks: ['rate==1'],
  l12_corrupt: ['count==0'],
  l12_refusals: ['count==0'],
  l12_control_errors: ['count==0'],
  l12_cache_fill_outage: ['count==0'],
  'http_reqs{phase:outage}': ['count>0'],
  'http_req_duration{phase:baseline}': [`p(99)<${P99_MS}`],
  'http_req_duration{phase:recovery}': [`p(99)<${P99_MS}`],
  // The outage's honest bound is the server's request deadline, not the budget.
  'http_req_duration{phase:outage}': ['p(99)<12000'],
  'http_req_duration{phase:margin}': ['p(99)<12000'],
};
if (SCENARIO === 'steady') {
  const scheduled = (RATE + WRITE_RATE + 2) * TOTAL_SECONDS;
  thresholds.dropped_iterations = [`count<=${Math.max(2, Math.floor(scheduled * 0.005))}`];
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
    publishes: {
      executor: 'constant-arrival-rate', rate: 1, timeUnit: `${PUBLISH_SECONDS}s`, duration: `${TOTAL_SECONDS}s`,
      preAllocatedVUs: 4, maxVUs: 8, exec: 'publish',
    },
    watch: {
      executor: 'per-vu-iterations', vus: 1, iterations: 1,
      startTime: `${PHASE * 2 + GAP * 2 + 1}s`, exec: 'watch',
    },
    sentinel: {
      executor: 'per-vu-iterations', vus: 1, iterations: 1,
      startTime: `${Math.floor(PHASE / 2)}s`, exec: 'sentinel',
    },
    conductor: {
      executor: 'per-vu-iterations', vus: 1, iterations: 1,
      startTime: `${PHASE}s`, exec: 'conductor',
    },
  },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '300s',
  teardownTimeout: '180s',
};

const EXPECTED = { responseCallback: http.expectedStatuses(200, 201, 204, 400, 401, 403, 404, 409, 410, 412, 422, 429, 500, 502, 503, 504) };

function signIn(username) {
  const jar = new http.CookieJar();
  const signedIn = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: `${username}@colophon.test`, password: 'colophon-dev' }), { jar, headers: { 'Content-Type': 'application/json' } });
  if (signedIn.status !== 204) fail(`sign-in as ${username} failed: HTTP ${signedIn.status}`);
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
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-${run}`, name: 'k6 L12' }), { headers: owner });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const projectId = project.json('data.id');
  const base = `${BASE_URL}/api/v1/projects/${projectId}/environments/production`;
  const entries = `${base}/collections/articles/entries`;
  // A webhook and a probe, keyed to this project's own site at the receiver: announcements
  // to /hook/<id> advance what /site/<id> reports, so no other project's releases can ever
  // look verified here (several publish-arm projects run beside this one).
  for (const [body, name] of [
    [{ name: 'receiver', url: `${RECEIVER_URL}/hook/${projectId}` }, 'destination'],
    [{ kind: 'probe', name: 'site', url: `${RECEIVER_URL}/site/${projectId}` }, 'probe'],
  ]) {
    const made = http.post(`${base}/destinations`, JSON.stringify(body), { headers: owner });
    if (made.status !== 201) fail(`${name}: HTTP ${made.status} ${made.body}`);
  }
  // A release behind the reads (and jobs for the outbox to run through baseline).
  const live = [];
  for (let i = 0; i < 20; i += 1) {
    const created = http.post(entries, JSON.stringify(article(`Public ${i}`, 'published')), { headers: Object.assign({ 'Idempotency-Key': `k6-l12-${run}-p${i}` }, owner) });
    if (created.status !== 201) fail(`public article ${i}: HTTP ${created.status}`);
    live.push(created.json('data.id'));
  }
  const set = http.post(`${base}/changesets`, JSON.stringify({ title: 'release 1', entries: live }), { headers: Object.assign({ 'Idempotency-Key': `k6-l12-${run}-c` }, owner) });
  if (set.status !== 201) fail(`changeset: HTTP ${set.status} ${set.body}`);
  const path = `${base}/changesets/${set.json('data.id')}`;
  const ifMatch = Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, owner);
  http.post(`${path}/submit`, null, { headers: ifMatch });
  http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch });
  const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: 0 }), { headers: Object.assign({ 'Idempotency-Key': `k6-l12-${run}-p` }, ifMatch) });
  if (published.status !== 201) fail(`publish: HTTP ${published.status} ${published.body}`);
  return { owner, base, entries, content: `${BASE_URL}/content/v1/${project.json('data.id')}/production/articles`, live, run };
}

function note(res) {
  if (res.status === 429) refusals.add(1);
}

/** One Postgres-bound read: a delivery list, a published entry, or a management
 *  list/read — every one needs the database, so the outage must show honest failures. */
export function read(data) {
  const tags = { phase: phase() };
  const headers = { Cookie: data.owner.Cookie };
  let res;
  const roll = __ITER % 5;
  if (roll < 2) {
    res = http.get(data.content, Object.assign({ redirects: 0, tags: Object.assign({ name: 'content' }, tags) }, EXPECTED));
  } else if (roll === 2) {
    res = http.get(`${data.content}/${data.live[__ITER % data.live.length]}`, Object.assign({ redirects: 0, tags: Object.assign({ name: 'content/{entry}' }, tags) }, EXPECTED));
  } else if (roll === 3) {
    res = http.get(`${data.entries}?limit=10`, Object.assign({ headers, redirects: 0, tags: Object.assign({ name: 'entries?limit' }, tags) }, EXPECTED));
  } else {
    res = http.get(`${data.entries}/${data.live[(__ITER + 1) % data.live.length]}`, Object.assign({ headers, redirects: 0, tags: Object.assign({ name: 'entries/{entry}' }, tags) }, EXPECTED));
  }
  note(res);
  // Classify the answer by the window it ARRIVED in, not the window the request was
  // issued in: a request still queued at the unpause returns 200 in the margin where
  // 'ok' is honest, while only an answer observed inside the outage window can be a lie.
  const resPhase = phase();
  const cls = classOf(res);
  readCounters[`${resPhase}_${cls}`].add(1);
  if (resPhase === 'outage' && res.status === 200) {
    const served = res.headers['Colophon-Cache'];
    if (served === 'hit') cacheHitOutage.add(1);
    else if (served === 'miss' || served === 'bypass' || served === 'absent') cacheFillOutage.add(1);
  }
  check(res, { 'read is an honest class (no 4xx games)': (r) => honest(r) || r.status === 404 });
}

// Per write-arm VU: its lazily-created article, the last etag a save was acknowledged
// with, and the titles of saves whose fate the client never learned (they may or may
// not have committed — a read either resyncs onto them or finds the acknowledged etag).
let mine = null;
let etag = null;
let pending = [];

function resync(res) {
  // A 200 read either confirms the acknowledged etag, or finds a silently-committed
  // pending save — both honest. Anything else is a lost or fake commit.
  const title = res.json('data.revision.title');
  if (res.headers.Etag === etag) {
    pending = [];
    return true;
  }
  if (pending.indexOf(title) !== -1) {
    etag = res.headers.Etag;
    pending = [];
    return true;
  }
  return false;
}

/** One signed-in iteration: a save on this VU's own article, or a read-back of it. */
export function write(data) {
  const tags = { phase: phase() };
  const headers = {
    Cookie: data.owner.Cookie, 'X-CSRF-Token': data.owner['X-CSRF-Token'], Origin: PUBLIC_URL, 'Content-Type': 'application/json',
  };
  if (mine === null) {
    const created = http.post(data.entries, JSON.stringify(article(`Draft by VU ${exec.vu.idInTest}`, 'start')), {
      headers: Object.assign({ 'Idempotency-Key': `k6-l12-${data.run}-w${exec.vu.idInTest}` }, headers),
      redirects: 0, tags: Object.assign({ name: 'entries', endpoint: 'create' }, tags), responseCallback: EXPECTED.responseCallback,
    });
    note(created);
    const createdPhase = phase(); // the window the answer arrived in
    writeCounters[`${createdPhase}_${classOf(created)}`].add(1);
    if (check(created, { 'write-arm article 201': (r) => r.status === 201 || !clean(createdPhase) })) {
      if (created.status === 201) {
        mine = created.json('data.id');
        etag = created.headers.Etag;
      }
    }
    return;
  }
  const url = `${data.entries}/${mine}`;
  if (__ITER % 3 === 0) {
    // The title IS the identity a read-back resyncs onto: it has to be unique, not the
    // body's text, because data.revision.title is what the served revision reports.
    const title = `VU ${exec.vu.idInTest} save ${__ITER}`;
    const res = http.put(url, JSON.stringify(article(title, `iteration ${__ITER}`)), {
      headers: Object.assign({ 'If-Match': etag, 'Idempotency-Key': `k6-l12-${data.run}-${exec.vu.idInTest}-${__ITER}` }, headers),
      redirects: 0, tags: Object.assign({ name: 'entries/{entry}', endpoint: 'save' }, tags), responseCallback: EXPECTED.responseCallback,
    });
    note(res);
    writeCounters[`${phase()}_${classOf(res)}`].add(1);
    check(res, { 'save answers an honest class': (r) => honest(r) || r.status === 412 || r.status === 409 });
    if (res.status === 200) {
      etag = res.headers.Etag;
      pending = [];
    } else if (!refusal(res)) {
      // A 409/412 refused the save outright — nothing committed, nothing to resync (the
      // next read-back adopts the current revision if an earlier unacknowledged save had
      // in fact landed). A 5xx or a timeout leaves the save's fate unknown: its title
      // joins the set a read may resync onto, since it may have committed without its answer.
      pending.push(title);
      if (pending.length > 3) pending.shift();
    }
  } else {
    const res = http.get(url, {
      headers, redirects: 0, tags: Object.assign({ name: 'entries/{entry}', endpoint: 'read' }, tags), responseCallback: EXPECTED.responseCallback,
    });
    note(res);
    readCounters[`${phase()}_${classOf(res)}`].add(1);
    const consistent = check(res, {
      'read-back is a real revision (the last ack or a pending save)': (r) => r.status !== 200 || resync(r),
      'read answers an honest class': (r) => honest(r),
    });
    if (!consistent) corrupt.add(1);
  }
}

/** One readiness poll: the database probe must say not_ready while Postgres is frozen. */
export function probe() {
  const tags = { phase: phase() };
  const res = http.get(`${BASE_URL}/readyz`, Object.assign({ redirects: 0, tags: Object.assign({ name: 'readyz' }, tags) }, EXPECTED));
  note(res);
  let state = 'error';
  if (res.status === 200) state = res.json('status') === 'degraded' ? 'degraded' : 'ready';
  else if (res.status === 503) state = 'not_ready';
  readyzCounters[`${phase()}_${state}`].add(1);
  check(res, { 'readyz answers': (r) => r.status === 200 || r.status === 503 });
}

// The publish arm's per-VU project, created lazily so each VU is the only publisher
// on it (exec.vu.idInTest is global across scenarios — a fixed pool could collide).
let mineBase = null;
let mineGeneration = 0;
let mineEntry = null;

/** One publish iteration: the five-request pipeline — entry, changeset, submit,
 *  approve, publish — on this VU's own project. During the outage every step fails
 *  honestly; afterwards the releases feed the resumed worker. */
export function publish(data) {
  const tags = { phase: phase() };
  const headers = {
    Cookie: data.owner.Cookie, 'X-CSRF-Token': data.owner['X-CSRF-Token'], Origin: PUBLIC_URL, 'Content-Type': 'application/json',
  };
  const params = (endpoint, extra) => ({
    headers: Object.assign({}, headers, extra || {}), redirects: 0,
    tags: Object.assign({ name: endpoint, endpoint }, tags), responseCallback: EXPECTED.responseCallback,
  });
  if (mineBase === null) {
    const run = `${data.run}-p${exec.vu.idInTest}`;
    const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-${run}`, name: `k6 L12 ${exec.vu.idInTest}` }), params('projects', { 'Idempotency-Key': `k6-l12-${run}` }));
    if (project.status !== 201) {
      note(project);
      publishCounters[`${tags.phase}_failed`].add(1);
      return;
    }
    const base = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;
    const pid = project.json('data.id');
    const made = http.post(`${base}/destinations`, JSON.stringify({ name: 'receiver', url: `${RECEIVER_URL}/hook/${pid}` }), params('destinations', { 'Idempotency-Key': `k6-l12-${run}-d` }));
    const site = http.post(`${base}/destinations`, JSON.stringify({ kind: 'probe', name: 'site', url: `${RECEIVER_URL}/site/${pid}` }), params('destinations', { 'Idempotency-Key': `k6-l12-${run}-s` }));
    if (made.status !== 201 || site.status !== 201) {
      publishCounters[`${tags.phase}_failed`].add(1);
      return;
    }
    mineBase = base;
  }
  const idem = (s) => `k6-l12-${data.run}-p${exec.vu.idInTest}-${__ITER}-${s}`;
  const entry = http.post(`${mineBase}/collections/articles/entries`, JSON.stringify(article(`Publish ${__ITER}`, 'published')), params('entry', { 'Idempotency-Key': idem('e') }));
  if (entry.status !== 201) {
    publishCounters[`${tags.phase}_failed`].add(1);
    return;
  }
  mineEntry = entry.json('data.id');
  const set = http.post(`${mineBase}/changesets`, JSON.stringify({ title: `k6 ${__ITER}`, entries: [mineEntry] }), params('changeset', { 'Idempotency-Key': idem('c') }));
  if (set.status !== 201) {
    publishCounters[`${tags.phase}_failed`].add(1);
    return;
  }
  const path = `${mineBase}/changesets/${set.json('data.id')}`;
  const ifMatch = { 'If-Match': `"${set.json('data.hash')}"` };
  const submitted = http.post(`${path}/submit`, null, params('submit', ifMatch));
  if (submitted.status !== 200) {
    publishCounters[`${tags.phase}_failed`].add(1);
    return;
  }
  const approved = http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), params('review', ifMatch));
  if (approved.status !== 200) {
    publishCounters[`${tags.phase}_failed`].add(1);
    return;
  }
  const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: mineGeneration }), params('publish', Object.assign({ 'Idempotency-Key': idem('p') }, ifMatch)));
  publishCounters[`${tags.phase}_${published.status === 201 ? 'ok' : 'failed'}`].add(1);
  if (published.status === 201) mineGeneration = published.json('data.generation');
}

/** The sentinel: one article created and acknowledged in the baseline, one publish
 *  committed just before the fault so the killed worker holds (or is owed) its jobs,
 *  then — in the recovery window — the acknowledged write must read back exactly. */
export function sentinel(data) {
  const created = http.post(data.entries, JSON.stringify(article(`Sentinel ${data.run}`, 'acknowledged before the kill')), {
    headers: Object.assign({ 'Idempotency-Key': `k6-l12-${data.run}-sentinel` }, data.owner),
    redirects: 0, tags: { name: 'entries', phase: 'baseline' },
  });
  if (!check(created, { 'sentinel create 201': (r) => r.status === 201 })) {
    sentinelStages.create_failed.add(1);
    return;
  }
  const id = created.json('data.id');
  const acknowledged = created.headers.Etag;
  sentinelStages.created.add(1);
  // Publish a release carrying it just before the fault: its jobs are pending or held
  // under the dead worker's lease while Postgres is frozen.
  sleep(Math.max(0, PHASE / 2 - 3));
  const headers = data.owner;
  const params = (endpoint, extra) => ({ headers: Object.assign({}, headers, extra || {}), redirects: 0, tags: { name: endpoint, phase: 'margin' }, responseCallback: EXPECTED.responseCallback });
  const set = http.post(`${data.base}/changesets`, JSON.stringify({ title: 'sentinel release', entries: [id] }), params('changeset', { 'Idempotency-Key': `k6-l12-${data.run}-sc` }));
  const path = set.status === 201 && `${data.base}/changesets/${set.json('data.id')}`;
  let published = null;
  if (path) {
    const ifMatch = { 'If-Match': `"${set.json('data.hash')}"` };
    http.post(`${path}/submit`, null, params('submit', ifMatch));
    http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), params('review', ifMatch));
    published = http.post(`${path}/publish`, JSON.stringify({ base_generation: 1 }), params('publish', Object.assign({ 'Idempotency-Key': `k6-l12-${data.run}-sp` }, ifMatch)));
  }
  sentinelStages[published && published.status === 201 ? 'published' : 'publish_failed'].add(1);
  // Land inside the recovery window: published near PHASE, read after 2*PHASE+4*GAP.
  sleep(PHASE + GAP * 4);
  const res = http.get(`${data.entries}/${id}`, { headers: { Cookie: data.owner.Cookie }, redirects: 0, tags: { name: 'entries/{entry}', phase: 'recovery' }, responseCallback: EXPECTED.responseCallback });
  const ok = check(res, {
    'acknowledged write read back': (r) => r.status === 200 && r.headers.Etag === acknowledged && r.json('data.revision.title') === `Sentinel ${data.run}`,
  });
  sentinelStages[ok ? 'acknowledged' : 'lost'].add(1);
}

/** The jobs watch: after the unpause + respawn, every delivery job must reach a
 *  terminal state — the pending ones, and the ones the dead worker had leased. */
export function watch(data) {
  const headers = { Cookie: data.owner.Cookie };
  const deadline = (TOTAL_SECONDS + 60) * 1000;
  let done = false;
  let failed = false;
  let attempts = 0;
  while (exec.instance.currentTestRunDuration < deadline && !done && !failed) {
    const jobs = http.get(`${data.base}/jobs`, { headers, tags: { name: 'jobs', phase: 'recovery' }, responseCallback: EXPECTED.responseCallback });
    if (jobs.status === 200) {
      const list = jobs.json('data');
      for (const job of list) {
        if (job.attempts > attempts) {
          attempts = job.attempts;
          maxAttempts.add(attempts);
        }
        if (job.status === 'failed') failed = true;
      }
      if (!failed && list.length > 0 && list.every((job) => job.status === 'succeeded' || job.status === 'superseded')) done = true;
    }
    if (!done && !failed) sleep(2);
  }
  jobResults[failed ? 'failed' : done ? 'resumed' : 'stuck'].add(1);
}

/** The fault injector's clock: pause Postgres and kill the worker at the phase
 *  boundary; unpause, then respawn the worker, inside the second margin. */
export function conductor() {
  const call = (body) => {
    const res = http.post(`${RECEIVER_URL}/control`, JSON.stringify(body), {
      headers: { 'Content-Type': 'application/json' }, tags: { name: 'control', phase: 'margin' },
    });
    if (!check(res, { 'control acknowledged': (r) => r.status === 200 })) controlErrors.add(1);
    return res;
  };
  call({ fault: 'postgres', action: 'down' });
  call({ fault: 'worker', action: 'down' });
  sleep(PHASE + GAP * 2);
  call({ fault: 'postgres', action: 'up' });
  sleep(2); // let Postgres take connections before the worker boots
  call({ fault: 'worker', action: 'up' });
}

export function teardown(data) {
  // What the watch arm couldn't outlive is asserted here: every job terminal, and the
  // latest release verified somewhere — the same end-state publishing.js requires.
  const headers = { Cookie: data.owner.Cookie };
  const deadline = Date.now() + 90000;
  for (;;) {
    const jobs = http.get(`${data.base}/jobs`, { headers });
    if (jobs.status === 200) {
      const list = jobs.json('data');
      if (list.length > 0 && list.every((job) => job.status === 'succeeded' || job.status === 'superseded')) break;
      if (list.find((job) => job.status === 'failed')) fail(`a job failed during/after the outage: ${JSON.stringify(list.slice(0, 3))}`);
    }
    if (Date.now() > deadline) fail(`jobs never reached a terminal state after the outage: ${jobs.status === 200 ? JSON.stringify(jobs.json('data').slice(0, 3)) : `HTTP ${jobs.status}`}`);
    sleep(0.5);
  }
}

const count = (data, name) => (data.metrics[name] ? data.metrics[name].values.count : 0);

/** Computes the L12 verdict and attaches it to the exported summary as `l12`. */
export function handleSummary(data) {
  const byPhase = (prefix) => {
    const out = {};
    for (const ph of PHASES) {
      out[ph] = {};
      for (const cls of CLASSES) out[ph][cls] = count(data, `${prefix}_${ph}_${cls}`);
      out[ph].total = Object.values(out[ph]).reduce((a, b) => a + b, 0);
    }
    return out;
  };
  const readTable = byPhase('l12_reads');
  const writeTable = byPhase('l12_writes');
  const readyzCount = (phaseName, state) => count(data, `l12_readyz_${phaseName}_${state}`);
  const sentinelStage = (stage) => count(data, `l12_sentinel_${stage}`);
  const publishCount = (phaseName, result) => count(data, `l12_publishes_${phaseName}_${result}`);
  const jobsCount = (result) => count(data, `l12_jobs_${result}`);
  const refused = count(data, 'l12_refusals');
  const corrupted = count(data, 'l12_corrupt');
  const hitWhileDead = count(data, 'l12_cache_hit_outage');
  const fillWhileDead = count(data, 'l12_cache_fill_outage');
  const recoveryReads = readTable.recovery;
  const recoveryWrites = writeTable.recovery;
  const recoveryOk = recoveryReads.ok + recoveryWrites.ok;
  const recoveryTotal = recoveryReads.total + recoveryWrites.total;
  const attemptsMetric = data.metrics.l12_job_max_attempts;
  const attempts = attemptsMetric ? attemptsMetric.values.max || attemptsMetric.values.value || 0 : 0;
  const recoveryP99 = data.metrics['http_req_duration{phase:recovery}'];

  const gates = {
    outage_ran: readTable.outage.total > 0 && writeTable.outage.total > 0,
    // During the freeze, reads and writes end in 5xx or timeouts — a 200 can be a
    // delivery read served honestly from the cache (marked hit), or a buffered
    // pre-freeze straggler (bounded); a 200 claiming a Postgres fill while Postgres
    // was frozen is impossible, and no 4xx may masquerade as an answer.
    reads_fail_honestly: readTable.outage.ok - hitWhileDead <= 2 && readTable.outage.weird === 0 && fillWhileDead === 0,
    writes_fail_honestly: writeTable.outage.ok <= 2 && writeTable.outage.weird === 0,
    // No fake commits anywhere in the run: every acknowledged save read back exactly.
    no_fake_commit: corrupted === 0,
    // The acknowledged sentinel write survived the outage and the kill.
    acknowledged_held: sentinelStage('created') === 1 && sentinelStage('acknowledged') === 1 && sentinelStage('lost') === 0,
    // Readiness told the truth: not_ready while the database was frozen, then back.
    readyz_honest: readyzCount('outage', 'not_ready') > 0 && readyzCount('outage', 'ready') <= 8 && readyzCount('recovery', 'not_ready') === 0 && readyzCount('recovery', 'error') === 0,
    // The outbox resumed: the restarted worker (or its reclaim of the dead worker's
    // lease) finished every job; none failed or stayed stuck.
    jobs_resumed: jobsCount('resumed') === 1 && jobsCount('stuck') === 0 && jobsCount('failed') === 0,
    // Recovery is real, not a drain: post-window traffic answers 200 again.
    recovered: recoveryTotal > 0 && recoveryOk / recoveryTotal >= 0.99 && recoveryP99 != null && recoveryP99.values['p(99)'] < P99_MS,
    no_refusals: refused === 0,
    controlled: count(data, 'l12_control_errors') === 0,
  };
  data.l12 = {
    spec: 'L12: PostgreSQL slow/unavailable and worker kill — writes fail honestly, no fake commit, jobs resume, cache policy stays explicit',
    gate: 'outage reads/writes end in 5xx|timeout (<=2 buffered 200s each); zero corrupt read-backs; sentinel ack read back; readyz not_ready then ready; every job terminal; no 429; recovery 99% ok inside p99 budget',
    windows_seconds: { phase: PHASE, margins: GAP, outage_spans: [PHASE + GAP, PHASE * 2 + GAP], injected: `docker compose pause postgres + SIGKILL worker at ~${PHASE}s; unpause + respawn at ~${PHASE * 2 + GAP * 2}s` },
    reads: readTable,
    writes: writeTable,
    cache_hit_while_dead: hitWhileDead,
    cache_fill_while_dead: fillWhileDead,
    publishes: { baseline: publishCount('baseline', 'ok'), outage: publishCount('outage', 'ok'), outage_failed: publishCount('outage', 'failed'), recovery: publishCount('recovery', 'ok') },
    readyz: { not_ready_in_outage: readyzCount('outage', 'not_ready'), ready_in_outage: readyzCount('outage', 'ready'), ready_in_recovery: readyzCount('recovery', 'ready') },
    jobs: { result: { resumed: jobsCount('resumed'), stuck: jobsCount('stuck'), failed: jobsCount('failed') }, max_attempts: attempts },
    sentinel: { created: sentinelStage('created'), published: sentinelStage('published'), acknowledged: sentinelStage('acknowledged'), lost: sentinelStage('lost') },
    corrupt_readbacks: corrupted,
    refusals_429: refused,
    recovery: { requests: recoveryTotal, ok: recoveryOk, ok_share: recoveryTotal > 0 ? recoveryOk / recoveryTotal : 0 },
    gates,
    held: Object.values(gates).every(Boolean),
  };
  return {
    stdout: [
      '',
      '  L12 — PostgreSQL paused and the worker SIGKILLed mid-run',
      `    outage: ${readTable.outage.total} reads (${readTable.outage.unavailable} 5xx, ${readTable.outage.timeout} timeouts, ${readTable.outage.ok} ok), ${writeTable.outage.total} writes (${writeTable.outage.unavailable} 5xx, ${writeTable.outage.timeout} timeouts, ${writeTable.outage.ok} ok)`,
      `    corrupt read-backs: ${corrupted}; readyz not_ready in outage: ${readyzCount('outage', 'not_ready')} polls; jobs resumed: ${jobsCount('resumed')}`,
      `    recovery: ${recoveryOk}/${recoveryTotal} ok (${((recoveryTotal > 0 ? recoveryOk / recoveryTotal : 0) * 100).toFixed(1)}%)`,
      `    verdict: ${data.l12.held ? 'HELD' : 'BROKE'}`,
      '',
    ].join('\n'),
  };
}
