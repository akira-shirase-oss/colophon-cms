// Load scenario for localized delivery (gap/locales): anonymous /content/v1 reads with
// ?locale=, ?fallback= and ?generation=, against an environment that lists many languages
// and a collection only partly translated.
//
// setup() signs in the administrator, makes a project and sets its languages with one PUT
// {P}/locales on top of the list's ETag — 20 of them: en (the default), pt → en,
// pt-BR → pt, en, fr → en, ja (no fallback), and a long tail of fifteen more that fall back
// to en, so every read resolves against a full list. It then makes a `stories` collection
// (title and section indexed) of 20 translation groups, each an English entry: groups 0-9 get
// a Portuguese sibling, 0-4 a Brazilian one, 10-14 a French one, group 0 a Japanese one. The
// English and Portuguese members go out in release 1, the rest in release 2, so a read pinned
// to generation 1 must resolve without pt-BR. Twenty slots, each checked to the row:
//
//   - exact (4 in 20): the list in en (20 exact rows), and an English entry's id asked for in
//     pt-BR for a group that has it (the pt-BR sibling answers; requested_id names the asked id);
//   - fallback (5 in 20): the list in pt-BR (5 exact, 5 from pt, 10 from en), in a tail tag
//     (20 from en), and in pt-BR with ?fallback=en (5 exact, 15 from en); an entry asked for in
//     pt-BR answered by its pt sibling;
//   - miss (2 in 20): an entry asked for in ja (no fallback) or in pt-BR with ?fallback=none
//     where nothing in the chain is published — 404 not_found, details.reason not_in_locale and
//     details.available the group's published tags (an expected status, not a failure);
//   - pinned (3 in 20): the pt-BR list and a pt-BR entry at ?generation=1 (pt answers: pt-BR
//     wasn't out yet);
//   - entry (3 in 20): an entry read with no locale — its own locale and its alternates (every
//     published sibling, itself excluded);
//   - query (3 in 20): POST …/stories/query?locale=fr filtered to section News (10 rows: 3 in
//     fr, 7 from en).
//
// cache_hits is the share of 200 answers served from the generation-keyed cache (Colophon-Cache
// hit or coalesced). There are 55 distinct answers, and setup() reads each once before the run,
// so under 99% means the locale reads aren't cached (or the cache keeps dropping them).
// CACHE=off skips that threshold. The 404s are left out of it: whether a
// not_in_locale answer is cached is the server's choice, not this scenario's claim.
//
// LARGE=1 adds the cold fallback path on a big collection: setup also seeds `big` with GROUPS
// translation groups (default 512; GROUPS=10000 for a big one), each an English entry made
// through POST …/entries?locale=en, every fourth with a Portuguese sibling through
// POST …/localizations?locale=pt (both batched 32 at a time), published in 100-entry releases.
// One iteration in ten then runs `cold`: POST …/big/query?locale=pt-BR with a filter naming two
// real titles and a per-request decoy, so the query hash — and the cache key — is new every
// time (L03's technique): each answer is a Postgres fill that resolves pt-BR → pt → en over the
// big collection (2 rows, each from pt or en as its group has it; Colophon-Cache miss or bypass,
// never hit). Under LARGE the p99 gate splits: the warm arms keep P99_MS, the cold arm gets
// COLD_P99_MS (default 750 ms, L03's cold gate).
//
// scripts/load.sh gives this endpoint delivery's RATE 2000 and P99_MS 100.

import http from 'k6/http';
import { check, fail } from 'k6';
import { Rate } from 'k6/metrics';

