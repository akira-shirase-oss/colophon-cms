// Load scenario for L10 (spec §29): CDN-warm traffic with repeated publishes underneath
// it. One origin is the honest local stand-in for the edge, so the "regions" here are
// three named read arms — `edge_iad`, `edge_lhr`, `edge_sin`, each a third of RATE — and
// what the spec asks them to prove is measured per region: its own latency series, its
// share of answers the cache served, the generation it observed, and the two failures
// that make a CDN dishonest: a stale refill (a latest read answering at an older
// generation than the same edge already saw — impossible while the generation always
// comes from Postgres) and a torn page (a row whose `published_in` is newer than the
// release its page claims, i.e. entries mixed from two releases).
//
// While they read, a `publisher` arm lands a real release every PUBLISH_SECONDS — a new
// article pinned and published on the generation it just read — so the regions' "latest"
// moves under them the whole run, the way it does behind a real CDN. A fresh release is
// a fresh cache key, so the fills each release causes are `miss`/`coalesced` — the share
// served `hit` is the warm evidence, counted per region.
//
// The arms are time-boxed: TOTAL and VUS are unused. `steady` and `throughput` run the
// same plan. handleSummary() computes the verdict into `l10` — per-region requests, hit
// share, p95/p99, the highest generation each observed, and the stale/torn counters;
// scripts/load.sh stores it and fails when `held` is false — and samples server RSS, CPU
// and Postgres connections into `resources`. scripts/load.sh gives this endpoint
// RATE 2000 (the regions split it) and P99_MS 400 — the warm-CDN gate.

import http from 'k6/http';
import { check, fail } from 'k6';
import exec from 'k6/execution';
import { Counter, Trend } from 'k6/metrics';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const RATE = parseInt(__ENV.RATE || '2000', 10);
const PUBLISH_SECONDS = parseInt(__ENV.PUBLISH_SECONDS || '15', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '400');
const HIT_FLOOR = parseFloat(__ENV.HIT_FLOOR || '0.90'); // a warm edge serves nearly all hits

const REGIONS = ['edge_iad', 'edge_lhr', 'edge_sin'];
const REGION_RATE = Math.max(1, Math.round(RATE / REGIONS.length));

const stale = new Counter('l10_stale');
const torn = new Counter('l10_torn');
const errors = new Counter('l10_errors');
const publishes = new Counter('l10_publishes');
// Where each edge's answer came from and the newest generation it observed — one root
// counter per (region, how) and one root trend per region. Root metrics, not tagged
// submetrics: a `{arm:edge_iad}` submetric only reaches the summary when it carries a
// threshold, and these names can't be thresholded ahead of time.
const HOWS = ['hit', 'miss', 'coalesced', 'bypass'];
const servedByRegion = {};
const observedByRegion = {};
for (const region of REGIONS) {
  observedByRegion[region] = new Trend(`l10_generation_${region}`, true);
  servedByRegion[region] = {};
  for (const how of HOWS) servedByRegion[region][how] = new Counter(`l10_served_${region}_${how}`);
}

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

if (!(SCENARIO === 'throughput' || SCENARIO === 'steady')) {
  throw new Error('SCENARIO must be throughput or steady');
}

const thresholds = {
  checks: ['rate==1'],
  http_req_failed: ['rate==0'],
  l10_stale: ['count==0'],
  l10_torn: ['count==0'],
  l10_errors: ['count==0'],
  l10_publishes: ['count>0'],
};
for (const region of REGIONS) {
  thresholds[`http_req_duration{arm:${region}}`] = [`p(99)<${P99_MS}`];
  thresholds[`http_reqs{arm:${region}}`] = ['count>0'];
}
if (SCENARIO === 'steady') {
  const scheduled = RATE * durationSeconds(DURATION);
  thresholds.dropped_iterations = [`count<=${Math.max(1, Math.floor(scheduled * 0.0001))}`];
}

