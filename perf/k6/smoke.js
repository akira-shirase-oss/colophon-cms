// Load scenario for L01 (spec §29): the one-user smoke — no load before basic correctness is
// proven. ONE virtual user for DURATION (five minutes in the evidence run) walks the whole
// loop a user walks: read an article, save a revision and read that exact revision back,
// publish the pool and read the release it lands (its id is kept — every release must get a
// new one), list and search, read the public delivery at the release it just made and at the
// first one, and send the requests a draft API must refuse — an anonymous read and a write
// with no sign-in at all. Every answer is checked for shape and content, not just status.
//
// The arms are not rate-scheduled: constant-vus holds a single user who paces themselves with
// a beat between iterations — TOTAL, VUS and RATE are all unused; DURATION is the run.
// `steady` and `throughput` are the same walk. handleSummary() counts what the walk proved
// into `l01`; scripts/load.sh stores it and fails when `held` is false.
// scripts/load.sh gives this endpoint P99_MS 700 (the editorial budget — publishes are
// several requests long).

import http from 'k6/http';
import { check, fail, sleep } from 'k6';
import { Counter } from 'k6/metrics';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '700');
const ARTICLES = 4;

if (!(SCENARIO === 'throughput' || SCENARIO === 'steady')) {
  throw new Error('SCENARIO must be throughput or steady');
}

// Every verified step lands in a counter, so the verdict names what walked, not just "no
// failures": a smoke that did nothing would be a quiet pass without them.
const reads = new Counter('l01_reads');
const saves = new Counter('l01_saves');
const revisionReads = new Counter('l01_revision_reads');
const publishes = new Counter('l01_publishes');
const deliveryReads = new Counter('l01_delivery_reads');
const refusals = new Counter('l01_refusals');
const bad = new Counter('l01_bad');

const thresholds = {
  checks: ['rate==1'],
  http_req_failed: ['rate==0'],
  l01_bad: ['count==0'],
  l01_publishes: ['count>0'],
  http_req_duration: [`p(99)<${P99_MS}`],
};

export const options = {
  scenarios: {
    walk: { executor: 'constant-vus', vus: 1, duration: DURATION, exec: 'walk' },
  },
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
  return `k6-smoke-${__ITER}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export function setup() {
  const signedIn = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }), { headers: { 'Content-Type': 'application/json' } });
  if (signedIn.status !== 204) fail(`sign-in failed: HTTP ${signedIn.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const session = { cookie: `colophon_session=${jar.colophon_session[0]}`, csrf: jar.colophon_csrf[0] };
  const write = { Cookie: session.cookie, 'X-CSRF-Token': session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };

  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 smoke' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const id = project.json('data.id');
  const env = `${BASE_URL}/api/v1/projects/${id}/environments/production`;
  const entries = `${env}/collections/articles/entries`;

  const articles = [];
  for (let i = 0; i < ARTICLES; i += 1) {
    const created = http.post(entries, article(`Smoke ${i}`, 'first'), {
      headers: Object.assign({ 'Idempotency-Key': `k6-smoke-${slug}-${i}` }, write),
    });
    if (created.status !== 201) fail(`article ${i}: HTTP ${created.status} ${created.body}`);
    articles.push({ id: created.json('data.id'), etag: created.headers['Etag'], text: 'first' });
  }

  // Release 1 is published here, before any save touches the pool, so a pinned read at
  // generation=1 deterministically serves the 'first' revision the walk proves later.
  const set = http.post(`${env}/changesets`, JSON.stringify({ title: 'smoke release 1', entries: articles.map((a) => a.id) }), {
    headers: Object.assign({ 'Idempotency-Key': `k6-smoke-${slug}-r1` }, write),
  });
  if (set.status !== 201) fail(`changeset: HTTP ${set.status} ${set.body}`);
  const path = `${env}/changesets/${set.json('data.id')}`;
  const ifMatch = Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, write);
  const submitted = http.post(`${path}/submit`, null, { headers: ifMatch });
  if (submitted.status !== 200) fail(`submit: HTTP ${submitted.status} ${submitted.body}`);
  const approved = http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch });
  if (approved.status !== 200) fail(`review: HTTP ${approved.status} ${approved.body}`);
  const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: 0 }), {
    headers: Object.assign({ 'Idempotency-Key': `k6-smoke-${slug}-p1` }, ifMatch),
  });
  if (published.status !== 201 || published.json('data.generation') !== 1) {
    fail(`release 1: HTTP ${published.status} ${published.body}`);
  }
  return {
    session, env, entries, articles,
    delivery: `${BASE_URL}/content/v1/${id}/production/articles`,
    generation: published.json('data.generation'),
  };
}

// The walk's own bookkeeping: one user's truth about what it wrote and published. The
// generation starts where setup's release 1 left it.
let generation = -1;
let releaseIds = new Set();