const cacheHits = new Rate('cache_hits');

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '2000', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '100');
const LARGE = __ENV.LARGE === '1';
const GROUPS = parseInt(__ENV.GROUPS || '512', 10);
const COLD_P99_MS = parseFloat(__ENV.COLD_P99_MS || '750');

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const scenarios = {
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '20m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);
if (LARGE && !(GROUPS >= 2)) throw new Error(`GROUPS must be at least 2, got ${__ENV.GROUPS}`);

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'] };
if (LARGE) {
  thresholds['http_req_duration{tier:warm}'] = [`p(99)<${P99_MS}`];
  thresholds['http_req_duration{tier:cold}'] = [`p(99)<${COLD_P99_MS}`];
} else {
  thresholds.http_req_duration = [`p(99)<${P99_MS}`];
}
// Reported per kind of request, so a tail can be traced to its cause; only the gates above fail a run.
for (const endpoint of ['exact', 'fallback', 'miss', 'pinned', 'entry', 'query', 'cold_fallback']) {
  thresholds[`http_req_duration{endpoint:${endpoint}}`] = ['p(99)<60000'];
}
if (__ENV.CACHE !== 'off') {
  thresholds.cache_hits = ['rate>0.99'];
}
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  // The big collection is written one entry a request (32 at a time) and published 100 a release.
  setupTimeout: `${LARGE ? Math.max(120, Math.ceil(GROUPS / 10)) : 120}s`,
};

// The long tail: listed, each falling back to English, read in rotation by the fallback arm.
const TAIL = ['de', 'es', 'it', 'nl', 'sv', 'da', 'nb', 'fi', 'pl', 'cs', 'ko', 'zh-Hans', 'zh-Hant', 'ar', 'he'];
const LANGUAGES = [
  { tag: 'en', label: 'English', default: true, fallback: [] },
  { tag: 'pt', label: 'Português', default: false, fallback: ['en'] },
  { tag: 'pt-BR', label: 'Português (Brasil)', default: false, fallback: ['pt', 'en'] },
  { tag: 'fr', label: 'Français', default: false, fallback: ['en'] },
  { tag: 'ja', label: '日本語', default: false, fallback: [] },
].concat(TAIL.map((tag) => ({ tag, label: `Language ${tag}`, default: false, fallback: ['en'] })));

const STORIES = {
  name: 'Stories',
  title_field: 'title',
  fields: [
    { id: 'title', label: 'Title', type: 'text', required: true, index: true },
    { id: 'slug', label: 'Slug', type: 'slug' },
    { id: 'section', label: 'Section', type: 'enum', values: ['News', 'Guides'], index: true },
  ],
};
const BIG = {
  name: 'Big',
  title_field: 'title',
  fields: [{ id: 'title', label: 'Title', type: 'text', required: true, index: true }],
};

