// Load scenario for the five Astro demo sites' real read shapes (apps/sites/README.md; D-133).
// The sites are static builds, so an audience lands on the delivery origin in three ways:
// media (`/media/v1`) on every portfolio page — ten w320 renditions on the index, a w1280 on
// each photo page, the full original on a download; collection lists (`/content/v1`) the way
// `apps/sites/shared`'s paging fetch reads them — a 100-entry first page at the latest
// release, and smaller pages walked by cursor, whose pin keeps the generation of the first
// page; and single-entry reads, latest and pinned at the seeded release, plus a browser's
// `If-None-Match` revalidation, which must answer 304.
//
// setup() reads the seeded project's record: `DEMO_PROJECT` and `DEMO_ENVIRONMENT` when set,
// else `apps/sites/.colophon.json` (written by scripts/seed-demo-sites.sh). It then lists each
// collection once — anonymously, the way the sites' own builds do — to learn the entry ids,
// the photos' asset ids (`content.photo`), one list's ETag and a cursor into page two of a
// five-entry page. Nothing is written; every measured request is a public read.
//
// One hundred iterations hold 35 w320 thumbs, 12 w1280s, 5 originals, 15 latest entries,
// 5 entries pinned at generation 1, 12 first pages, 5 five-entry first pages, 5 cursor
// continuations, 5 revalidations and 1 made-up entry (a 404, which is never cached).
// cache_hits covers the reads that may be cached: media and the 404 cannot be, so under ~95%
// the delivery cache isn't working; CACHE=off skips that threshold.

import http from 'k6/http';
import { check, fail } from 'k6';
import exec from 'k6/execution';
import { Rate } from 'k6/metrics';

const cacheHits = new Rate('cache_hits');

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '2000', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '100');

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const scenarios = {
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '30m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
// Reported per read shape, so a tail can be traced to its cause; only the overall p99 fails a run.
for (const endpoint of ['list', 'list_page', 'list_next', 'entry', 'entry_pinned', 'revalidate', 'media_thumb', 'media_large', 'media_original', 'gone']) {
  thresholds[`http_req_duration{endpoint:${endpoint}}`] = ['p(99)<60000'];
}
if (__ENV.CACHE !== 'off') {
  thresholds.cache_hits = ['rate>0.95'];
}
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '60s',
};

const COLLECTIONS = ['posts', 'guides', 'recipes', 'stories', 'photos'];

// The seed's record (open() is init-stage only, so it is read here, and may legitimately not
// exist: setup() then asks for DEMO_PROJECT).
let seeded = null;
try {
  seeded = JSON.parse(open('../../apps/sites/.colophon.json'));
} catch {
  seeded = null;
}

export function setup() {
  let project = __ENV.DEMO_PROJECT || (seeded && seeded.project);
  const environment = __ENV.DEMO_ENVIRONMENT || (seeded && seeded.environment) || 'production';
  if (!project) {
    fail('audience-sites needs the seeded demo-sites project: run scripts/seed-demo-sites.sh, or set DEMO_PROJECT');
  }
  const base = `${BASE_URL}/content/v1/${project}/${environment}`;
  const entries = {};
  const assets = [];
  let generation = 0;
  let first = null;
  for (const collection of COLLECTIONS) {
    const res = http.get(`${base}/${collection}?limit=100`);
    if (res.status !== 200) fail(`setup: ${collection} list: HTTP ${res.status}`);
    const page = res.json();
    entries[collection] = page.data.map((entry) => entry.id);
    if (page.data.length === 0) fail(`setup: ${collection} is empty — seed the demo sites first`);
    generation = Math.max(generation, parseInt(res.headers['Colophon-Generation'], 10) || 0);
    if (collection === 'photos') {
      for (const entry of page.data) {
        if (typeof entry.content.photo === 'string') assets.push(entry.content.photo);
      }
    }
    if (first === null) {
      first = { collection, etag: res.headers.Etag, count: page.data.length };
    }
  }
  if (assets.length === 0) fail('setup: photos carry no asset ids — seed the demo sites first');
  if (generation === 0) fail('setup: no generation named — is anything published?');
  // A five-entry first page's cursor: continuation reads it, pinned to its generation.
  const paged = http.get(`${base}/posts?limit=5`);
  if (paged.status !== 200 || !paged.json('next_cursor')) fail('setup: posts never paged — seed the demo sites first');
  const cursor = paged.json('next_cursor');
  // Warm the read keys the way a live audience's first views would: every cacheable answer
  // exists before the measured run, so cache_hits measures the cache working — a cold miss is
  // exactly one per key per TTL window and would only blur the gate on short runs.
  for (const collection of COLLECTIONS) {
    for (const id of entries[collection]) {
      http.get(`${base}/${collection}/${id}`);
      http.get(`${base}/${collection}/${id}?generation=1`);
    }
    http.get(`${base}/${collection}?limit=100`);
  }
  http.get(`${base}/guides?limit=5`);
  http.get(`${base}/posts?cursor=${encodeURIComponent(cursor)}`);
  return {
    base,
    media: `${BASE_URL}/media/v1/${project}/${environment}`,
    generation,
    entries,
    assets,
    revalidate: first,
    cursor,
    missing: '00000000-0000-7000-8000-000000000000',
  };
}

