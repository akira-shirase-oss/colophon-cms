// Load scenario for GraphQL reads (DX.10, D-115): each VU owns a posts collection it
// seeds — a staff writer and a page of posts by her — then every iteration posts a
// bounded `entries` read (a `where`/`orderBy`/`first` on indexed fields, `title` and a
// nested `author { name }` on each node, `pageInfo` and `applied`) or, one iteration in
// ten, an `entry(id)` read of a seeded post. Requests are named by operation, so k6
// keeps one series per operation.
// scripts/load.sh gives this endpoint RATE 500 and P99_MS 250 — entries' read arm: the
// schema is built per request but the bounded document and its one-transaction run are
// the same pipeline `entries/query` takes.

import http from 'k6/http';
import { check, fail } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '500', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '250');

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

const STAFF = {
  name: 'Staff',
  title_field: 'name',
  fields: [{ id: 'name', label: 'Name', type: 'text', required: true }],
};
const POSTS = {
  name: 'Posts',
  title_field: 'headline',
  fields: [
    { id: 'headline', label: 'Headline', type: 'text', required: true, index: true },
    { id: 'rating', label: 'Rating', type: 'number', integer: true, min: 0, max: 5, index: true },
    { id: 'section', label: 'Section', type: 'enum', values: ['News', 'Guides'], index: true },
    { id: 'author', label: 'Author', type: 'reference', to: 'staff' },
  ],
};

function write_headers(session) {
  return { Cookie: session.cookie, 'X-CSRF-Token': session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };
}

export function setup() {
  const signedIn = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }), { headers: { 'Content-Type': 'application/json' } });
  if (signedIn.status !== 204) fail(`sign-in failed: HTTP ${signedIn.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const session = { cookie: `colophon_session=${jar.colophon_session[0]}`, csrf: jar.colophon_csrf[0] };
  const write = write_headers(session);

  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 graphql' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const env = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;

  const vus = [];
  for (let i = 0; i < VUS; i++) {
    for (const [collection, definition] of [[`staff${i}`, STAFF], [`posts${i}`, POSTS]]) {
      const made = http.post(`${env}/content-types`, JSON.stringify({ slug: collection, definition: { ...definition, fields: definition.fields.map((f) => (f.to ? { ...f, to: `staff${i}` } : f)) } }), {
        headers: Object.assign({ 'Idempotency-Key': `k6-type-${slug}-${collection}` }, write),
      });
      if (made.status !== 201) fail(`content type ${collection}: HTTP ${made.status} ${made.body}`);
    }
    const writer = http.post(`${env}/collections/staff${i}/entries`, JSON.stringify({ name: `Writer ${i}` }), {
      headers: Object.assign({ 'Idempotency-Key': `k6-staff-${slug}-${i}` }, write),
    });
    if (writer.status !== 201) fail(`staff ${i}: HTTP ${writer.status} ${writer.body}`);
    const ada = writer.json('data.id');
    const entries = [];
    for (let p = 0; p < 5; p++) {
      const post = http.post(`${env}/collections/posts${i}/entries`, JSON.stringify({
        headline: `Post ${i}-${p}`, rating: p % 6, section: p % 2 === 0 ? 'News' : 'Guides', author: ada,
      }), { headers: Object.assign({ 'Idempotency-Key': `k6-post-${slug}-${i}-${p}` }, write) });
      if (post.status !== 201) fail(`post ${i}/${p}: HTTP ${post.status} ${post.body}`);
      entries.push(post.json('data.id'));
    }
    vus.push({ graphql: `${env}/collections/posts${i}/graphql`, entries });
  }
  return { session, vus };
}

const DOCUMENT = '{ entries(where: {section: {eq: "News"}, rating: {gte: 1}}, orderBy: [{field: "rating", dir: DESC}], first: 3) { '
  + 'nodes { title headline rating author { name } } pageInfo { endCursor hasNextPage } applied { queryHash } } }';

export default function (data) {
  const vu = data.vus[(__VU - 1) % data.vus.length];
  const read = { Cookie: data.session.cookie, 'X-CSRF-Token': data.session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };
  if (__ITER % 10 === 9) {
    const res = http.post(vu.graphql, JSON.stringify({ query: `{ entry(id: "${vu.entries[__ITER % vu.entries.length]}") { title rating } }` }), {
      headers: read,
      tags: { endpoint: 'graphql-entry', name: 'graphql-entry' },
    });
    check(res, {
      'entry 200': (r) => r.status === 200,
      'entry answers': (r) => typeof r.json('data.entry.title') === 'string',
    });
  } else {
    const res = http.post(vu.graphql, JSON.stringify({ query: DOCUMENT }), {
      headers: read,
      tags: { endpoint: 'graphql-entries', name: 'graphql-entries' },
    });
    check(res, {
      'entries 200': (r) => r.status === 200,
      'nodes answered': (r) => Array.isArray(r.json('data.entries.nodes')),
      'author expanded': (r) => r.json('data.entries.nodes.0.author.name') === `Writer ${(__VU - 1) % data.vus.length}`,
      'applied hash': (r) => String(r.json('data.entries.applied.queryHash')).startsWith('sha256:'),
    });
  }
}
