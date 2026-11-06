// Load scenario for L13 (spec §30; D-127): measured proof that a CDN that answers
// purges with 429/5xx and a site whose rebuild is late both end in bounded retries and
// truthful states — pending or needs_attention, never an accepted purge or a verified
// release that didn't happen — and that the backlog drains when the receiver heals.
//
// The fault is real: the receiver scripts/load.sh starts refuses every POST /purge with
// 503 and a Retry-After, and freezes the generation its /site answers — a purge sink
// that refuses and a rebuilt site that lags. A one-iteration `conductor` arm flips both
// at DURATION through the receiver's /control endpoint and heals them near the end of
// the window; each iteration tags its requests `phase` from
// exec.instance.currentTestRunDuration so only clean windows are gated.
//
// What the run proves, beside the API staying healthy throughout:
//   - the worker honors the failure's Retry-After: the receiver records every purge
//     attempt with its idempotency key, and the per-key gap between consecutive
//     attempts never goes under the asked-for wait (a hot loop would show gaps of
//     milliseconds) — and no job retries past its bounded attempt count;
//   - while the fault holds, nothing reports success it didn't earn: the audit arm
//     samples {P}/releases and {P}/jobs — the largest generation marked
//     purge:'accepted' or label:'verified' may not grow while the sink is down (an
//     accepted purge needs a 204 the receiver isn't giving; a verified label needs a
//     site showing the release, and the site is frozen);
//   - the sentinel arm's publish made during the outage is observed pending (or
//     needs_attention) while the fault holds, then — after the heal — drains to
//     verified with its purge accepted;
//   - teardown requires what publishing.js does: every project's every release
//     verified at one location with an accepted purge.
//
// Arms: `publishes` (the five-request pipeline at RATE publishes a second, one lazily
// created project per VU), `audit` (the release/job sampler), `sentinel`, `stats`
// (collects the receiver's attempt log near the end), `conductor`.
// handleSummary() computes the verdict into `l13`; scripts/load.sh stores it and fails
// the run when `held` is false.
//
// The arms are time-boxed, not iteration-counted: TOTAL and VUS are unused. DURATION is
// the length of EACH of the three measured phases. `steady` and `throughput` run the
// same plan; CI's smoke passes a short DURATION. scripts/load.sh gives this endpoint
// RATE 8 and P99_MS 700 — publishing's editorial write budget.

import http from 'k6/http';
import { check, fail, sleep } from 'k6';
import exec from 'k6/execution';
import { Counter, Gauge } from 'k6/metrics';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const RECEIVER_URL = __ENV.RECEIVER_URL || 'http://127.0.0.1:58096';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const RATE = parseInt(__ENV.RATE || '8', 10); // publishes a second; each is five requests
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '700');
const RETRY_AFTER = parseInt(__ENV.RETRY_AFTER_S || '5', 10); // seconds the sink asks for
const P_VUS = parseInt(__ENV.P_VUS || `${Math.max(8, Math.ceil(RATE * 1.5))}`, 10);
// The worker's bounded retry counts, per job kind (colophon-domain job.rs): posts
// (webhook and purge) get MAX_ATTEMPTS, probes get MAX_PROBE_ATTEMPTS — "not yet" is a
// probe's usual answer, so a frozen site lets it look more times than a post may try.
const MAX_ATTEMPTS_BOUND = 5;
const MAX_PROBE_ATTEMPTS_BOUND = 10;

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const PHASE = durationSeconds(DURATION);
const GAP = 2; // seconds: mode flips are instant; the margin only absorbs call latency
const TOTAL_SECONDS = PHASE * 3 + GAP * 4;

if (!(SCENARIO === 'throughput' || SCENARIO === 'steady')) {
  throw new Error('SCENARIO must be throughput or steady');
}

