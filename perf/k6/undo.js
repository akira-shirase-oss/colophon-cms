// Load scenario for undoing a takedown (D-184). setup() signs in the administrator and creates one
// project per virtual user, each with one published article. Each iteration takes that article
// down (changeset, submit, approve, publish: four requests), then sends two undos of that release
// at once with different keys — exactly one may commit, the other must be refused 409
// `changed_since` (the race the endpoint exists for) — then replays the winning key (it must
// answer the same release, marked replayed) and reads the article anonymously (200 again). Eight
// requests an iteration; every answer is checked. Nobody else publishes in a virtual user's
// project, so it always knows its generation.
// TOTAL counts requests, as elsewhere: a full run of 1,000,000 is 125,000 iterations. RATE counts
// iterations; scripts/load.sh gives this endpoint RATE 25 (200 requests a second) and P99_MS 700,
// publishing's editorial write budget, since every iteration commits two releases.

import http from 'k6/http';
import { check, fail } from 'k6';
import { idempotencyKey } from './lib/idempotency-key.js';
import { raceKeys } from './lib/keys.js';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '25', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '700');
const REQUESTS_PER_ITERATION = 8;

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const scenarios = {
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: Math.ceil(TOTAL / REQUESTS_PER_ITERATION), maxDuration: '30m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);
const projects = SCENARIO === 'steady' ? VUS * 2 : VUS;

// The losing undo's 409 is the expected answer, so it isn't a failed request; every other
// answer must be a success, and the checks say which.
const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
for (const endpoint of ['changeset', 'submit', 'review', 'publish', 'undo', 'replay', 'read']) {
  thresholds[`http_req_duration{endpoint:${endpoint}}`] = ['p(99)<60000'];
}
thresholds['http_req_duration{endpoint:undo}'] = [`p(99)<${P99_MS}`];
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '300s',
  systemTags: ['status', 'method', 'name', 'scenario', 'check', 'error', 'error_code', 'expected_response'],
};

const article = (title) => ({
  title,
  body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: title }] }] } },
});

/** Request, submit, approve and publish `body` on `generation`; the new generation, or fail. */
function release(base, headers, body, generation, key, params) {
  const set = http.post(`${base}/changesets`, JSON.stringify(body), params('changeset', { 'Idempotency-Key': idempotencyKey(`${key}-c`, 'undo') }));
  if (!check(set, { 'changeset 201': (r) => r.status === 201 })) return null;
  const path = `${base}/changesets/${set.json('data.id')}`;
  const ifMatch = { 'If-Match': `"${set.json('data.hash')}"` };
  const submitted = http.post(`${path}/submit`, null, params('submit', ifMatch));
  if (!check(submitted, { 'submit 200': (r) => r.status === 200 })) return null;
  const approved = http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), params('review', ifMatch));
  if (!check(approved, { approved: (r) => r.status === 200 && r.json('data.status.state') === 'approved' })) return null;
  const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: generation }), params('publish', Object.assign({ 'Idempotency-Key': idempotencyKey(`${key}-p`, 'undo') }, ifMatch)));
  if (!check(published, { 'publish 201': (r) => r.status === 201 })) return null;
  return published.json('data.generation');
}

export function setup() {
  const signedIn = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }), { headers: { 'Content-Type': 'application/json' } });
  if (signedIn.status !== 204) fail(`sign-in failed: HTTP ${signedIn.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const headers = {
    Cookie: `__Host-colophon_session=${jar['__Host-colophon_session'][0]}`, 'X-CSRF-Token': jar['__Host-colophon_csrf'][0], Origin: PUBLIC_URL,
    'Content-Type': 'application/json',
  };
  const run = Date.now().toString(36);
  const params = (endpoint, extra) => ({ headers: Object.assign({}, headers, extra || {}), tags: { endpoint: 'setup', name: endpoint } });
  const worlds = [];
  for (let i = 0; i < projects; i += 1) {
    const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-undo-${run}-${i}`, name: `k6 undo ${i}` }), { headers });
    if (project.status !== 201) fail(`project: HTTP ${project.status}`);
    const id = project.json('data.id');
    const base = `${BASE_URL}/api/v1/projects/${id}/environments/production`;
    const entry = http.post(`${base}/collections/articles/entries`, JSON.stringify(article(`Undo ${i}`)), params('entry', { 'Idempotency-Key': idempotencyKey('k6', 'undo', run, i, 'entry') }));
    if (entry.status !== 201) fail(`entry: HTTP ${entry.status} ${entry.body}`);
    const entryId = entry.json('data.id');
    const generation = release(base, headers, { title: `Undo ${i}`, entries: [entryId] }, 0, idempotencyKey('k6', 'undo', run, i, 'first'), params);
    if (generation !== 1) fail(`first release of ${base}: ${generation}`);
    worlds.push({ base, read: `${BASE_URL}/content/v1/${id}/production/articles/${entryId}`, entry: entryId });
  }
  return { headers, worlds, run };
}

// Each virtual user works in its own project, so it knows the generation it builds on.
let generation = null;

export default function (data) {
  const world = data.worlds[(__VU - 1) % data.worlds.length];
  if (generation === null) generation = 1;
  const params = (endpoint, extra) => ({ headers: Object.assign({}, data.headers, extra || {}), redirects: 0, tags: { endpoint, name: endpoint } });
  const key = idempotencyKey('k6', 'undo', data.run, __VU, __ITER);

  const down = release(world.base, data.headers, { title: `Unpublish ${__ITER}`, unpublish: [world.entry] }, generation, key, params);
  if (down === null) return;
  generation = down;

  // Two undos of the one takedown at once, each with its own key: exactly one commits. The keys are built once,
  // so the replay below sends exactly the key the winner sent.
  const keys = raceKeys(key, 2, 'undo');
  const undo = (which) => ({
    method: 'POST',
    url: `${world.base}/releases/${down}/undo`,
    body: null,
    params: Object.assign(params('undo', { 'Idempotency-Key': keys[which - 1] }), { responseCallback: http.expectedStatuses(201, 409) }),
  });
  const [left, right] = http.batch([undo(1), undo(2)]);
  const winners = [left, right].filter((r) => r.status === 201);
  const losers = [left, right].filter((r) => r.status === 409);
  const raced = check([left, right], {
    'one undo commits': () => winners.length === 1 && winners[0].json('data.generation') === down + 1,
    'the other is changed_since': () => losers.length === 1 && losers[0].json('error.details.reason') === 'changed_since',
  });
  if (!raced) return;
  generation = down + 1;
  const winningKey = left.status === 201 ? keys[0] : keys[1];

  const replay = http.post(`${world.base}/releases/${down}/undo`, null, params('replay', { 'Idempotency-Key': winningKey }));
  check(replay, {
    'replay is the same release': (r) => r.status === 201 && r.headers['Idempotent-Replayed'] === 'true' && r.json('data.generation') === down + 1,
  });
  const read = http.get(world.read, { tags: { endpoint: 'read', name: 'read' } });
  check(read, { 'readers have it again': (r) => r.status === 200 });
}
