// Load scenario for mirrored references (D-085): posts name their writer, and the writers'
// `posts` field mirrors it. setup() signs in, defines both collections, and gives each VU two
// writers and a post linked to the first. Each iteration either moves the post to the other
// writer (40%: one forward save plus two mirror revisions, in one transaction), reads both
// writers and checks the mirror holds the post exactly where the link says (30%), or reads the
// post (30%). Requests are named by route, so k6 keeps one series per route.
// scripts/load.sh gives this endpoint RATE 250 and P99_MS 700 (END_GOAL's editorial write
// budget: a move is three revision writes, not one).

import http from 'k6/http';
import { check, fail } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '250', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '700');

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const scenarios = {
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '40m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '120s',
};

const WRITERS = {
  name: 'Writers',
  title_field: 'name',
  fields: [
    { id: 'name', label: 'Name', type: 'text', required: true },
    { id: 'posts', label: 'Posts', type: 'reference', to: 'posts', many: true, max: 10 },
  ],
};
const POSTS = {
  name: 'Posts',
  title_field: 'headline',
  fields: [
    { id: 'headline', label: 'Headline', type: 'text', required: true },
    { id: 'writer', label: 'Writer', type: 'reference', to: 'writers', inverse: 'posts' },
  ],
};

function key() {
  return `k6-${__VU}-${__ITER}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export function setup() {
  const signedIn = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }), { headers: { 'Content-Type': 'application/json' } });
  if (signedIn.status !== 204) fail(`sign-in failed: HTTP ${signedIn.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const session = { cookie: `colophon_session=${jar.colophon_session[0]}`, csrf: jar.colophon_csrf[0] };
  const write = { Cookie: session.cookie, 'X-CSRF-Token': session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };

  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 inverses' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const env = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;
  for (const [type, definition] of [['writers', WRITERS], ['posts', POSTS]]) {
    const made = http.post(`${env}/content-types`, JSON.stringify({ slug: type, definition }), {
      headers: Object.assign({ 'Idempotency-Key': `k6-type-${slug}-${type}` }, write),
    });
    if (made.status !== 201) fail(`content type ${type}: HTTP ${made.status} ${made.body}`);
  }
  const writersBase = `${env}/collections/writers/entries`;
  const postsBase = `${env}/collections/posts/entries`;

  const vus = [];
  for (let i = 0; i < VUS; i++) {
    const writers = [];
    for (const name of ['Ada', 'Grace']) {
      const made = http.post(writersBase, JSON.stringify({ name: `${name} ${i}` }), {
        headers: Object.assign({ 'Idempotency-Key': `k6-setup-${slug}-${i}-${name}` }, write),
      });
      if (made.status !== 201) fail(`writer: HTTP ${made.status} ${made.body}`);
      writers.push(made.json('data.id'));
    }
    const post = http.post(postsBase, JSON.stringify({ headline: `Compilers ${i}`, writer: writers[0] }), {
      headers: Object.assign({ 'Idempotency-Key': `k6-setup-${slug}-${i}-post` }, write),
    });
    if (post.status !== 201) fail(`post: HTTP ${post.status} ${post.body}`);
    vus.push({ post: post.json('data.id'), etag: post.headers['Etag'], writers });
  }
  return { session, writersBase, postsBase, vus };
}

let etag = null;
let linked = 0; // which of the VU's two writers the post names, 0 or 1

export default function (data) {
  const mine = data.vus[(__VU - 1) % data.vus.length];
  if (etag === null) etag = mine.etag;
  const read = { headers: { Cookie: data.session.cookie }, redirects: 0 };
  const roll = __ITER % 10;

  if (roll < 4) {
    const other = 1 - linked;
    const headers = {
      Cookie: data.session.cookie, 'X-CSRF-Token': data.session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json',
      'If-Match': etag, 'Idempotency-Key': key(),
    };
    const res = http.put(`${data.postsBase}/${mine.post}`, JSON.stringify({ headline: 'Compilers', writer: mine.writers[other] }), {
      headers, redirects: 0, tags: { endpoint: 'move', name: 'posts/{entry}' },
    });
    const saved = check(res, {
      'move 200': (r) => r.status === 200,
      'move gives a new ETag': (r) => typeof r.headers['Etag'] === 'string' && r.headers['Etag'] !== etag,
    });
    if (saved) {
      etag = res.headers['Etag'];
      linked = other;
    }
  } else if (roll < 7) {
    for (const which of [0, 1]) {
      const res = http.get(`${data.writersBase}/${mine.writers[which]}`, Object.assign({ tags: { endpoint: 'mirror', name: 'writers/{entry}' } }, read));
      const holds = (r) => ((r.json('data.content.posts') || []).includes(mine.post));
      check(res, {
        'writer 200': (r) => r.status === 200,
        'mirror holds the post exactly where the link says': (r) => holds(r) === (which === linked),
      });
    }
  } else {
    const res = http.get(`${data.postsBase}/${mine.post}`, Object.assign({ tags: { endpoint: 'read', name: 'posts/{entry}' } }, read));
    check(res, {
      'read 200': (r) => r.status === 200,
      'read returns the last accepted save': (r) => r.headers['Etag'] === etag,
    });
  }
}