// The conductor fails the purge sink and freezes the site at PHASE and heals both at
// 2*PHASE + 2*GAP, so [PHASE+GAP, 2*PHASE+GAP) is certainly failing; the margins cover
// the call edges and the first scheduled retries after the heal.
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
const publishCounters = {}; // l13_publishes_{phase}_{ok|failed}
for (const ph of PHASES) {
  for (const result of ['ok', 'failed']) publishCounters[`${ph}_${result}`] = new Counter(`l13_publishes_${ph}_${result}`);
}
const falsePurge = new Counter('l13_false_purge'); // accepted-purge growth while the sink is down
const falseVerified = new Counter('l13_false_verified'); // verified-label growth while the site is frozen
const sentinelStages = {}; // outage_published|pending_while_down|false_early|recovered|stuck|outage_publish_failed
for (const stage of ['outage_published', 'pending_while_down', 'false_early', 'recovered', 'stuck', 'outage_publish_failed']) {
  sentinelStages[stage] = new Counter(`l13_sentinel_${stage}`);
}
const maxAttemptsPost = new Gauge('l13_job_max_attempts_post');
const maxAttemptsProbe = new Gauge('l13_job_max_attempts_probe');
const purgeGap = new Gauge('l13_purge_min_gap_ms'); // smallest per-key gap between attempts
const purgeFailedAttempts = new Gauge('l13_purge_failed_attempts');
const controlErrors = new Counter('l13_control_errors');

function phase() {
  const w = exec.instance.currentTestRunDuration;
  if (typeof w !== 'number') fail('k6 does not report currentTestRunDuration');
  return phaseOf(w);
}

const thresholds = {
  checks: ['rate==1'],
  http_req_failed: ['rate==0'],
  l13_false_purge: ['count==0'],
  l13_false_verified: ['count==0'],
  l13_control_errors: ['count==0'],
  l13_publishes_outage_ok: ['count>0'],
  http_req_duration: [`p(99)<${P99_MS}`],
};
if (SCENARIO === 'steady') {
  const scheduled = RATE * TOTAL_SECONDS * 5 + TOTAL_SECONDS;
  thresholds.dropped_iterations = [`count<=${Math.max(1, Math.floor(scheduled * 0.0005))}`];
}

export const options = {
  scenarios: {
    publishes: {
      executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: `${TOTAL_SECONDS}s`,
      preAllocatedVUs: P_VUS, maxVUs: P_VUS * 2, exec: 'publish',
    },
    audit: {
      executor: 'constant-arrival-rate', rate: 1, timeUnit: '1s', duration: `${TOTAL_SECONDS}s`,
      preAllocatedVUs: 2, maxVUs: 4, exec: 'audit',
    },
    sentinel: {
      executor: 'per-vu-iterations', vus: 1, iterations: 1,
      startTime: `${PHASE + GAP + 1}s`, exec: 'sentinel',
    },
    stats: {
      executor: 'per-vu-iterations', vus: 1, iterations: 1,
      startTime: `${PHASE * 2 + GAP * 2 + 10}s`, exec: 'stats',
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

const article = (title) => ({
  title,
  body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: title }] }] } },
});