export function walk(data) {
  if (generation === -1) generation = data.generation;
  const write = { Cookie: data.session.cookie, 'X-CSRF-Token': data.session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };
  const read = { Cookie: data.session.cookie };
  const article_ = data.articles[__ITER % ARTICLES];
  const url = `${data.entries}/${article_.id}`;
  const turn = __ITER % 10;

  // Every turn begins by reading the article — the revision this user last saved, or the
  // walk is broken.
  const res = http.get(url, { headers: read, redirects: 0, tags: { endpoint: 'read', name: 'entries/{entry}' } });
  if (!check(res, {
    'read 200': (r) => r.status === 200,
    'read returns the last accepted save': (r) => r.headers['Etag'] === article_.etag,
  })) { bad.add(1); console.log(`read broke: ${res.status} etag=${res.headers['Etag']} wanted ${article_.etag}`); return; }
  reads.add(1);

  if (turn < 3) {
    // A save: the new revision must come back by ETag and by id.
    const text = `turn ${__ITER}`;
    const saved = http.put(url, article(`Smoke ${__ITER % ARTICLES}`, text), {
      headers: Object.assign({ 'If-Match': article_.etag, 'Idempotency-Key': key() }, write),
      redirects: 0, tags: { endpoint: 'save', name: 'entries/{entry}' },
    });
    if (!check(saved, {
      'save 200': (r) => r.status === 200,
      'save gives a new ETag': (r) => typeof r.headers['Etag'] === 'string' && r.headers['Etag'] !== article_.etag,
    })) { bad.add(1); console.log(`save broke: ${saved.status} ${saved.body.slice(0, 200)}`); return; }
    article_.etag = saved.headers['Etag'];
    article_.text = text;
    saves.add(1);
    const revision = article_.etag.replace(/"/g, '');
    const rev = http.get(`${url}/revisions/${revision}`, { headers: read, redirects: 0, tags: { endpoint: 'revision', name: 'entries/{entry}/revisions/{revision}' } });
    if (!check(rev, {
      'revision 200': (r) => r.status === 200,
      'revision is the one asked for': (r) => r.json('data.revision.id') === revision,
    })) { bad.add(1); console.log(`revision broke: ${rev.status} ${rev.body.slice(0, 200)}`); return; }
    revisionReads.add(1);
  } else if (turn === 3) {
    // A publish of the whole pool on the generation the walk believes is current: it must
    // land exactly one ahead, on a release id never seen before.
    const set = http.post(`${data.env}/changesets`, JSON.stringify({ title: `smoke ${__ITER}`, entries: data.articles.map((a) => a.id) }), {
      headers: Object.assign({ 'Idempotency-Key': key() }, write), redirects: 0, tags: { endpoint: 'changeset', name: 'changesets' },
    });
    if (!check(set, { 'changeset 201': (r) => r.status === 201 })) { bad.add(1); console.log(`changeset broke: ${set.status} ${set.body.slice(0, 200)}`); return; }
    const path = `${data.env}/changesets/${set.json('data.id')}`;
    const ifMatch = Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, write);
    const submitted = http.post(`${path}/submit`, null, { headers: ifMatch, redirects: 0, tags: { endpoint: 'submit', name: 'changesets/{changeset}/submit' } });
    if (!check(submitted, { 'submit 200': (r) => r.status === 200 })) { bad.add(1); console.log(`submit broke: ${submitted.status} ${submitted.body.slice(0, 200)}`); return; }
    const approved = http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch, redirects: 0, tags: { endpoint: 'review', name: 'changesets/{changeset}/reviews' } });
    if (!check(approved, { 'approved': (r) => r.status === 200 })) { bad.add(1); console.log(`review broke: ${approved.status} ${approved.body.slice(0, 200)}`); return; }
    const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: generation }), {
      headers: Object.assign({ 'Idempotency-Key': key() }, ifMatch), redirects: 0, tags: { endpoint: 'publish', name: 'changesets/{changeset}/publish' },
    });
    if (!check(published, {
      'publish 201': (r) => r.status === 201,
      'the next generation': (r) => r.json('data.generation') === generation + 1,
      'a new release id': (r) => typeof r.json('data.changeset_id') === 'string' && !releaseIds.has(r.json('data.changeset_id')),
    })) { bad.add(1); console.log(`publish broke: ${published.status} ${published.body}`); return; }
    releaseIds.add(published.json('data.changeset_id'));
    generation = published.json('data.generation');
    publishes.add(1);
    // And the delivery must now serve it: the list at its own generation, and the pool's
    // first article pinned at generation 1 still serving the revision published then.
    const latest = http.get(data.delivery, { redirects: 0, tags: { endpoint: 'delivery', name: 'content' } });
    if (!check(latest, {
      'delivery 200': (r) => r.status === 200,
      'delivery serves the new generation': (r) => r.headers['Colophon-Generation'] === String(generation),
      'delivery lists the pool': (r) => r.json('data').length === ARTICLES,
    })) { bad.add(1); console.log(`delivery broke: ${latest.status} gen=${latest.headers['Colophon-Generation']} ${latest.body.slice(0, 200)}`); return; }
    deliveryReads.add(1);
  } else if (turn === 4 && generation > 0) {
    // The first release is history now: a pinned read must serve the revision published then.
    const pinned = http.get(`${data.delivery}/${data.articles[0].id}?generation=1`, { redirects: 0, tags: { endpoint: 'pinned', name: 'content/{entry}' } });
    if (!check(pinned, {
      'pinned 200': (r) => r.status === 200,
      'generation 1 serves the first revision': (r) => r.headers['Colophon-Generation'] === '1' && r.json('data.content.body.doc.content')[0].content[0].text === 'first',
    })) { bad.add(1); console.log(`pinned broke: ${pinned.status} ${pinned.body.slice(0, 200)}`); return; }
    deliveryReads.add(1);
  } else if (turn === 5) {
    // The drafts API without sign-in: refused, envelope-shaped. The 401s are the check's
    // evidence, not failures — expectedStatuses keeps them out of http_req_failed.
    const refused = http.expectedStatuses(401, 403);
    const anon = http.get(url, { redirects: 0, responseCallback: refused, tags: { endpoint: 'anon', name: 'entries/{entry}' } });
    if (!check(anon, {
      'anonymous read refused': (r) => r.status === 401,
      'refusal is an error envelope': (r) => typeof r.json('error.code') === 'string',
    })) { bad.add(1); console.log(`anon read: ${anon.status} ${anon.body.slice(0, 200)}`); return; }
    const writeAnon = http.put(url, article('Smoke', 'intruder'), { headers: { 'Content-Type': 'application/json', 'If-Match': article_.etag, 'Idempotency-Key': key() }, redirects: 0, responseCallback: refused, tags: { endpoint: 'anon-write', name: 'entries/{entry}' } });
    if (!check(writeAnon, { 'anonymous write refused': (r) => r.status === 401 || r.status === 403 })) { bad.add(1); console.log(`anon write: ${writeAnon.status}`); return; }
    refusals.add(1);
  } else {
    // The ordinary reads between writes: the list, and a search that must only match.
    const list = http.get(`${data.entries}?limit=20`, { headers: read, redirects: 0, tags: { endpoint: 'list', name: 'entries?limit' } });
    if (!check(list, {
      'list 200': (r) => r.status === 200,
      'list is a page': (r) => r.body.indexOf('{"data":[') === 0,
    })) { bad.add(1); console.log(`list broke: ${list.status} ${list.body.slice(0, 200)}`); return; }
    const search = http.get(`${data.entries}?q=Smoke`, { headers: read, redirects: 0, tags: { endpoint: 'search', name: 'entries?q' } });
    if (!check(search, {
      'search 200': (r) => r.status === 200,
      'search finds only matching titles': (r) => {
        const rows = r.json('data');
        return Array.isArray(rows) && rows.every((row) => row.revision.title.toLowerCase().indexOf('smoke') !== -1);
      },
    })) { bad.add(1); console.log(`search broke: ${search.status} ${search.body.slice(0, 300)}`); return; }
    reads.add(1);
  }
  sleep(1); // a user's beat between turns
}

