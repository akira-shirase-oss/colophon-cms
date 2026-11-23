// Load scenario for images inside rich text (gap 3; D-177, D-178): the save that records a
// document's images as asset references, and the delivery reads that carry the versions a
// release recorded. setup() signs in, makes a project with a `posts` type (a title and a
// rich-text body), uploads IMAGES real PNGs and waits for the worker to verify them, then
// publishes 20 posts that each name every image — an image-heavy collection.
//
// Each iteration is one of:
// - half: the collection's latest page (20 rows × IMAGES media entries, served from the
//   generation-keyed cache after the first fill) — every row must carry every image's
//   versioned URL;
// - a fifth: one post, pinned to the setup's release;
// - a fifth: a new draft naming every image (the save arm: validation collects the images,
//   one indexed ANY read checks them, IMAGES reference rows are written);
// - a tenth: a draft naming an id that is no upload — refused 422 asset_not_found at its
//   pointer, the refusal's own cost.
// RATE counts iterations (one request each); scripts/load.sh gives it the entries arm's rate
// and editorial write budget, since a fifth of the requests are writes.

import http from 'k6/http';
import { check, fail, sleep } from 'k6';
import exec from 'k6/execution';
import { idempotencyKey } from './lib/idempotency-key.js';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '500', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '250');
const IMAGES = parseInt(__ENV.IMAGES || '20', 10);

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const scenarios = {
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '40m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
  stress: {
    executor: 'ramping-arrival-rate', startRate: Math.floor(RATE / 4), timeUnit: '1s', preAllocatedVUs: VUS, maxVUs: VUS * 4,
    stages: [{ target: RATE, duration: '30s' }, { target: RATE * 2, duration: '30s' }, { target: RATE * 4, duration: '30s' }],
  },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);

// The refusal arm answers 422 by design: it is expected, not a failure.
http.setResponseCallback(http.expectedStatuses({ min: 200, max: 299 }, 422));

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}
if (SCENARIO === 'stress') delete thresholds.http_req_duration;

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '300s',
};

// A 400×4 PNG: wider than w320, so each image has one rendition in its media entry.
const WIDE = new Uint8Array([
  137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 1, 144, 0, 0, 0, 4, 8, 2,
  0, 0, 0, 47, 93, 254, 122, 0, 0, 0, 42, 73, 68, 65, 84, 120, 156, 237, 206, 65, 9, 0, 48, 12,
  4, 176, 74, 159, 243, 206, 194, 253, 142, 66, 32, 2, 50, 243, 22, 224, 134, 254, 0, 32, 212,
  31, 0, 132, 250, 3, 128, 204, 7, 24, 121, 90, 72, 220, 227, 41, 34, 0, 0, 0, 0, 73, 69, 78,
  68, 174, 66, 96, 130,
]);

// __VU and __ITER exist only in iterations; setup() counts its own keys.
// per-VU state: counts setup-time keys only (typeof __ITER undefined); unused in iterations.
let setupKeys = 0;
function key() {
  // Not exec.scenario: k6 throws reading it outside a VU (setup), CI 44698136206.
  const where = typeof __ITER === 'undefined' ? ['setup', (setupKeys += 1)] : [__VU, __ITER];
  return idempotencyKey('k6-rtm', ...where, Date.now().toString(36), Math.random().toString(36).slice(2, 10), 'rich-text-media');
}

const post = (title, images) => ({
  title,
  body: {
    schema: 'colophon/rich-text',
    version: 1,
    doc: {
      type: 'doc',
      content: [{ type: 'paragraph', content: [{ type: 'text', text: title }] }].concat(
        images.map((id, n) => ({ type: 'image', attrs: { assetId: id, alt: `Figure ${n + 1}` } })),
      ),
    },
  },
});