export function setup() {
  const owner = signIn('dev-owner');
  const run = Date.now().toString(36);
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-${run}`, name: 'k6 L13' }), { headers: owner });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const projectId = project.json('data.id');
  const base = `${BASE_URL}/api/v1/projects/${projectId}/environments/production`;
  // The audited project gets its own keyed site at the receiver: announcements to
  // /hook/<id> advance what /site/<id> reports — no other project's generations can
  // ever make one of this project's releases look verified (the publish arm runs
  // projects of its own beside it).
  for (const body of [
    { name: 'receiver', url: `${RECEIVER_URL}/hook/${projectId}` },
    { kind: 'cdn', name: 'cdn', url: `${RECEIVER_URL}/purge` },
    { kind: 'probe', name: 'site', url: `${RECEIVER_URL}/site/${projectId}` },
  ]) {
    const made = http.post(`${base}/destinations`, JSON.stringify(body), { headers: owner });
    if (made.status !== 201) fail(`destination: HTTP ${made.status} ${made.body}`);
  }
  return { owner, base, run };
}

// The publish arm's per-VU project, created lazily: each VU is its only publisher, so
// base_generation never races (exec.vu.idInTest is global across scenarios).
let mineBase = null;
let mineGeneration = 0;

function ensureProject(data, headers, tags) {
  if (mineBase) return true;
  const run = `${data.run}-p${exec.vu.idInTest}`;
  const params = (extra) => ({ headers: Object.assign({}, headers, extra || {}), redirects: 0, tags: Object.assign({ name: 'setup' }, tags) });
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-${run}`, name: `k6 L13 ${exec.vu.idInTest}` }), params({ 'Idempotency-Key': `k6-l13-${run}` }));
  if (project.status !== 201) return false;
  const pid = project.json('data.id');
  const base = `${BASE_URL}/api/v1/projects/${pid}/environments/production`;
  for (const [body, step] of [
    [{ name: 'receiver', url: `${RECEIVER_URL}/hook/${pid}` }, 'w'],
    [{ kind: 'cdn', name: 'cdn', url: `${RECEIVER_URL}/purge` }, 'c'],
    [{ kind: 'probe', name: 'site', url: `${RECEIVER_URL}/site/${pid}` }, 's'],
  ]) {
    const made = http.post(`${base}/destinations`, JSON.stringify(body), params({ 'Idempotency-Key': `k6-l13-${run}-${step}` }));
    if (made.status !== 201) return false;
  }
  mineBase = base;
  return true;
}

function publishInto(base, generation, headers, tags, run) {
  const params = (endpoint, extra) => ({
    headers: Object.assign({}, headers, extra || {}), redirects: 0, tags: Object.assign({ name: endpoint, endpoint }, tags),
  });
  const idem = (s) => `k6-l13-${run}-${s}`;
  const entry = http.post(`${base}/collections/articles/entries`, JSON.stringify(article(`Publish ${__ITER}`)), params('entry', { 'Idempotency-Key': idem('e') }));
  if (entry.status !== 201) return { published: false };
  const set = http.post(`${base}/changesets`, JSON.stringify({ title: `k6 ${__ITER}`, entries: [entry.json('data.id')] }), params('changeset', { 'Idempotency-Key': idem('c') }));
  if (set.status !== 201) return { published: false };
  const path = `${base}/changesets/${set.json('data.id')}`;
  const ifMatch = { 'If-Match': `"${set.json('data.hash')}"` };
  if (http.post(`${path}/submit`, null, params('submit', ifMatch)).status !== 200) return { published: false };
  if (http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), params('review', ifMatch)).status !== 200) return { published: false };
  const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: generation }), params('publish', Object.assign({ 'Idempotency-Key': idem('p') }, ifMatch)));
  return { published: published.status === 201, generation: published.status === 201 ? published.json('data.generation') : generation };
}

/** One publish a second per VU on its own project, through every window — the backlog
 *  the worker drains after the heal. */
export function publish(data) {
  const tags = { phase: phase() };
  const headers = {
    Cookie: data.owner.Cookie, 'X-CSRF-Token': data.owner['X-CSRF-Token'], Origin: PUBLIC_URL, 'Content-Type': 'application/json',
  };
  if (!ensureProject(data, headers, tags)) {
    publishCounters[`${tags.phase}_failed`].add(1);
    check(null, { 'publish project exists': () => false });
    return;
  }
  const done = publishInto(mineBase, mineGeneration, headers, tags, `${data.run}-${exec.vu.idInTest}-${__ITER}`);
  publishCounters[`${tags.phase}_${done.published ? 'ok' : 'failed'}`].add(1);
  check(null, { 'publish committed': () => done.published });
  if (done.published) mineGeneration = done.generation;
}

// The audit arm's high-water marks: an accepted purge or a verified label can only be
// earned before the fault — while the sink is down they must not grow.
let acceptedHighWater = 0;
let verifiedHighWater = 0;

/** One sample of the audited project's releases and jobs: nothing may report a success
 *  the dead sink could not have given. */
