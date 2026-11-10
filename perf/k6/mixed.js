// Load scenario for L05 (spec §29): mixed reads, writes and publishing, all at once — the
// case that asks whether each operation class keeps its own budget while the others run,
// and whether the content they fight over stays consistent.
//
// Three arms share one project: `reads` is anonymous /content/v1 traffic at RATE — the
// same mix warm-baseline walks (latest and pinned lists and entries); `writes` is signed-in
// draft work at WRITE_RATE, each virtual user owning one article: an If-Match save and a
// read-back that must return the revision it was just given; `publishes` lands a real
// release every PUBLISH_SECONDS — a new entry, a changeset, submit, approve, publish on
// the base the publisher just read — and then reads the entry back over the public API at
// the generation it minted. Two publishers racing the same base get the honest 409:
// whoever loses re-reads the generation and republishes, counted as a conflict, not hidden.
//
// "Content stays consistent" is measured, not assumed: every latest-generation read's
// `Colophon-Generation` must never step backwards for the VU that saw newer (`l05_stale`,
// a stale refill), and no row may carry `published_in` past the generation its page was
// served at (`l05_torn`, a mixed-release page). Saves must read back their own ETag; every
// publish must land generation +1 on a release id never seen; and the read following each
// publish must find the new entry published at exactly that generation.
//
// The arms are time-boxed, not iteration-counted: TOTAL and VUS are unused. `steady` and
// `throughput` run the same plan. handleSummary() computes the verdict into `l05`;
// scripts/load.sh stores it and fails when `held` is false — and samples server RSS, CPU
// and Postgres connections into `resources` beside it. scripts/load.sh gives this
// endpoint RATE 100 and P99_MS 700 — the editorial write budget, which binds every arm.

import http from 'k6/http';
import { check, fail } from 'k6';
import exec from 'k6/execution';
import { Counter } from 'k6/metrics';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const RATE = parseInt(__ENV.RATE || '100', 10);
const WRITE_RATE = parseFloat(__ENV.WRITE_RATE || `${Math.max(5, Math.round(RATE / 10))}`);
const PUBLISH_SECONDS = parseInt(__ENV.PUBLISH_SECONDS || '12', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '700');
const READ_P99_MS = parseFloat(__ENV.READ_P99_MS || '350'); // the warm gate for the read arm
const CORPUS = parseInt(__ENV.CORPUS || '400', 10);

const stale = new Counter('l05_stale');
const torn = new Counter('l05_torn');
const errors = new Counter('l05_errors');
// DRILL_WINDOWS announces that soak-sidecar.sh drills will fire during the run. k6 cannot
// read files after init, so the scenario cannot weigh failures against the windows
// itself: it emits the raw material — a sparse 10 s bucket series of every failed
// assertion — and load.sh splits it against the windows file when it builds the record.
// With DRILL_WINDOWS set the zero-error k6 thresholds below are dropped (a paused
// dependency must refuse load); the buckets are what the gate is actually made of.
const DRILL_WINDOWS = __ENV.DRILL_WINDOWS || '';
const BUCKET_S = 10;
const failBuckets = {};
function bucket() {
  const at = Math.floor(Date.now() / 1000 / BUCKET_S);
  failBuckets[at] = (failBuckets[at] || 0) + 1;
}
const readAnswers = new Counter('l05_reads');
const saves = new Counter('l05_saves');
const writeBacks = new Counter('l05_write_backs');
const publishOps = new Counter('l05_publishes');
const conflicts = new Counter('l05_conflicts');
const reauths = new Counter('l05_reauths');
const served = {
  hit: new Counter('l05_hit'),
  miss: new Counter('l05_miss'),
  coalesced: new Counter('l05_coalesced'),
  bypass: new Counter('l05_bypass'),
};

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

if (!(SCENARIO === 'throughput' || SCENARIO === 'steady')) {
  throw new Error('SCENARIO must be throughput or steady');
}