/** The published tags of story group `g` at the latest release, sorted. */
function tagsOf(g) {
  const tags = ['en'];
  if (g < 10) tags.push('pt');
  if (g < 5) tags.push('pt-BR');
  if (g >= 10 && g < 15) tags.push('fr');
  if (g === 0) tags.push('ja');
  return tags.sort();
}

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
  const key = () => `k6-delivery-locales-${run}-${(n += 1)}`;
  const keyed = (extra = {}) => Object.assign({ 'Idempotency-Key': key() }, headers, extra);

  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-${run}`, name: 'k6 delivery locales' }), { headers });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const projectId = project.json('data.id');
  const env = `${BASE_URL}/api/v1/projects/${projectId}/environments/production`;

  // The languages: one PUT of the whole list on top of the ETag the read carried.
  const listed = http.get(`${env}/locales`, { headers });
  if (listed.status !== 200) fail(`locales: HTTP ${listed.status} ${listed.body}`);
  const saved = http.put(`${env}/locales`, JSON.stringify({ locales: LANGUAGES }), { headers: keyed({ 'If-Match': listed.headers.Etag }) });
  if (saved.status !== 200) fail(`locales PUT: HTTP ${saved.status} ${saved.body}`);
  if (saved.json('data.locales').length !== LANGUAGES.length) fail(`locales PUT kept ${saved.json('data.locales').length} of ${LANGUAGES.length}`);

  const type = (slug, definition) => {
    const made = http.post(`${env}/content-types`, JSON.stringify({ slug, definition }), { headers: keyed() });
    if (made.status !== 201) fail(`content type ${slug}: HTTP ${made.status} ${made.body}`);
  };
  let generation = 0;
  const release = (ids, title) => {
    const set = http.post(`${env}/changesets`, JSON.stringify({ title, entries: ids }), { headers: keyed() });
    if (set.status !== 201) fail(`changeset ${title}: HTTP ${set.status} ${set.body}`);
    const path = `${env}/changesets/${set.json('data.id')}`;
    const ifMatch = Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, headers);
    http.post(`${path}/submit`, null, { headers: ifMatch });
    http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch });
    const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: generation }), { headers: Object.assign({ 'Idempotency-Key': key() }, ifMatch) });
    if (published.status !== 201) fail(`publish ${title}: HTTP ${published.status} ${published.body}`);
    generation = published.json('data.generation');
  };
  /** Batched creates: each request `[method, url, body]`, each answer's `data.id`. */
  const batch = (requests, what) => {
    const ids = [];
    for (let i = 0; i < requests.length; i += 32) {
      const answers = http.batch(requests.slice(i, i + 32).map(([method, url, body]) => ({ method, url, body, params: { headers: keyed() } })));
      for (const answer of answers) {
        if (answer.status !== 201) fail(`${what}: HTTP ${answer.status} ${answer.body}`);
        ids.push(answer.json('data.id'));
      }
    }
    return ids;
  };

  type('stories', STORIES);
  const entries = `${env}/collections/stories/entries`;
  const en = batch(
    Array.from({ length: 20 }, (_, g) => ['POST', `${entries}?locale=en`, JSON.stringify({ title: `Story ${g}`, slug: `story-${g}`, section: g % 2 === 0 ? 'News' : 'Guides' })]),
    'story',
  );
  const localize = (groups, locale) => batch(groups.map((g) => ['POST', `${entries}/${en[g]}/localizations?locale=${locale}`, null]), `localize ${locale}`);
  const range = (from, to) => Array.from({ length: to - from }, (_, i) => from + i);
  const pt = localize(range(0, 10), 'pt');
  const ptBR = localize(range(0, 5), 'pt-BR');
  const fr = localize(range(10, 15), 'fr');
  const ja = localize([0], 'ja');
  release(en.concat(pt), 'English and Portuguese');
  const first = generation;
  release(ptBR.concat(fr, ja), 'Brazilian, French and Japanese');

  const data = {
    content: `${BASE_URL}/content/v1/${projectId}/production/stories`,
    en, pt, ptBR, fr, first, latest: generation,
  };

  if (LARGE) {
    type('big', BIG);
    const bigEntries = `${env}/collections/big/entries`;
    const bigEn = batch(Array.from({ length: GROUPS }, (_, g) => ['POST', `${bigEntries}?locale=en`, JSON.stringify({ title: `Big ${g}` })]), 'big');
    const translated = range(0, GROUPS).filter((g) => g % 4 === 0);
    const bigPt = batch(translated.map((g) => ['POST', `${bigEntries}/${bigEn[g]}/localizations?locale=pt`, null]), 'big localize pt');
    const all = bigEn.concat(bigPt);
    for (let i = 0; i < all.length; i += 100) release(all.slice(i, i + 100), `big ${i / 100}`);
    data.big = `${BASE_URL}/content/v1/${projectId}/production/big/query`;
    data.latest = generation;
  }
  // Last: the big collection's releases move the generation, and with it every cache key.
  warm(data);
  return data;
}

/** Reads every cacheable answer the arms ask for once — 55 of them — so the measured run's
 *  cache_hits counts steady state whatever its length: a miss in the run is then an eviction or
 *  a cache that doesn't key the locale, never a first read. (LARGE's cold arm is new keys by
 *  design and stays out of cache_hits.) */
function warm(data) {
  const lists = ['locale=en', 'locale=pt-BR', 'locale=pt-BR&fallback=en', `locale=pt-BR&generation=${data.first}`]
    .concat(TAIL.map((tag) => `locale=${tag}`))
    .map((query) => ['GET', `${data.content}?limit=100&${query}`, null]);
  const entries = data.en.map((id) => ['GET', `${data.content}/${id}`, null])
    .concat(data.en.slice(0, 10).map((id) => ['GET', `${data.content}/${id}?locale=pt-BR`, null]))
    .concat(data.en.slice(0, 5).map((id) => ['GET', `${data.content}/${id}?locale=pt-BR&generation=${data.first}`, null]));
  const query = [['POST', `${data.content}/query?locale=fr`, JSON.stringify({ filters: [{ field: 'section', op: 'eq', value: 'News' }], limit: 100 })]];
  const reads = lists.concat(entries, query).map(([method, url, body]) => ({ method, url, body, params: { headers: { 'Content-Type': 'application/json' } } }));
  for (let i = 0; i < reads.length; i += 16) {
    for (const answer of http.batch(reads.slice(i, i + 16))) {
      if (answer.status !== 200) fail(`warm ${answer.request.url}: HTTP ${answer.status} ${answer.body}`);
    }
  }
}

/** The rows' `localization.resolved`, counted by tag — `{pt-BR: 5, pt: 5, en: 10}`. */
function resolved(rows, requested) {
  const counts = {};
  for (const row of rows) {
    const how = row.localization || {};
    if (how.requested !== requested) return null;
    if (how.fallback !== (how.resolved !== requested)) return null;
    counts[how.resolved] = (counts[how.resolved] || 0) + 1;
  }
  return counts;
}

/** JSON with every object's keys sorted, so two values compare by content, not insertion order. */
function canonical(value) {
  if (Array.isArray(value)) return `[${value.map(canonical).join(',')}]`;
  if (value !== null && typeof value === 'object') {
    return `{${Object.keys(value).sort().map((k) => `${JSON.stringify(k)}:${canonical(value[k])}`).join(',')}}`;
  }
  return JSON.stringify(value);
}

function same(a, b) {
  return canonical(a) === canonical(b);
}

function list(data, query, endpoint, expected, requested, generation) {
  const res = http.get(`${data.content}?limit=100&${query}`, { redirects: 0, tags: { endpoint, tier: 'warm' } });
  check(res, {
    [`${endpoint}: ${query} resolves every group once`]: (r) =>
      r.status === 200 &&
      r.headers['Colophon-Generation'] === String(generation) &&
      r.headers['Content-Language'] === requested &&
      same(resolved(r.json('data'), requested), expected),
  });
  return res;
}

function entry(data, id, query, endpoint, expect) {
  const res = http.get(`${data.content}/${id}${query}`, { redirects: 0, tags: { endpoint, tier: 'warm' } });
  check(res, { [`${endpoint}: entry ${query || 'without a locale'}`]: (r) => r.status === 200 && expect(r.json('data'), r) });
  return res;
}

function alternates(row) {
  return (row.alternates || []).map((other) => other.locale).sort();
}

export default function (data) {
  if (LARGE && __ITER % 10 === 9) {
    cold(data);
    return;
  }
  const slot = __ITER % 20;
  const pick = (from, count) => from + (Math.floor(__ITER / 20) % count);
  let res;
  if (slot < 2) {
    res = list(data, 'locale=en', 'exact', { en: 20 }, 'en', data.latest);
  } else if (slot < 4) {
    const g = pick(0, 5);
    res = entry(data, data.en[g], '?locale=pt-BR', 'exact', (row) =>
      row.id === data.ptBR[g] && row.requested_id === data.en[g] && row.locale === 'pt-BR' &&
      same(row.localization, { requested: 'pt-BR', resolved: 'pt-BR', fallback: false }) &&
      same(alternates(row), tagsOf(g).filter((tag) => tag !== 'pt-BR')));
  } else if (slot < 6) {
    res = list(data, 'locale=pt-BR', 'fallback', { 'pt-BR': 5, pt: 5, en: 10 }, 'pt-BR', data.latest);
  } else if (slot < 7) {
    const tag = TAIL[Math.floor(__ITER / 20) % TAIL.length];
    res = list(data, `locale=${tag}`, 'fallback', { en: 20 }, tag, data.latest);
  } else if (slot < 8) {
    res = list(data, 'locale=pt-BR&fallback=en', 'fallback', { 'pt-BR': 5, en: 15 }, 'pt-BR', data.latest);
  } else if (slot < 9) {
    const g = pick(5, 5);
    res = entry(data, data.en[g], '?locale=pt-BR', 'fallback', (row) =>
      row.id === data.pt[g] && row.requested_id === data.en[g] &&
      same(row.localization, { requested: 'pt-BR', resolved: 'pt', fallback: true }));
  } else if (slot < 11) {
    // Nothing in the chain is published: ja has no fallback; pt-BR with none is exact only.
    const g = slot === 9 ? pick(10, 10) : pick(5, 5);
    const query = slot === 9 ? '?locale=ja' : '?locale=pt-BR&fallback=none';
    const miss = http.get(`${data.content}/${data.en[g]}${query}`, {
      redirects: 0, tags: { endpoint: 'miss', tier: 'warm' }, responseCallback: http.expectedStatuses(404),
    });
    check(miss, {
      'miss: 404 not_in_locale naming the published tags': (r) =>
        r.status === 404 && r.json('error.code') === 'not_found' && r.json('error.details.reason') === 'not_in_locale' &&
        same((r.json('error.details.available') || []).slice().sort(), tagsOf(g)),
    });
    return;
  } else if (slot < 13) {
    res = list(data, `locale=pt-BR&generation=${data.first}`, 'pinned', { pt: 10, en: 10 }, 'pt-BR', data.first);
  } else if (slot < 14) {
    const g = pick(0, 5);
    res = entry(data, data.en[g], `?locale=pt-BR&generation=${data.first}`, 'pinned', (row, r) =>
      r.headers['Colophon-Generation'] === String(data.first) && row.id === data.pt[g] &&
      same(row.localization, { requested: 'pt-BR', resolved: 'pt', fallback: true }) &&
      same(alternates(row), ['en']));
  } else if (slot < 17) {
    const g = pick(0, 20);
    res = entry(data, data.en[g], '', 'entry', (row) =>
      row.id === data.en[g] && row.locale === 'en' && row.localization === undefined &&
      same(alternates(row), tagsOf(g).filter((tag) => tag !== 'en')));
  } else {
    res = http.post(`${data.content}/query?locale=fr`, JSON.stringify({ filters: [{ field: 'section', op: 'eq', value: 'News' }], limit: 100 }), {
      headers: { 'Content-Type': 'application/json' }, redirects: 0, tags: { endpoint: 'query', tier: 'warm' },
    });
    check(res, {
      'query: News in fr, French where published, English otherwise': (r) =>
        r.status === 200 && r.headers['Content-Language'] === 'fr' && same(resolved(r.json('data'), 'fr'), { fr: 3, en: 7 }),
    });
  }
  if (res.status === 200) {
    const served = res.headers['Colophon-Cache'];
    cacheHits.add(served === 'hit' || served === 'coalesced');
  }
}

/** LARGE's cold arm: a query whose key nobody asked before, resolving pt-BR → pt → en over the
 *  big collection — two real groups and a decoy, so the answer is two rows. */
function cold(data) {
  const decoy = `decoy-${__VU}-${__ITER}-${Math.random().toString(36).slice(2, 10)}`;
  const a = __ITER % GROUPS;
  let b = (__ITER * 7 + 3) % GROUPS;
  if (b === a) b = (a + 1) % GROUPS;
  const res = http.post(`${data.big}?locale=pt-BR`, JSON.stringify({ filters: [{ field: 'title', op: 'in', value: [`Big ${a}`, `Big ${b}`, decoy] }], limit: 5 }), {
    headers: { 'Content-Type': 'application/json' }, redirects: 0, tags: { endpoint: 'cold_fallback', tier: 'cold' },
  });
  const want = (g) => (g % 4 === 0 ? 'pt' : 'en');
  check(res, {
    'cold: two groups, each from pt or en as translated': (r) => {
      if (r.status !== 200) return false;
      const rows = r.json('data');
      if (rows.length !== 2) return false;
      return rows.every((row) => {
        const g = parseInt(String(row.title).slice('Big '.length), 10);
        return (g === a || g === b) && same(row.localization, { requested: 'pt-BR', resolved: want(g), fallback: true });
      });
    },
    'cold: a fill of its own, never a hit': (r) => r.headers['Colophon-Cache'] === 'miss' || r.headers['Colophon-Cache'] === 'bypass',
  });
}
