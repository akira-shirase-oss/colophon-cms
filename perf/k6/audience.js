// Load scenario for the audience (D-137): a project's visitors signing up and in, and
// reading delivery as who they are. setup() signs in the administrator, creates a project
// with two audience roles — `member` open to signup and `vip` closed — publishes one article
// per gate (public, authenticated, role:member, role:vip), and signs up USERS visitors to
// `member`, keeping each one's session token.
//
// Every measured request is a visitor's, never staff's: most are member delivery lists on a
// live bearer token — each pays the `colophon_audience_session` lookup and reads Postgres as
// `colophon_delivery` gated by `app.audience_role`, answered `private` and never touching the
// anonymous cache — some are anonymous lists served by the generation cache, some are `me`,
// one in eight signs in (Argon2id on the blocking pool), a fiftieth requests a magic link
// (D-140; the consume arm runs only when the server hands the link back, the development
// mailer's COLOPHON_DEV_MAIL), and a hundredth signs up. Signups mint a new
// account each time; sign-ins refresh the VU's own token, so a member read always carries a
// live one (MAX_SESSIONS evicts only the oldest).
// scripts/load.sh gives this endpoint RATE 100 and P99_MS 500: one request in seven pays
// Argon2id, so its steady rate is a fraction of delivery's and its p99 is the hashing queue.
// anon_cache_hits is the share of anonymous lists answered without a database read of their
// own; CACHE=off skips that threshold.

import http from 'k6/http';
import { check, fail } from 'k6';
import { Rate } from 'k6/metrics';

const anonCacheHits = new Rate('anon_cache_hits');

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '100', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '500');
// One visitor per virtual user k6 may ever run, so no two VUs race one user's sessions.
const USERS = parseInt(__ENV.USERS || '512', 10);

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const scenarios = {
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '30m' },
  steady: {
    executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION,
    preAllocatedVUs: VUS, maxVUs: USERS,
  },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
// Reported per kind of request, so a tail can be traced to its cause; only the overall p99 fails a run.
for (const endpoint of ['member_list', 'anon_list', 'me', 'signin', 'signup', 'magic_request', 'magic_consume']) {
  thresholds[`http_req_duration{endpoint:${endpoint}}`] = ['p(99)<60000'];
}
if (__ENV.CACHE !== 'off') {
  thresholds.anon_cache_hits = ['rate>0.99'];
}
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '300s',
};

const PASSWORD = 'a-long-enough-password';

const body = (title, text) => ({
  title,
  body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] } },
});

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
  const key = () => `k6-audience-${run}-${(n += 1)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-${run}`, name: 'k6 audience' }), { headers });
  if (project.status !== 201) fail(`project: HTTP ${project.status}`);
  const projectId = project.json('data.id');
  const base = `${BASE_URL}/api/v1/projects/${projectId}`;
  const staff = `${base}/audience`;
  for (const [name, open] of [['member', true], ['vip', false]]) {
    const role = http.post(`${staff}/roles`, JSON.stringify({ name, self_assign: open }), { headers: Object.assign({ 'Idempotency-Key': key() }, headers) });
    if (role.status !== 201) fail(`role ${name}: HTTP ${role.status} ${role.body}`);
  }
  const entries = `${base}/environments/production/collections/articles/entries`;
  const ids = [];
  for (const [title, gate] of [['For everyone', 'public'], ['For accounts', 'authenticated'], ['For members', 'role:member'], ['For vips', 'role:vip']]) {
    const created = http.post(`${entries}?visibility=${encodeURIComponent(gate)}`, JSON.stringify(body(title, 'text')), { headers: Object.assign({ 'Idempotency-Key': key() }, headers) });
    if (created.status !== 201) fail(`entry ${title}: HTTP ${created.status} ${created.body}`);
    ids.push(created.json('data.id'));
  }
  const set = http.post(`${base}/environments/production/changesets`, JSON.stringify({ title: 'Gates', entries: ids }), { headers: Object.assign({ 'Idempotency-Key': key() }, headers) });
  const path = `${base}/environments/production/changesets/${set.json('data.id')}`;
  const ifMatch = Object.assign({ 'If-Match': `"${set.json('data.hash')}"` }, headers);
  http.post(`${path}/submit`, null, { headers: ifMatch });
  http.post(`${path}/reviews`, JSON.stringify({ decision: 'approve' }), { headers: ifMatch });
  const published = http.post(`${path}/publish`, JSON.stringify({ base_generation: 0 }), { headers: Object.assign({ 'Idempotency-Key': key() }, ifMatch) });
  if (published.status !== 201) fail(`publish: HTTP ${published.status} ${published.body}`);

  // The audience itself: public signups, batched — each returns its first session token.
  const users = [];
  const audience = `${BASE_URL}/audience/v1/${projectId}`;
  for (let first = 0; first < USERS; first += 32) {
    const batch = [];
    for (let i = first; i < Math.min(first + 32, USERS); i += 1) {
      batch.push(['POST', `${audience}/signup`, JSON.stringify({ email: `k6-aud-${run}-${i}@example.test`, password: PASSWORD }), { headers: { 'Content-Type': 'application/json' } }]);
    }
    for (const res of http.batch(batch)) {
      if (res.status !== 201) fail(`signup: HTTP ${res.status} ${res.body}`);
      users.push({ email: res.json('data.user.email'), token: res.json('data.session') });
    }
  }
  return { audience, list: `${BASE_URL}/content/v1/${projectId}/production/articles`, users };
}