const thresholds = {
  l05_stale: ['count==0'],
  l05_torn: ['count==0'],
  l05_publishes: ['count>0'],
  l05_saves: ['count>0'],
  l05_write_backs: ['count>0'],
  http_req_duration: [`p(99)<${P99_MS}`],
  // Each operation class against its own budget: the read arm at the warm gate, the write
  // and publish requests inside the editorial write budget.
  'http_req_duration{arm:read}': [`p(99)<${READ_P99_MS}`],
  'http_req_duration{arm:write}': [`p(95)<${P99_MS}`],
  'http_req_duration{arm:publish}': [`p(99)<${P99_MS}`],
  'http_reqs{arm:read}': ['count>0'],
  'http_reqs{arm:write}': ['count>0'],
  'http_reqs{arm:publish}': ['count>0'],
};
if (DRILL_WINDOWS === '') {
  // No drills announced: the strict contract — every check passes, nothing fails.
  thresholds.checks = ['rate==1'];
  thresholds.http_req_failed = ['rate==0'];
  thresholds.l05_errors = ['count==0'];
}
if (SCENARIO === 'steady') {
  const scheduled = RATE * durationSeconds(DURATION) + WRITE_RATE * durationSeconds(DURATION);
  thresholds.dropped_iterations = [`count<=${Math.max(1, Math.floor(scheduled * 0.0001))}`];
}

export const options = {
  scenarios: {
    reads: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: 32, maxVUs: 64, exec: 'reads' },
    writes: { executor: 'constant-arrival-rate', rate: WRITE_RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: 16, maxVUs: 32, exec: 'writes' },
    publishes: { executor: 'constant-arrival-rate', rate: 1, timeUnit: `${PUBLISH_SECONDS}s`, duration: DURATION, preAllocatedVUs: 2, maxVUs: 4, exec: 'publishes' },
  },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '300s',
};

const article = (title, text) => JSON.stringify({
  title,
  body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] } },
});

function key() {
  return `k6-l05-${__VU}-${__ITER}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

// A session lives 12 hours absolute however busy (SessionSettings::default), so a 24 h soak
// must re-authenticate at the boundary instead of collecting 401s — the 2026-11-09 run's
// entire second half was exactly that. Each VU keeps its own credential snapshot: `data` is
// per-VU already, and a re-login here signs in as the same dev-owner the setup() session did.
let creds = null;
function session(data) {
  if (creds === null) creds = { cookie: data.session.cookie, csrf: data.session.csrf };
  return creds;
}
function reauth(data) {
  reauths.add(1);
  const res = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }), {
    headers: { 'Content-Type': 'application/json' }, redirects: 0,
  });
  if (res.status !== 204) return false;
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  creds = { cookie: `colophon_session=${jar.colophon_session[0]}`, csrf: jar.colophon_csrf[0] };
  return true;
}
// Sends fn(); a 401 means the session died — re-authenticate and send once more with the
// fresh credential. Headers are built inside fn() so the retry carries the new cookie.
// A 401 never reached the handler, so the retried Idempotency-Key may safely differ.
// Every authed call expects its success statuses plus 401: the dead-session answer is
// handled by the retry, and only a 401 that survives it reaches the checks — counting
// the handled one in http_req_failed would fail the run on a recovered event.
const expected = (...statuses) => http.expectedStatuses(...statuses, 401);
function authed(data, fn) {
  let res = fn();
  if (res.status === 401 && reauth(data)) res = fn();
  return res;
}
// check() plus drill bookkeeping: every assertion the arms make lands in a 10 s bucket
// when it fails, so handleSummary can split failures inside a drill window from the
// ones that must never happen. k6's own checks/http_req_failed metrics are untouched.
function verify(res, checks) {
  const held = check(res, checks);
  if (!held) bucket();
  return held;
}

export function setup() {
  const signedIn = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }), { headers: { 'Content-Type': 'application/json' } });
  if (signedIn.status !== 204) fail(`sign-in failed: HTTP ${signedIn.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const session = { cookie: `colophon_session=${jar.colophon_session[0]}`, csrf: jar.colophon_csrf[0] };
  const write = { Cookie: session.cookie, 'X-CSRF-Token': session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };

  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 mixed' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const id = project.json('data.id');
  const env = `${BASE_URL}/api/v1/projects/${id}/environments/production`;
  const entries = `${env}/collections/articles/entries`;

  // The corpus: CORPUS real articles, imported a 512-item chunk at a time and published
  // in 100-entry releases — the changeset cap (the same shape cold-cache seeds).
  let file = '';
  const ids = [];
  for (let i = 0; i < CORPUS; i += 1) {
    file += JSON.stringify({ title: `Article ${i}`, body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'first' }] }] } } }) + '\n';
    if ((i + 1) % 512 === 0 || i === CORPUS - 1) {
      const imported = http.post(`${env}/collections/articles/import`, file, {
        headers: Object.assign({}, write, { 'Content-Type': 'application/jsonlines', 'Idempotency-Key': `k6-seed-${slug}-${i}` }),
      });
      if (imported.status !== 201) fail(`seed import: HTTP ${imported.status} ${imported.body}`);
      for (const outcome of imported.json('data.outcomes')) ids.push(outcome.entry);
      file = '';
    }
  }
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
  return {
    session, env, entries, ids,
    delivery: `${BASE_URL}/content/v1/${id}/production/articles`,
    generation,
  };
}