export function audit(data) {
  const tags = { phase: phase() };
  const headers = { Cookie: data.owner.Cookie };
  const releases = http.get(`${data.base}/releases`, { headers, redirects: 0, tags: Object.assign({ name: 'releases' }, tags) });
  check(releases, { 'releases 200': (r) => r.status === 200 });
  if (releases.status === 200) {
    let accepted = 0;
    let verified = 0;
    for (const release of releases.json('data')) {
      if (release.purge === 'accepted' && release.generation > accepted) accepted = release.generation;
      if (release.label === 'verified' && release.generation > verified) verified = release.generation;
    }
    if (tags.phase === 'outage') {
      if (accepted > acceptedHighWater) falsePurge.add(1);
      if (verified > verifiedHighWater) falseVerified.add(1);
    } else {
      if (accepted > acceptedHighWater) acceptedHighWater = accepted;
      if (verified > verifiedHighWater) verifiedHighWater = verified;
    }
  }
  const jobs = http.get(`${data.base}/jobs`, { headers, redirects: 0, tags: Object.assign({ name: 'jobs' }, tags) });
  check(jobs, { 'jobs 200': (r) => r.status === 200 });
  if (jobs.status === 200) {
    // Per-kind peaks: the 'site' destination is the probe (bound 10); posts bound at 5.
    let mostPost = 0;
    let mostProbe = 0;
    for (const job of jobs.json('data')) {
      if (job.destination_name === 'site') mostProbe = Math.max(mostProbe, job.attempts);
      else mostPost = Math.max(mostPost, job.attempts);
    }
    if (mostPost > 0) maxAttemptsPost.add(mostPost);
    if (mostProbe > 0) maxAttemptsProbe.add(mostProbe);
  }
}

/** The receiver's attempt log, sampled once after the heal: per-key gaps between
 *  purge attempts prove the backoff is honored rather than a hot loop. */
export function stats() {
  const res = http.get(`${RECEIVER_URL}/stats`, { tags: { name: 'receiver-stats', phase: 'recovery' } });
  if (!check(res, { 'stats 200': (r) => r.status === 200 })) return;
  const attempts = res.json('purge_attempts') || [];
  let failed = 0;
  for (const attempt of attempts) {
    if (attempt.answered >= 400) failed += 1;
  }
  purgeFailedAttempts.add(failed);
  // The smallest gap between consecutive attempts of the same job key — under the
  // asked-for wait would mean the retry loop ignored Retry-After.
  const ordered = {};
  for (const attempt of attempts) {
    (ordered[attempt.key] = ordered[attempt.key] || []).push(attempt.at);
  }
  let smallest = null;
  for (const times of Object.values(ordered)) {
    times.sort((a, b) => a - b);
    for (let i = 1; i < times.length; i += 1) {
      const gap = times[i] - times[i - 1];
      if (smallest === null || gap < smallest) smallest = gap;
    }
  }
  if (smallest !== null) purgeGap.add(smallest);
}

/** The deterministic leg: a publish made while the sink is down must read pending or
 *  needs_attention — never accepted or verified — and drain after the heal. */
export function sentinel(data) {
  const headers = {
    Cookie: data.owner.Cookie, 'X-CSRF-Token': data.owner['X-CSRF-Token'], Origin: PUBLIC_URL, 'Content-Type': 'application/json',
  };
  const tags = { phase: 'outage', name: 'sentinel' };
  // A release committed inside the outage window: it cannot possibly be delivered.
  const outage = publishInto(data.base, 0, headers, tags, `${data.run}-sentinel-o`);
  if (!check(null, { 'sentinel outage publish committed': () => outage.published })) {
    sentinelStages.outage_publish_failed.add(1);
    return;
  }
  sentinelStages.outage_published.add(1);
  // Still inside the outage: the release must say pending (or needs_attention), never
  // a success the dead sink didn't give.
  sleep(Math.max(0, PHASE - 8));
  const mid = http.get(`${data.base}/releases`, { headers, tags: { name: 'releases', phase: 'outage' } });
  const mine = mid.status === 200 && mid.json('data').find((r) => r.generation === outage.generation);
  const honest = mine && mine.purge !== 'accepted' && (mine.label === 'pending' || mine.label === 'needs_attention' || mine.label === 'published');
  check(null, { 'outage release is pending/uncertain, not success': () => honest });
  sentinelStages[honest ? 'pending_while_down' : 'false_early'].add(1);
  // Past the heal: publish once more, then wait for both releases to verify and both
  // purges to be accepted — the backlog drains.
  sleep(GAP * 4);
  const healed = publishInto(data.base, outage.generation, headers, { phase: 'recovery', name: 'sentinel' }, `${data.run}-sentinel-r`);
  check(null, { 'sentinel recovery publish committed': () => healed.published });
  const deadline = Date.now() + 90000;
  let drained = false;
  while (Date.now() < deadline && !drained) {
    const releases = http.get(`${data.base}/releases`, { headers, tags: { name: 'releases', phase: 'recovery' } });
    if (releases.status === 200) {
      const list = releases.json('data');
      const covered = (g) => list.find((r) => r.generation === g);
      const o = covered(outage.generation);
      const r = healed.published ? covered(healed.generation) : null;
      if (o && o.label === 'verified' && o.purge === 'accepted' && (!healed.published || (r && r.label === 'verified' && r.purge === 'accepted'))) drained = true;
    }
    if (!drained) sleep(2);
  }
  sentinelStages[drained ? 'recovered' : 'stuck'].add(1);
}

