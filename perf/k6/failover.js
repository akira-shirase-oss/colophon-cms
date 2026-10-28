// Failover scenario (milestone S, D-075), run by scripts/failover.sh against the cluster
// (scripts/cluster.sh): while it runs, one server is killed (SIGKILL) or drained (SIGTERM) and
// started again. Every iteration is one logical write: an article titled with its VU and
// iteration, created under one Idempotency-Key and retried with that same key on a network error,
// 502, 503 or 504, up to RETRIES times. A retry of a create that did commit gets the recorded
// answer, not a second article, so afterwards the driver counts the articles in the database
// and their distinct titles against the creates acknowledged here.
//
// Metrics: `acknowledged` (creates answered 201), `first_attempt_failures` (creates whose first
// try failed: in flight when a server died), `retried` (tries after the first) and
// `logical_failures` (creates that never succeeded; must be 0). setup() prints the project id
// as `failover-project <id>` for the driver.

import http from 'k6/http';
import { check, fail, sleep } from 'k6';
import { Counter } from 'k6/metrics';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58405';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58405';
const RATE = parseInt(__ENV.RATE || '200', 10);
const DURATION = __ENV.DURATION || '30s';
const VUS = parseInt(__ENV.VUS || '64', 10);
const RETRIES = parseInt(__ENV.RETRIES || '10', 10);
// `drain`: a SIGTERM must cost no first attempt at all; `kill` allows in-flight failures.
const MODE = __ENV.MODE || 'kill';

const acknowledged = new Counter('acknowledged');
const firstAttemptFailures = new Counter('first_attempt_failures');
const retried = new Counter('retried');
const logicalFailures = new Counter('logical_failures');

const thresholds = {
  logical_failures: ['count==0'],
  checks: ['rate==1'],
};
if (MODE === 'drain') thresholds.first_attempt_failures = ['count==0'];

export const options = {
  scenarios: {
    writes: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: VUS, maxVUs: VUS * 2 },
  },
  thresholds,
  // Failed tries are expected while a server dies; the thresholds above judge the run.
  summaryTrendStats: ['avg', 'med', 'p(90)', 'p(95)', 'p(99)', 'max'],
};

export function setup() {
  const page = http.get(`${PUBLIC_URL}/auth/login?return_to=/me`);
  const action = parseHTML(page.body).find('#kc-form-login').attr('action');
  if (!action) fail('no Keycloak login form');
  const me = http.post(action.replace(/&amp;/g, '&'), { username: 'dev-owner', password: 'colophon-dev' });
  if (me.status !== 200) fail(`sign-in failed: HTTP ${me.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const session = { cookie: `colophon_session=${jar.colophon_session[0]}`, csrf: jar.colophon_csrf[0] };
  const write = { Cookie: session.cookie, 'X-CSRF-Token': session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug: `k6-failover-${Date.now().toString(36)}`, name: 'k6 failover' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const id = project.json('data.id');
  console.log(`failover-project ${id}`);
  return { write, base: `${BASE_URL}/api/v1/projects/${id}/environments/production/collections/articles/entries` };
}

function article(title) {
  return JSON.stringify({
    title,
    body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: title }] }] } },
  });
}

const retryable = (status) => status === 0 || status === 502 || status === 503 || status === 504;

export default function (data) {
  const title = `op ${__VU}-${__ITER}`;
  const headers = Object.assign({ 'Idempotency-Key': `failover-${__VU}-${__ITER}-${Math.random().toString(36).slice(2, 10)}` }, data.write);
  const body = article(title);
  let res = null;
  for (let attempt = 0; attempt <= RETRIES; attempt++) {
    if (attempt > 0) {
      retried.add(1);
      sleep(0.1 + Math.random() * 0.2);
    }
    res = http.post(data.base, body, {
      headers,
      redirects: 0,
      tags: { name: 'entries', attempt: attempt === 0 ? 'first' : 'retry' },
      responseCallback: http.expectedStatuses(201),
    });
    if (!retryable(res.status)) break;
    if (attempt === 0) firstAttemptFailures.add(1);
  }
  const created = check(res, {
    'created once, under its key': (r) => r.status === 201 && r.json('data.content.title') === title,
  });
  if (created) acknowledged.add(1);
  else logicalFailures.add(1);
}