// A read arm's own truth: the newest generation this VU has been served — a step back is a
// stale refill. `account` also counts where the answer came from and looks for torn pages.
const seen = { g: 0 };
function account(res, params) {
  const how = res.headers['Colophon-Cache'];
  if (how in served) served[how].add(1);
  const generation = parseInt(res.headers['Colophon-Generation'] || '-1', 10);
  if (!params.pinned) {
    if (generation >= 0 && generation < seen.g) stale.add(1);
    if (generation > seen.g) seen.g = generation;
  }
  if (res.status === 200 && generation >= 0) {
    const rows = res.json('data');
    const list = Array.isArray(rows) ? rows : [rows];
    for (const row of list) {
      if (row && typeof row.published_in === 'number' && row.published_in > generation) { torn.add(1); break; }
    }
  }
  return generation;
}

export function reads(data) {
  const params = (endpoint) => ({ redirects: 0, tags: { arm: 'read', endpoint } });
  const slot = __ITER % 10;
  let res; let pinned = false;
  if (slot < 4) {
    res = http.get(`${data.delivery}?limit=50`, params('latest'));
  } else if (slot < 6) {
    pinned = true;
    const generation = 1 + (__ITER % data.generation);
    res = http.get(`${data.delivery}?limit=50&generation=${generation}`, params('pinned'));
    verify(res, {
      'pinned list answers at the release asked for': (r) => r.status === 200 && r.headers['Colophon-Generation'] === String(generation),
    });
  } else if (slot < 8) {
    res = http.get(`${data.delivery}/${data.ids[__ITER % data.ids.length]}`, params('entry'));
  } else {
    pinned = true;
    res = http.get(`${data.delivery}/${data.ids[__ITER % data.ids.length]}?generation=1`, params('pinned_entry'));
    verify(res, {
      'pinned entry answers at release 1': (r) => r.status === 200 && r.headers['Colophon-Generation'] === '1',
    });
  }
  verify(res, { 'read arm 200': (r) => r.status === 200 });
  if (res.status === 200) readAnswers.add(1);
  account(res, { pinned });
}

