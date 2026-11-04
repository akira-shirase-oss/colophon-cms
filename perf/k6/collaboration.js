// Load scenario for comments and tasks (E1, D-102). setup() signs the owner in through Keycloak,
// creates a project and one article per virtual user that can exist. Every measured request is
// the owner's: four in ten list an entry's comments and one its tasks, two write a comment on the
// article's current revision, one resolves or reopens a comment on the etag it was read at, one
// creates a task on its entry, and one changes a task's title, assignee or status on its etag —
// so every write pays the policy check, the idempotency claim, the audit record and the etag's
// hash. Same settings as changesets.js; scripts/load.sh gives this endpoint RATE 1000 and
// P99_MS 100.

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

// One article per virtual user that can exist; steady runs grow to twice VUS.
const MAX_VUS = SCENARIO === 'steady' ? VUS * 2 : VUS;
const scenarios = {
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '20m' },
  steady: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: MAX_VUS },
};
if (!(SCENARIO in scenarios)) throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}`);

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
// Reported per kind of request, so a tail can be traced to its cause; only the overall p99 fails a run.
for (const endpoint of ['list', 'comment', 'answer', 'tasks', 'task', 'patch']) {
  thresholds[`http_req_duration{endpoint:${endpoint}}`] = ['p(99)<60000'];
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

const uuid = () => 'xxxxxxxx-xxxx-4xxx-8xxx-xxxxxxxxxxxx'.replace(/x/g, () => Math.floor(Math.random() * 16).toString(16));

export function setup() {
  const owner = signIn('dev-owner');
  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 collaboration' }), { headers: owner });
  if (project.status !== 201) fail(`project: HTTP ${project.status}`);
  const base = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;
  const doc = { type: 'doc', content: [{ type: 'paragraph' }] };
  const entries = [];
  for (let vu = 1; vu <= MAX_VUS; vu += 1) {
    const entry = http.post(`${base}/collections/articles/entries`,
      JSON.stringify({ title: `Note ${vu}`, body: { schema: 'colophon/rich-text', version: 1, doc } }),
      { headers: Object.assign({ 'Idempotency-Key': uuid() }, owner) });
    if (entry.status !== 201) fail(`entry ${vu}: HTTP ${entry.status}`);
    entries.push({ id: entry.json('data.id'), revision: entry.json('data.revision.id') });
  }
  return { owner, base, entries };
}

// This virtual user's comments and tasks, with the etag each one was last read or written at:
// resolve/reopen and PATCH bind it, so each one moves only the state it saw.
const comments = [];
const tasks = [];

export default function (data) {
  const params = { headers: data.owner, redirects: 0 };
  const mine = data.entries[(__VU - 1) % data.entries.length];
  const entryPath = `${data.base}/collections/articles/entries/${mine.id}`;
  const write = () => ({ headers: Object.assign({ 'Idempotency-Key': uuid() }, data.owner), redirects: 0 });
  const slot = __ITER % 10;
  if (slot < 4) {
    const res = http.get(`${entryPath}/comments?limit=100`, Object.assign({ tags: { endpoint: 'list' } }, params));
    check(res, { 'list 200': (r) => r.status === 200 });
  } else if (slot === 4) {
    const res = http.get(`${data.base}/tasks?entry=${mine.id}&limit=100`, Object.assign({ tags: { endpoint: 'tasks' } }, params));
    check(res, { 'tasks 200': (r) => r.status === 200 });
  } else if (slot < 7 || comments.length === 0) {
    const res = http.post(`${entryPath}/comments`, JSON.stringify({ revision: mine.revision, body: `Check the opening of note ${__VU} (${__ITER})` }),
      { headers: write().headers, tags: { endpoint: 'comment' } });
    if (check(res, { 'comment 201': (r) => r.status === 201 })) {
      comments.push({ id: res.json('data.id'), etag: res.json('data.etag'), resolved: false });
      if (comments.length > 50) comments.shift();
    }
  } else if (slot === 7) {
    // Resolve an open comment, or reopen a resolved one; either on the etag it was read at.
    const open = comments.find((comment) => !comment.resolved);
    const answered = comments.find((comment) => comment.resolved);
    const target = open ?? answered;
    const resolve = target === open;
    const res = http.post(`${entryPath}/comments/${target.id}/${resolve ? 'resolve' : 'reopen'}`, null,
      { headers: Object.assign({ 'If-Match': `"${target.etag}"` }, write().headers), tags: { endpoint: 'answer' } });
    if (check(res, { 'answer 200': (r) => r.status === 200 })) {
      target.etag = res.json('data.etag');
      target.resolved = resolve;
    }
  } else if (slot === 8 || tasks.length === 0) {
    const res = http.post(`${data.base}/tasks`, JSON.stringify({ entry: mine.id, title: `Finish note ${__VU}` }),
      { headers: write().headers, tags: { endpoint: 'task' } });
    if (check(res, { 'task 201': (r) => r.status === 201 })) {
      tasks.push({ id: res.json('data.id'), etag: res.json('data.etag'), status: 'open' });
      if (tasks.length > 20) tasks.shift();
    }
  } else {
    // Between done and open, on the etag the task was last read at.
    const task = tasks[__ITER % tasks.length];
    const status = task.status === 'done' ? 'open' : 'done';
    const res = http.patch(`${data.base}/tasks/${task.id}`, JSON.stringify({ status }),
      { headers: Object.assign({ 'If-Match': `"${task.etag}"` }, write().headers), tags: { endpoint: 'patch' } });
    if (check(res, { 'patch 200': (r) => r.status === 200 })) {
      task.etag = res.json('data.etag');
      task.status = status;
    }
  }
}