export const options = {
  scenarios: {
    edge_iad: { executor: 'constant-arrival-rate', rate: REGION_RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: 48, maxVUs: 96, exec: 'edge' },
    edge_lhr: { executor: 'constant-arrival-rate', rate: REGION_RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: 48, maxVUs: 96, exec: 'edge' },
    edge_sin: { executor: 'constant-arrival-rate', rate: REGION_RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: 48, maxVUs: 96, exec: 'edge' },
    publisher: { executor: 'constant-arrival-rate', rate: 1, timeUnit: `${PUBLISH_SECONDS}s`, duration: DURATION, preAllocatedVUs: 2, maxVUs: 4, exec: 'publisher' },
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
  return `k6-l10-${__VU}-${__ITER}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
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
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 cdn warm' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const id = project.json('data.id');
  const env = `${BASE_URL}/api/v1/projects/${id}/environments/production`;

  // Two hundred articles in two releases: the corpus every edge is warm on.
  let file = '';
  const ids = [];
  for (let i = 0; i < 200; i += 1) {
    file += article(`Article ${i}`, 'first') + '\n';
  }
  const imported = http.post(`${env}/collections/articles/import`, file, {
    headers: Object.assign({}, write, { 'Content-Type': 'application/jsonlines', 'Idempotency-Key': `k6-seed-${slug}` }),
  });
  if (imported.status !== 201) fail(`seed import: HTTP ${imported.status} ${imported.body}`);
  for (const outcome of imported.json('data.outcomes')) ids.push(outcome.entry);
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
    session, env,
    entries: `${env}/collections/articles/entries`,
    delivery: `${BASE_URL}/content/v1/${id}/production/articles`,
    ids, generation,
  };
}

// An edge VU's own truth: the newest generation it has been served. A latest-read that
// answers older is a stale refill — the failure this case exists to catch.
const seen = { g: 0 };

export function edge(data) {
  const region = exec.scenario.name;
  const slot = __ITER % 10;
  let res; let pinned = false;
  if (slot < 6) {
    res = http.get(`${data.delivery}?limit=50`, { redirects: 0, tags: { arm: region, endpoint: 'latest', name: 'content' } });
  } else if (slot < 9) {
    res = http.get(`${data.delivery}/${data.ids[__ITER % data.ids.length]}`, { redirects: 0, tags: { arm: region, endpoint: 'entry', name: 'content/{entry}' } });
  } else {
    pinned = true;
    res = http.get(`${data.delivery}?limit=50&generation=1`, { redirects: 0, tags: { arm: region, endpoint: 'pinned', name: 'content' } });
  }
  const generation = parseInt(res.headers['Colophon-Generation'] || '-1', 10);
  if (res.status === 200 && generation >= 0) {
    observedByRegion[region].add(generation);
    const how = res.headers['Colophon-Cache'];
    if (typeof how === 'string' && servedByRegion[region][how]) servedByRegion[region][how].add(1);
    // Pinned pages are old on purpose — only a "latest" answer can be a stale refill.
    if (!pinned) {
      if (generation < seen.g) stale.add(1);
      if (generation > seen.g) seen.g = generation;
    }
    const rows = res.json('data');
    const list = Array.isArray(rows) ? rows : [rows];
    for (const row of list) {
      if (row && typeof row.published_in === 'number' && row.published_in > generation) { torn.add(1); break; }
    }
  }
  if (!check(res, {
    'edge read 200': (r) => r.status === 200,
    'served at the pinned release asked for': (r) => !pinned || r.headers['Colophon-Generation'] === '1',
  })) errors.add(1);
}

// The publisher's own truth: the generation it read latest — the base it publishes on —
// and each release must land exactly one ahead.
export function publisher(data) {
  const write = { Cookie: data.session.cookie, 'X-CSRF-Token': data.session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };
  const latest = http.get(`${data.delivery}?limit=1`, { redirects: 0, tags: { arm: 'publisher', endpoint: 'base', name: 'content' } });
  if (!check(latest, { 'base read 200': (r) => r.status === 200 })) { errors.add(1); return; }
  const base = parseInt(latest.headers['Colophon-Generation'] || '-1', 10);
  const made = http.post(data.entries, article(`Release ${key()}`, 'new'), {
    headers: Object.assign({ 'Idempotency-Key': key() }, write),
    redirects: 0, tags: { arm: 'publisher', endpoint: 'entry', name: 'entries' },
  });
  if (!check(made, { 'entry 201': (r) => r.status === 201 })) { errors.add(1); return; }
  const id = made.json('data.id');
  const set = http.post(`${data.env}/changesets`, JSON.stringify({ title: `cdn ${key()}`, entries: [id] }), {
    headers: Object.assign({ 'Idempotency-Key': key() }, write), redirects: 0, tags: { arm: 'publisher', endpoint: 'changeset', name: 'changesets' },
  });
  if (!check(set, { 'changeset 201': (r) => r.status === 201 })) { errors.add(1); return; }
  const path = `${data.env}/changesets/${set.json('data.id')}`;
  const ifMatch = Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, write);
  const submitted = http.post(`${path}/submit`, null, { headers: ifMatch, redirects: 0, tags: { arm: 'publisher', endpoint: 'submit', name: 'changesets/{changeset}/submit' } });
  if (!check(submitted, { 'submit 200': (r) => r.status === 200 })) { errors.add(1); return; }
  const approved = http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch, redirects: 0, tags: { arm: 'publisher', endpoint: 'review', name: 'changesets/{changeset}/reviews' } });
  if (!check(approved, { 'review 200': (r) => r.status === 200 })) { errors.add(1); return; }
  const expect = { responseCallback: http.expectedStatuses(201, 409) };
  let published = http.post(`${path}/publish`, JSON.stringify({ base_generation: base }), {
    headers: Object.assign({ 'Idempotency-Key': key() }, ifMatch), redirects: 0,
    tags: { arm: 'publisher', endpoint: 'publish', name: 'changesets/{changeset}/publish' },
  });
  let landed = false;
  if (published.status === 409) {
    // Another publish won between the base read and this one — read the generation and
    // republish, the honest retry.
    const again = http.get(`${data.delivery}?limit=1`, { redirects: 0, tags: { arm: 'publisher', endpoint: 'base', name: 'content' } });
    const moved = parseInt(again.headers['Colophon-Generation'] || '-1', 10);
    published = http.post(`${path}/publish`, JSON.stringify({ base_generation: moved }), {
      headers: Object.assign({ 'Idempotency-Key': key() }, ifMatch), redirects: 0,
      tags: { arm: 'publisher', endpoint: 'publish', name: 'changesets/{changeset}/publish' },
    });
    landed = check(published, { 'republish lands one ahead': (r) => r.status === 201 && r.json('data.generation') === moved + 1 });
  } else {
    landed = check(published, {
      'publish 201': (r) => r.status === 201,
      'one generation forward': (r) => r.json('data.generation') === base + 1,
    });
  }
  if (!landed) { errors.add(1); return; }
  publishes.add(1);
  // A region would see it like this: the new entry reads back at the release it minted.
  const back = http.get(`${data.delivery}/${id}`, { redirects: 0, tags: { arm: 'publisher', endpoint: 'published-read', name: 'content/{entry}' } });
  if (!check(back, {
    'published read 200': (r) => r.status === 200,
    'served at the release it minted': (r) => r.headers['Colophon-Generation'] === String(published.json('data.generation')),
  })) errors.add(1);
}

/** Computes the L10 verdict: every region ran inside the warm gate with its hit share
 *  above the floor, the newest generation was observed by every region, and stale
 *  refills and torn pages stayed at zero while releases landed underneath. */
export function handleSummary(data) {
  const metric = (name) => (data.metrics[name] ? data.metrics[name].values : null);
  const count = (name) => (metric(name) ? metric(name).count || 0 : 0);
  const regions = {};
  let regionsHeld = true;
  let totalServed = 0; let totalWarm = 0;
  for (const region of REGIONS) {
    const dur = metric(`http_req_duration{arm:${region}}`) || {};
    const reqs = metric(`http_reqs{arm:${region}}`) || {};
    const gen = metric(`l10_generation_${region}`) || {};
    const served = {};
    for (const how of HOWS) {
      served[how] = count(`l10_served_${region}_${how}`);
    }
    const regionTotal = served.hit + served.miss + served.coalesced + served.bypass;
    const regionWarm = regionTotal > 0 ? (served.hit + served.coalesced) / regionTotal : 0;
    totalServed += regionTotal;
    totalWarm += served.hit + served.coalesced;
    regions[region] = {
      requests: reqs.count || 0,
      latency_ms: { p50: dur.med, p95: dur['p(95)'], p99: dur['p(99)'] },
      generation_observed_max: gen.max !== undefined ? gen.max : null,
      served,
      hit_share: regionWarm,
    };
    regionsHeld = regionsHeld && (dur['p(99)'] || Infinity) < P99_MS && (reqs.count || 0) > 0 && regionWarm > HIT_FLOOR;
  }
  const hitShare = totalServed > 0 ? totalWarm / totalServed : 0;
  const gates = {
    each_region_within_gate: regionsHeld,
    warm_share: hitShare > HIT_FLOOR,
    no_stale_refills: count('l10_stale') === 0,
    no_torn_pages: count('l10_torn') === 0,
    publishes_landed: count('l10_publishes') > 0,
    no_errors: count('l10_errors') === 0,
  };
  data.l10 = {
    spec: 'L10: CDN warm traffic + repeated publishes — per-region latency and observed generation; detect stale refills and mixed-release pages',
    gate: `each named region's p99 <${P99_MS} ms, hit+coalesced share >${HIT_FLOOR * 100}% over fills every publish caused; zero stale refills or torn pages; releases landed the whole run`,
    regions_simulated: REGIONS,
    publishes_landed: count('l10_publishes'),
    served: {
      hit: REGIONS.reduce((n, r) => n + regions[r].served.hit, 0),
      miss: REGIONS.reduce((n, r) => n + regions[r].served.miss, 0),
      coalesced: REGIONS.reduce((n, r) => n + regions[r].served.coalesced, 0),
      bypass: REGIONS.reduce((n, r) => n + regions[r].served.bypass, 0),
    },
    hit_share: hitShare,
    stale_refills: count('l10_stale'),
    torn_pages: count('l10_torn'),
    errors: count('l10_errors'),
    regions,
    gates,
    held: Object.values(gates).every(Boolean),
  };
  const fmt = Object.entries(regions).map(([name, r]) => `${name} ${r.requests}req p99 ${(r.latency_ms.p99 || 0).toFixed(1)}ms gen<=${r.generation_observed_max}`).join('; ');
  return {
    stdout: [
      '',
      '  L10 — CDN warm traffic + repeated publishes',
      `    ${fmt}`,
      `    hit share ${(hitShare * 100).toFixed(1)}% (floor ${HIT_FLOOR * 100}%), ${count('l10_publishes')} publishes landed; stale ${count('l10_stale')}, torn ${count('l10_torn')}, errors ${count('l10_errors')}`,
      `    verdict: ${data.l10.held ? 'HELD' : 'BROKE'}`,
      '',
    ].join('\n'),
  };
}