// This VU's visitor: set on its first iteration, its token refreshed by every sign-in it
// makes, so member reads never carry a session MAX_SESSIONS already pushed out.
let mine = null;

export default function (data) {
  if (mine === null) {
    mine = data.users[(__VU - 1) % data.users.length];
  }
  const params = (endpoint, headers) => ({ redirects: 0, tags: { endpoint }, headers });
  const bearer = { Authorization: `Bearer ${mine.token}` };
  const slot = __ITER % 100;
  let res;
  if (slot < 65) {
    res = http.get(data.list, params('member_list', bearer));
    check(res, {
      'member list: the three gates a member reads, never cached': (r) =>
        r.status === 200
        && r.headers['Colophon-Cache'] === 'bypass'
        && r.headers['Cache-Control'] === 'private, no-cache'
        && r.json('data').length === 3,
    });
  } else if (slot < 80) {
    res = http.get(data.list, params('anon_list'));
    check(res, {
      'anonymous list: the public row alone': (r) => r.status === 200 && r.json('data').length === 1,
    });
    const served = res.headers['Colophon-Cache'];
    anonCacheHits.add(served === 'hit' || served === 'coalesced');
  } else if (slot < 85) {
    res = http.get(`${data.audience}/me`, params('me', bearer));
    check(res, { 'me 200': (r) => r.status === 200 && r.json('data.email') === mine.email });
  } else if (slot < 97) {
    res = http.post(`${data.audience}/signin`, JSON.stringify({ email: mine.email, password: PASSWORD }), params('signin', { 'Content-Type': 'application/json' }));
    check(res, { 'signin 200': (r) => r.status === 200 && typeof r.json('data.session') === 'string' });
    if (res.status === 200) mine.token = res.json('data.session');
  } else if (slot < 99) {
    // A magic-link request always answers the same 202; spending it is possible only where
    // the server mails development links back (COLOPHON_DEV_MAIL, loopback-only).
    res = http.post(`${data.audience}/magic-link`, JSON.stringify({ email: mine.email, return_to: 'http://site.test/magic' }), params('magic_request', { 'Content-Type': 'application/json' }));
    check(res, { 'magic request 202 sent': (r) => r.status === 202 && r.json('data.sent') === true });
    const link = res.status === 202 ? res.json('data.dev_link') : undefined;
    if (typeof link === 'string') {
      const consumed = http.post(`${data.audience}/magic-link/${link.split('token=').pop()}`, null, params('magic_consume', { 'Content-Type': 'application/json' }));
      check(consumed, { 'magic consume 200': (r) => r.status === 200 && typeof r.json('data.session') === 'string' });
      if (consumed.status === 200) mine.token = consumed.json('data.session');
    }
  } else {
    const email = `k6-aud-new-${__VU}-${__ITER}@example.test`;
    res = http.post(`${data.audience}/signup`, JSON.stringify({ email, password: PASSWORD }), params('signup', { 'Content-Type': 'application/json' }));
    check(res, { 'signup 201 as member': (r) => r.status === 201 && r.json('data.user.role') === 'member' });
  }
}