/** The fault injector's clock: fail the purge sink and freeze the site's generation at
 *  the phase boundary; heal both inside the second margin. */
export function conductor() {
  const call = (body) => {
    const res = http.post(`${RECEIVER_URL}/control`, JSON.stringify(body), {
      headers: { 'Content-Type': 'application/json' }, tags: { name: 'control', phase: 'margin' },
    });
    if (!check(res, { 'control acknowledged': (r) => r.status === 200 })) controlErrors.add(1);
  };
  call({ fault: 'purge', action: 'down', status: 503, retry_after: RETRY_AFTER });
  call({ fault: 'site', action: 'down' });
  sleep(PHASE + GAP * 2);
  call({ fault: 'purge', action: 'up' });
  call({ fault: 'site', action: 'up' });
}

export function teardown(data) {
  // Every project's releases must end verified at one location with an accepted purge —
  // the same end-state publishing.js requires (D-050, D-051).
  const headers = { Cookie: data.owner.Cookie };
  const projects = http.get(`${BASE_URL}/api/v1/projects?limit=100`, { headers });
  const bases = [data.base];
  if (projects.status === 200) {
    for (const project of projects.json('data')) {
      if (project.slug.startsWith(`k6-${data.run}-`)) bases.push(`${BASE_URL}/api/v1/projects/${project.id}/environments/production`);
    }
  }
  const deadline = Date.now() + 90000;
  for (const base of bases) {
    for (;;) {
      const listed = http.get(`${base}/releases`, { headers }).json('data');
      const unpurged = listed.filter((release) => release.purge !== 'accepted');
      const unverified = listed.filter((release) => release.label !== 'verified' || release.locations !== 1);
      if (unpurged.length === 0 && unverified.length === 0) break;
      if (Date.now() > deadline) fail(`releases of ${base} never told the truth after the heal: unpurged ${JSON.stringify(unpurged.slice(0, 2))} unverified ${JSON.stringify(unverified.slice(0, 2))}`);
      sleep(1);
    }
  }
}

const count = (data, name) => (data.metrics[name] ? data.metrics[name].values.count : 0);

