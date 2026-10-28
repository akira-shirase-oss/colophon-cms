// Load scenario for API tokens and the audit log (step R.4, D-063). setup() signs the owner in
// through Keycloak, makes a project with an article, and two tokens: a read-only one, and a
// custom one that may read only articles' titles. Every measured request then exercises a token or
// the log: six in ten list articles with the read-only token (its hash found through the lookup
// function, then a transaction that re-checks it under a share lock and reads its permissions),
// two read the article with the custom token (its fields projected to the title), one reads a page
// of the audit log as the owner, and one makes a token and revokes it (two writes, each recorded in
// the log, so the log grows through the run). Same settings as roles.js; scripts/load.sh gives
// this endpoint RATE 1000 and P99_MS 100.

import http from 'k6/http';
import { check, fail } from 'k6';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '1000', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '100');

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

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
// Reported per kind of request, so a tail can be traced to its cause; only the overall p99 fails a run.
for (const endpoint of ['list', 'read', 'audit', 'create', 'revoke']) {
  thresholds[`http_req_duration{endpoint:${endpoint}}`] = ['p(99)<60000'];
}
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '120s',
};

/** Signs `username` in with a cookie jar of its own; the headers a browser tab would send. */
function signIn(username) {
  const jar = new http.CookieJar();
  const page = http.get(`${PUBLIC_URL}/auth/login?return_to=/me`, { jar });
  const action = parseHTML(page.body).find('#kc-form-login').attr('action');
  if (!action) fail(`no Keycloak login form for ${username}`);
  const me = http.post(action.replace(/&amp;/g, '&'), { username, password: 'colophon-dev' }, { jar });
  if (me.status !== 200) fail(`sign-in as ${username} failed: HTTP ${me.status}`);
  const cookies = jar.cookiesForURL(PUBLIC_URL);
  return {
    Cookie: `colophon_session=${cookies.colophon_session[0]}`, 'X-CSRF-Token': cookies.colophon_csrf[0], Origin: PUBLIC_URL,
    'Content-Type': 'application/json',
  };
}

function uuid() {
  const hex = '0123456789abcdef';
  let text = '';
  for (let i = 0; i < 32; i += 1) text += hex[Math.floor(Math.random() * 16)];
  return `${text.slice(0, 8)}-${text.slice(8, 12)}-4${text.slice(13, 16)}-8${text.slice(17, 20)}-${text.slice(20)}`;
}

function token(owner, base, body) {
  const made = http.post(`${base}/tokens`, JSON.stringify(body), { headers: owner });
  if (made.status !== 201) fail(`token ${body.name}: HTTP ${made.status} ${made.body}`);
  return made.json('data.secret');
}

export function setup() {
  const owner = signIn('dev-owner');
  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 tokens' }), { headers: owner });
  if (project.status !== 201) fail(`project: HTTP ${project.status}`);
  const base = `${BASE_URL}/api/v1/projects/${project.json('data.id')}`;
  const entries = `${base}/environments/production/collections/articles/entries`;
  const article = http.post(entries, JSON.stringify({
    title: 'Load', body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } },
  }), { headers: Object.assign({ 'Idempotency-Key': uuid() }, owner) });
  if (article.status !== 201) fail(`article: HTTP ${article.status} ${article.body}`);
  const reader = token(owner, base, { name: 'Site build', kind: 'read-only', expires_in_days: 30 });
  const titles = token(owner, base, {
    name: 'Titles only', kind: 'custom', expires_in_days: 30,
    permissions: [{ action: 'content.read', subject: 'articles', fields: ['title'] }],
  });
  return { owner, base, entries, entry: `${entries}/${article.json('data.id')}`, reader, titles };
}

export default function (data) {
  const slot = __ITER % 10;
  if (slot < 6) {
    const res = http.get(data.entries, { headers: { Authorization: `Bearer ${data.reader}` }, redirects: 0, tags: { endpoint: 'list' } });
    check(res, { 'list 200': (r) => r.status === 200 });
  } else if (slot < 8) {
    const res = http.get(data.entry, { headers: { Authorization: `Bearer ${data.titles}` }, redirects: 0, tags: { endpoint: 'read' } });
    check(res, {
      'read 200': (r) => r.status === 200,
      'only the title': (r) => r.json('data.hidden_fields').length > 0 && r.json('data.revision.title') === 'Load',
    });
  } else if (slot === 8) {
    const res = http.get(`${data.base}/audit?limit=20`, { headers: data.owner, redirects: 0, tags: { endpoint: 'audit' } });
    check(res, { 'audit 200': (r) => r.status === 200 });
  } else {
    const made = http.post(`${data.base}/tokens`, JSON.stringify({ name: `vu-${__VU}`, kind: 'read-only', expires_in_days: 1 }), {
      headers: data.owner, redirects: 0, tags: { endpoint: 'create' },
    });
    if (!check(made, { 'create 201': (r) => r.status === 201 })) return;
    const res = http.del(`${data.base}/tokens/${made.json('data.id')}`, null, { headers: data.owner, redirects: 0, tags: { endpoint: 'revoke' } });
    check(res, { 'revoke 200': (r) => r.status === 200 && r.json('data.state') === 'revoked' });
  }
}