export default function (data) {
  // The scenario-wide iteration index: __ITER is per-VU, and at modest rates on many
  // preallocated VUs it never leaves the media slots, skewing the mix to images only.
  const n = exec.scenario.iterationInTest;
  const slot = n % 100;
  const params = (endpoint, extra) => Object.assign({ redirects: 0, tags: { endpoint } }, extra);
  let res;
  if (slot < 35) {
    const asset = data.assets[n % data.assets.length];
    res = http.get(`${data.media}/${asset}/w320`, params('media_thumb'));
    check(res, { 'media_thumb 200, an image': (r) => r.status === 200 && r.headers['Content-Type'] === 'image/png' && r.body.length > 0 });
    return;
  } else if (slot < 47) {
    const asset = data.assets[n % data.assets.length];
    res = http.get(`${data.media}/${asset}/w1280`, params('media_large'));
    check(res, { 'media_large 200, an image': (r) => r.status === 200 && r.headers['Content-Type'] === 'image/png' && r.body.length > 0 });
    return;
  } else if (slot < 52) {
    const asset = data.assets[n % data.assets.length];
    res = http.get(`${data.media}/${asset}`, params('media_original'));
    check(res, { 'media_original 200, an image': (r) => r.status === 200 && r.headers['Content-Type'] === 'image/png' && r.body.length > 0 });
    return;
  } else if (slot < 67) {
    const collection = COLLECTIONS[n % COLLECTIONS.length];
    const ids = data.entries[collection];
    const entry = ids[(n / COLLECTIONS.length | 0) % ids.length];
    res = http.get(`${data.base}/${collection}/${entry}`, params('entry'));
    check(res, { 'entry: 200, the entry asked for': (r) => r.status === 200 && r.json('data.id') === entry });
  } else if (slot < 72) {
    const collection = COLLECTIONS[n % COLLECTIONS.length];
    const ids = data.entries[collection];
    const entry = ids[(n / COLLECTIONS.length | 0) % ids.length];
    res = http.get(`${data.base}/${collection}/${entry}?generation=1`, params('entry_pinned'));
    check(res, { 'pinned entry: 200 at generation 1': (r) => r.status === 200 && r.headers['Colophon-Generation'] === '1' });
  } else if (slot < 84) {
    const collection = COLLECTIONS[n % COLLECTIONS.length];
    res = http.get(`${data.base}/${collection}?limit=100`, params('list'));
    check(res, { 'list: 200, the collection at its release': (r) => r.status === 200 && r.json('data').length === data.entries[collection].length });
  } else if (slot < 89) {
    res = http.get(`${data.base}/guides?limit=5`, params('list_page'));
    check(res, { 'list_page: five of the guides, with more after': (r) => r.status === 200 && r.json('data').length === 5 && r.json('next_cursor') });
  } else if (slot < 94) {
    res = http.get(`${data.base}/posts?cursor=${encodeURIComponent(data.cursor)}`, params('list_next'));
    check(res, { 'list_next: the page after, pinned to its generation': (r) => r.status === 200 && r.json('data').length >= 1 });
  } else if (slot < 99) {
    res = http.get(`${data.base}/${data.revalidate.collection}?limit=100`, params('revalidate', { headers: { 'If-None-Match': data.revalidate.etag } }));
    check(res, { 'revalidate: 304': (r) => r.status === 304 });
  } else {
    res = http.get(`${data.base}/posts/${data.missing}`, params('gone', { responseCallback: http.expectedStatuses(404) }));
    check(res, { 'made-up entry: 404, not 410': (r) => r.status === 404 });
    return;
  }
  const served = res.headers['Colophon-Cache'];
  cacheHits.add(served === 'hit' || served === 'coalesced');
}