/** Computes the L13 verdict and attaches it to the exported summary as `l13`. */
export function handleSummary(data) {
  const publishCount = (phaseName, cls) => count(data, `l13_publishes_${phaseName}_${cls}`);
  const publishesFailed = PHASES.reduce((sum, ph) => sum + publishCount(ph, 'failed'), 0);
  const sentinelStage = (stage) => count(data, `l13_sentinel_${stage}`);
  const postMetric = data.metrics.l13_job_max_attempts_post;
  const postAttempts = postMetric ? postMetric.values.max || postMetric.values.value || 0 : 0;
  const probeMetric = data.metrics.l13_job_max_attempts_probe;
  const probeAttempts = probeMetric ? probeMetric.values.max || probeMetric.values.value || 0 : 0;
  const gapMetric = data.metrics.l13_purge_min_gap_ms;
  const minGap = gapMetric ? gapMetric.values.value : null;
  const failedMetric = data.metrics.l13_purge_failed_attempts;
  const failedAttempts = failedMetric ? failedMetric.values.value || 0 : 0;
  const gapBound = RETRY_AFTER * 800; // honored Retry-After, with slack for clock and the 500 ms claim poll

  const gates = {
    publishes_ran: publishCount('outage', 'ok') > 0 && publishCount('recovery', 'ok') > 0,
    // While the sink was dead, no release's accepted-purge or verified label grew:
    // nothing claimed a global success the outage made impossible.
    no_false_purge: count(data, 'l13_false_purge') === 0,
    no_false_verified: count(data, 'l13_false_verified') === 0,
    // The sentinel's own outage release was seen pending (or needs_attention), and it
    // drained to verified + accepted after the heal.
    pending_then_recovered: sentinelStage('pending_while_down') === 1 && sentinelStage('false_early') === 0 && sentinelStage('recovered') === 1 && sentinelStage('stuck') === 0,
    // Bounded work, not a hot loop: retries respect the asked-for wait between a job's
    // attempts, and no job retries past its kind's bound — a post (webhook or purge)
    // stops at MAX_ATTEMPTS, a probe at MAX_PROBE_ATTEMPTS (job.rs).
    backoff_honored: failedAttempts > 0 && minGap != null && minGap >= gapBound,
    retries_bounded: postAttempts > 0 && probeAttempts > 0 && postAttempts <= MAX_ATTEMPTS_BOUND && probeAttempts <= MAX_PROBE_ATTEMPTS_BOUND,
    controlled: count(data, 'l13_control_errors') === 0,
  };
  data.l13 = {
    spec: 'L13: CDN purge 429/5xx and a delayed site rebuild — bounded retries/backoff, pending/uncertain while failing, no claim of global success, truthful recovery',
    gate: `zero accepted-purge/verified growth during the outage; sentinel release pending then recovered; per-job retry gaps >= ${gapBound} ms; post job attempts <= ${MAX_ATTEMPTS_BOUND}, probe attempts <= ${MAX_PROBE_ATTEMPTS_BOUND}`,
    windows_seconds: { phase: PHASE, margins: GAP, outage_spans: [PHASE + GAP, PHASE * 2 + GAP], injected: `receiver /purge -> 503 + Retry-After: ${RETRY_AFTER}s, /site frozen at ~${PHASE}s; healed at ~${PHASE * 2 + GAP * 2}s` },
    publishes: { baseline: publishCount('baseline', 'ok'), outage: publishCount('outage', 'ok'), recovery: publishCount('recovery', 'ok'), failed: publishesFailed },
    retry: { asked_seconds: RETRY_AFTER, failed_attempts: failedAttempts, min_per_job_gap_ms: minGap == null ? null : minGap, max_post_attempts: postAttempts, max_probe_attempts: probeAttempts },
    sentinel: { outage_published: sentinelStage('outage_published'), pending_while_down: sentinelStage('pending_while_down'), recovered: sentinelStage('recovered'), stuck: sentinelStage('stuck') },
    gates,
    held: Object.values(gates).every(Boolean),
  };
  return {
    stdout: [
      '',
      '  L13 — CDN purge 503 + delayed site rebuild, then healed',
      `    outage publishes: ${publishCount('outage', 'ok')} committed; false successes seen: ${count(data, 'l13_false_purge') + count(data, 'l13_false_verified')}`,
      `    retries: ${failedAttempts} failed purge attempts, smallest per-job gap ${minGap == null ? 'n/a' : `${minGap.toFixed(0)}ms`} (asked ${RETRY_AFTER * 1000}ms), max attempts post ${postAttempts} probe ${probeAttempts}`,
      `    sentinel: pending_while_down ${sentinelStage('pending_while_down')}, recovered ${sentinelStage('recovered')}`,
      `    verdict: ${data.l13.held ? 'HELD' : 'BROKE'}`,
      '',
    ].join('\n'),
  };
}