export function setup() {
  const signedIn = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }), { headers: { 'Content-Type': 'application/json' } });
  if (signedIn.status !== 204) fail(`sign-in failed: HTTP ${signedIn.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const headers = {
    Cookie: `__Host-colophon_session=${jar['__Host-colophon_session'][0]}`, 'X-CSRF-Token': jar['__Host-colophon_csrf'][0], Origin: PUBLIC_URL,
    'Content-Type': 'application/json',
  };
  const run = Date.now().toString(36);
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-rtm-${run}`, name: 'k6 rich-text media' }), { headers });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const projectId = project.json('data.id');
  const base = `${BASE_URL}/api/v1/projects/${projectId}/environments/production`;
  const type = http.post(`${base}/content-types`, JSON.stringify({
    slug: 'posts',
    definition: { name: 'Posts', title_field: 'title', fields: [
      { id: 'title', label: 'Title', type: 'text', required: true },
      { id: 'body', label: 'Body', type: 'rich_text' },
    ] },
  }), { headers: Object.assign({ 'Idempotency-Key': key() }, headers) });
  if (type.status !== 201) fail(`type: HTTP ${type.status} ${type.body}`);

  const images = [];
  for (let n = 0; n < IMAGES; n += 1) {
    const made = http.post(`${base}/assets`, WIDE.buffer, {
      headers: { Cookie: headers.Cookie, 'X-CSRF-Token': headers['X-CSRF-Token'], Origin: PUBLIC_URL, 'Content-Type': 'image/png', 'X-Colophon-File-Name': `figure-${n}.png`, 'Idempotency-Key': key() },
    });
    if (made.status !== 202) fail(`upload: HTTP ${made.status} ${made.body}`);
    images.push(made.json('data.id'));
  }
  for (const id of images) {
    for (let waits = 0; ; waits += 1) {
      const one = http.get(`${base}/assets/${id}`, { headers });
      if (one.json('data.state') === 'usable') break;
      if (waits === 300) fail(`image never verified: ${one.body}`);
      sleep(0.1);
    }
  }

  const entries = [];
  for (let n = 0; n < 20; n += 1) {
    const created = http.post(`${base}/collections/posts/entries`, JSON.stringify(post(`Post ${n}`, images)), { headers: Object.assign({ 'Idempotency-Key': key() }, headers) });
    if (created.status !== 201) fail(`entry: HTTP ${created.status} ${created.body}`);
    entries.push(created.json('data.id'));
  }
  const set = http.post(`${base}/changesets`, JSON.stringify({ title: 'images', entries }), { headers: Object.assign({ 'Idempotency-Key': key() }, headers) });
  const path = `${base}/changesets/${set.json('data.id')}`;
  const ifMatch = Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, headers);
  http.post(`${path}/submit`, null, { headers: ifMatch });
  http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch });
  const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: 0 }), { headers: Object.assign({ 'Idempotency-Key': key() }, ifMatch) });
  if (published.status !== 201) fail(`publish: HTTP ${published.status} ${published.body}`);

  return { headers, base, images, entries, content: `${BASE_URL}/content/v1/${projectId}/production/posts` };
}

/** Every image of the setup has its versioned URL in the row's media. */
function carriesEveryImage(row, images) {
  const media = row && row.media;
  return media !== undefined && images.every((id) => typeof media[id] === 'object' && /\/[0-9a-f]{64}$/.test(media[id].url) && media[id].width === 400);
}

export default function (data) {
  const slot = exec.scenario.iterationInTest % 10;
  const params = (endpoint) => ({ redirects: 0, tags: { endpoint } });
  if (slot < 5) {
    const res = http.get(`${data.content}?limit=20`, params('list'));
    check(res, {
      'list: 20 posts, each with every image version': (r) => r.status === 200 && r.json('data').length === 20 && r.json('data').every((row) => carriesEveryImage(row, data.images)),
    });
  } else if (slot < 7) {
    const res = http.get(`${data.content}/${data.entries[exec.scenario.iterationInTest % 20]}?generation=1`, params('entry'));
    check(res, { 'entry: every image version': (r) => r.status === 200 && carriesEveryImage(r.json('data'), data.images) });
  } else if (slot < 9) {
    const res = http.post(`${data.base}/collections/posts/entries`, JSON.stringify(post(`Draft ${__VU}-${__ITER}`, data.images)), Object.assign({ headers: Object.assign({ 'Idempotency-Key': key() }, data.headers) }, params('save')));
    check(res, { 'save: 201': (r) => r.status === 201 });
  } else {
    const ghost = `00000000-0000-4000-8000-${(exec.scenario.iterationInTest % 1e12).toString(16).padStart(12, '0')}`;
    const res = http.post(`${data.base}/collections/posts/entries`, JSON.stringify(post('Ghost', [data.images[0], ghost])), Object.assign({ headers: Object.assign({ 'Idempotency-Key': key() }, data.headers) }, params('refused')));
    check(res, {
      'refused: asset_not_found at the image': (r) => r.status === 422 && r.json('error.details.reason') === 'asset_not_found' && r.json('error.details.pointer') === '/body/doc/content/2/attrs/assetId',
    });
  }
}