// A write VU's own truth: the ETag of the revision it last saved — read-your-writes.
let etag = null;
export function writes(data) {
  const mine = data.ids[exec.vu.idInTest % data.ids.length];
  const url = `${data.entries}/${mine}`;
  const read = () => ({ Cookie: session(data).cookie });
  const write = () => ({ Cookie: session(data).cookie, 'X-CSRF-Token': session(data).csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' });
  if (etag === null) {
    const current = authed(data, () => http.get(url, { headers: read(), redirects: 0, responseCallback: expected(200), tags: { arm: 'write', endpoint: 'read', name: 'entries/{entry}' } }));
    if (!verify(current, { 'write-arm read 200': (r) => r.status === 200 })) { errors.add(1); return; }
    etag = current.headers['Etag'];
  }
  const text = `iteration ${__ITER}`;
  const saved = authed(data, () => http.put(url, article(`Article for VU ${exec.vu.idInTest}`, text), {
    headers: Object.assign({ 'If-Match': etag, 'Idempotency-Key': key() }, write()),
    redirects: 0, responseCallback: expected(200), tags: { arm: 'write', endpoint: 'save', name: 'entries/{entry}' },
  }));
  if (!verify(saved, {
    'save 200': (r) => r.status === 200,
    'save gives a new ETag': (r) => typeof r.headers['Etag'] === 'string' && r.headers['Etag'] !== etag,
  })) { errors.add(1); etag = null; return; }
  saves.add(1);
  etag = saved.headers['Etag'];
  // The consistency check: what the save answered must be what a read returns next.
  const back = authed(data, () => http.get(url, { headers: read(), redirects: 0, responseCallback: expected(200), tags: { arm: 'write', endpoint: 'write-back', name: 'entries/{entry}' } }));
  if (!verify(back, {
    'read-back 200': (r) => r.status === 200,
    'read-back returns the revision just saved': (r) => r.headers['Etag'] === etag,
  })) errors.add(1);
  else writeBacks.add(1);
}

// A publish VU's own truth: the generation it believes the environment is at, and the
// release ids it has already seen — it learns the base from a delivery read before it
// publishes, so two publishers colliding get the honest 409 and a re-read, not a pass.
export function publishes(data) {
  const write = () => ({ Cookie: session(data).cookie, 'X-CSRF-Token': session(data).csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' });
  const latest = http.get(`${data.delivery}?limit=1`, { redirects: 0, tags: { arm: 'publish', endpoint: 'base', name: 'content' } });
  if (!verify(latest, { 'base read 200': (r) => r.status === 200 })) { errors.add(1); return; }
  let base = parseInt(latest.headers['Colophon-Generation'] || '-1', 10);
  const made = authed(data, () => http.post(data.entries, article(`Published ${key()}`, 'new'), {
    headers: Object.assign({ 'Idempotency-Key': key() }, write()),
    redirects: 0, responseCallback: expected(201), tags: { arm: 'publish', endpoint: 'entry', name: 'entries' },
  }));
  if (!verify(made, { 'publish-arm entry 201': (r) => r.status === 201 })) { errors.add(1); return; }
  const id = made.json('data.id');
  const set = authed(data, () => http.post(`${data.env}/changesets`, JSON.stringify({ title: `mixed ${key()}`, entries: [id] }), {
    headers: Object.assign({ 'Idempotency-Key': key() }, write()), redirects: 0, responseCallback: expected(201), tags: { arm: 'publish', endpoint: 'changeset', name: 'changesets' },
  }));
  if (!verify(set, { 'changeset 201': (r) => r.status === 201 })) { errors.add(1); return; }
  const path = `${data.env}/changesets/${set.json('data.id')}`;
  const ifMatch = () => Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, write());
  const submitted = authed(data, () => http.post(`${path}/submit`, null, { headers: ifMatch(), redirects: 0, responseCallback: expected(200), tags: { arm: 'publish', endpoint: 'submit', name: 'changesets/{changeset}/submit' } }));
  if (!verify(submitted, { 'submit 200': (r) => r.status === 200 })) { errors.add(1); return; }
  const approved = authed(data, () => http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch(), redirects: 0, responseCallback: expected(200), tags: { arm: 'publish', endpoint: 'review', name: 'changesets/{changeset}/reviews' } }));
  if (!verify(approved, { 'review 200': (r) => r.status === 200 })) { errors.add(1); return; }
  let published = authed(data, () => http.post(`${path}/publish`, JSON.stringify({ base_generation: base }), {
    headers: Object.assign({ 'Idempotency-Key': key() }, ifMatch()), redirects: 0,
    responseCallback: expected(201, 409),
    tags: { arm: 'publish', endpoint: 'publish', name: 'changesets/{changeset}/publish' },
  }));
  if (published.status === 409) {
    // The honest answer to a lost race: re-read the generation and republish once.
    conflicts.add(1);
    const again = http.get(`${data.delivery}?limit=1`, { redirects: 0, tags: { arm: 'publish', endpoint: 'base', name: 'content' } });
    base = parseInt(again.headers['Colophon-Generation'] || '-1', 10);
    published = authed(data, () => http.post(`${path}/publish`, JSON.stringify({ base_generation: base }), {
      headers: Object.assign({ 'Idempotency-Key': key() }, ifMatch()), redirects: 0,
      responseCallback: expected(201, 409),
      tags: { arm: 'publish', endpoint: 'publish', name: 'changesets/{changeset}/publish' },
    }));
  }
  if (!verify(published, {
    'publish 201': (r) => r.status === 201,
    'one generation forward': (r) => r.json('data.generation') === base + 1,
  })) { errors.add(1); return; }
  publishOps.add(1);
  const generation = published.json('data.generation');
  // The consistency check: the entry it just published reads back at its own release.
  const back = http.get(`${data.delivery}/${id}`, { redirects: 0, tags: { arm: 'publish', endpoint: 'published-read', name: 'content/{entry}' } });
  if (!verify(back, {
    'published read 200': (r) => r.status === 200,
    'served at the generation it minted': (r) => r.headers['Colophon-Generation'] === String(generation),
    'marked published in it': (r) => r.json('data.published_in') === generation,
  })) errors.add(1);
}

/** Computes the L05 verdict: every class ran inside its own budget, releases climbed by
 *  one, and the consistency counters — stale refills, torn pages, lost saves — stayed at
 *  zero while all three arms were running. `no_errors` here is the strict reading —
 *  zero failed assertions anywhere. When DRILL_WINDOWS announced sidecar drills,
 *  load.sh cannot be consulted from k6 (open() is init-stage only), so it re-weighs
 *  `error_buckets` against the windows file and rewrites this gate before recording:
 *  the stored verdict then means "no failures outside the announced windows". */
export function handleSummary(data) {
  const metric = (name) => (data.metrics[name] ? data.metrics[name].values : null);
  const count = (name) => (metric(name) ? metric(name).count || 0 : 0);
  const arm = (which) => metric(`http_req_duration{arm:${which}}`) || {};
  const readArm = arm('read'); const writeArm = arm('write'); const publishArm = arm('publish');
  const bucketRows = [];
  let bucketTotal = 0;
  for (const key of Object.keys(failBuckets)) {
    bucketRows.push([parseInt(key, 10) * BUCKET_S, failBuckets[key]]);
    bucketTotal += failBuckets[key];
  }
  bucketRows.sort((a, b) => a[0] - b[0]);
  const drillsAnnounced = DRILL_WINDOWS !== '';
  const gates = {
    reads_held: (readArm['p(99)'] || Infinity) < READ_P99_MS && count('l05_reads') > 0,
    writes_held: (writeArm['p(95)'] || Infinity) < P99_MS && count('l05_saves') > 0 && count('l05_write_backs') > 0,
    publishes_held: (publishArm['p(99)'] || Infinity) < P99_MS && count('l05_publishes') > 0,
    no_stale_refills: count('l05_stale') === 0,
    no_torn_pages: count('l05_torn') === 0,
    // Every failed assertion, wherever it landed — the strict reading. With drills
    // announced, load.sh replaces this with "none outside the windows" before gating.
    no_errors: bucketTotal === 0,
  };
  data.l05 = {
    spec: 'L05: mixed reads, writes and publishing — each operation class meets its target; content stays consistent',
    gate: `read arm p99 <${READ_P99_MS} ms (warm gate), write arm p95 <${P99_MS} ms, publish requests p99 <${P99_MS} ms — all with reads, saves, read-backs and publishes actually counted; zero stale refills, torn pages or failed assertions` +
      (drillsAnnounced ? ' — drills announced: load.sh re-weighs error_buckets against the windows and requires zero failures outside them' : ''),
    requests: {
      reads: count('l05_reads'),
      saves: count('l05_saves'),
      write_backs: count('l05_write_backs'),
      publishes: count('l05_publishes'),
      publish_conflicts_retried: count('l05_conflicts'),
      session_reauths: count('l05_reauths'),
    },
    latency_ms: {
      read: { p50: readArm.med, p95: readArm['p(95)'], p99: readArm['p(99)'] },
      write: { p50: writeArm.med, p95: writeArm['p(95)'], p99: writeArm['p(99)'] },
      publish: { p50: publishArm.med, p95: publishArm['p(95)'], p99: publishArm['p(99)'] },
    },
    consistency: {
      stale_refills: count('l05_stale'),
      torn_pages: count('l05_torn'),
      errors: count('l05_errors'),
    },
    // Sparse [epoch_s, failures] series over every failed assertion — the material
    // load.sh splits against the drill windows. Emitted whether or not drills ran.
    error_buckets: bucketRows,
    drills_announced: drillsAnnounced,
    // Filled by load.sh from the windows file; null here because k6 cannot re-read it.
    drills: null,
    served: { hit: count('l05_hit'), miss: count('l05_miss'), coalesced: count('l05_coalesced'), bypass: count('l05_bypass') },
    gates,
    held: Object.values(gates).every(Boolean),
  };
  return {
    stdout: [
      '',
      '  L05 — mixed reads, writes and publishing',
      `    ${count('l05_reads')} reads (p99 ${(readArm['p(99)'] || 0).toFixed(1)} ms, gate ${READ_P99_MS}), ${count('l05_saves')} saves + ${count('l05_write_backs')} verified read-backs (p95 ${(writeArm['p(95)'] || 0).toFixed(1)} ms), ${count('l05_publishes')} publishes (${count('l05_conflicts')} conflicts retried, ${count('l05_reauths')} session re-auths, p99 ${(publishArm['p(99)'] || 0).toFixed(1)} ms)`,
      `    stale ${count('l05_stale')}, torn ${count('l05_torn')}, errors ${count('l05_errors')}${drillsAnnounced ? ' (pre-window; load.sh re-weighs against drill windows)' : ''}`,
      `    verdict: ${data.l05.held ? 'HELD' : 'BROKE'}${drillsAnnounced ? ' — provisional until load.sh applies the drill windows' : ''}`,
      '',
    ].join('\n'),
  };
}
