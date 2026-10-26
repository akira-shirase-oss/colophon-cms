// Load scenario for the signed-in path (step 1.1): every request is authenticated by session
// cookie, so each one looks the session up in Postgres and reads through row-level security.
// setup() signs in once through the real OpenID Connect flow, Keycloak's login form included.
// Requests alternate between GET /me and GET /api/v1/projects. Same settings as health.js,
// with the defaults scripts/load.sh gives this endpoint: RATE 2000 and P99_MS 100.

import http from 'k6/http';
import { check, fail } from 'k6';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
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
  throughput: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '15m' },
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
  // setup() follows the sign-in redirects itself; the measured requests never redirect.
  setupTimeout: '60s',
};

export function setup() {
  // Start the sign-in on the public origin (the registered redirect URI), follow to Keycloak.
  const page = http.get(`${PUBLIC_URL}/auth/login?return_to=/me`);
  const action = parseHTML(page.body).find('#kc-form-login').attr('action');
  if (!action) fail('no Keycloak login form');
  // Posting the credentials follows Keycloak's redirect to the callback, then to /me.
  const me = http.post(action.replace(/&amp;/g, '&'), { username: 'dev-owner', password: 'colophon-dev' });
  if (me.status !== 200 || me.json('data.display_name') !== 'Dev Owner') fail(`sign-in failed: HTTP ${me.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  return { cookie: `colophon_session=${jar.colophon_session[0]}` };
}

export default function (data) {
  const params = { headers: { Cookie: data.cookie }, redirects: 0 };
  if (__ITER % 2 === 0) {
    const res = http.get(`${BASE_URL}/me`, Object.assign({ tags: { endpoint: 'me' } }, params));
    check(res, {
      'me 200': (r) => r.status === 200,
      'me names the person': (r) => r.body.indexOf('"display_name":"Dev Owner"') !== -1,
    });
  } else {
    const res = http.get(`${BASE_URL}/api/v1/projects`, Object.assign({ tags: { endpoint: 'projects' } }, params));
    check(res, {
      'projects 200': (r) => r.status === 200,
      'projects listed': (r) => r.body.indexOf('{"data":[') === 0,
    });
  }
}