/** Computes the L01 verdict: every walk step counted and none failed, releases each got a
 *  new id and the generation the walk tracked kept climbing. */
export function handleSummary(data) {
  const count = (name) => (data.metrics[name] ? data.metrics[name].values.count || 0 : 0);
  const gates = {
    steps_done: count('l01_reads') > 0 && count('l01_saves') > 0 && count('l01_revision_reads') > 0
      && count('l01_publishes') > 0 && count('l01_delivery_reads') > 0 && count('l01_refusals') > 0,
    nothing_bad: count('l01_bad') === 0,
  };
  data.l01 = {
    spec: 'L01: one-user smoke — correct bodies, authorization, revisions and release IDs; no load before basic correctness',
    gate: 'one VU walks reads, saves, revision reads, publishes (new release id, generation +1), pinned reads and refused anonymous access; every answer checked; no step may fail',
    steps: {
      reads: count('l01_reads'),
      saves: count('l01_saves'),
      revision_reads: count('l01_revision_reads'),
      publishes: count('l01_publishes'),
      delivery_reads: count('l01_delivery_reads'),
      refused_anonymous: count('l01_refusals'),
    },
    checks: data.metrics.checks ? data.metrics.checks.values : {},
    gates,
    held: gates.steps_done && gates.nothing_bad,
  };
  return {
    stdout: [
      '',
      '  L01 — one-user smoke',
      `    steps: ${count('l01_reads')} reads, ${count('l01_saves')} saves, ${count('l01_revision_reads')} revision reads, ${count('l01_publishes')} publishes, ${count('l01_delivery_reads')} delivery reads, ${count('l01_refusals')} refused anonymous attempts`,
      `    verdict: ${data.l01.held ? 'HELD' : 'BROKE'}`,
      '',
    ].join('\n'),
  };
}
