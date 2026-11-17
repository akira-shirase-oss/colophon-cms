// Load scenario for the quick start (D-172, D-173): the container image behind its compose
// file, as a newcomer runs it, with the install's own defaults — rate limits included. Run by
// scripts/quickstart.sh --load (a smoke on every CI push); BASE_URL points at the container.
//
// SCENARIO=steady  open model: RATE requests a second for DURATION, whatever the latency. The
//                  default 200 a second stays under the per-address limit (300 a second), since
//                  every request comes from one address; a heavier run raises the limits with a
//                  compose override first, as scripts/load.sh does for the native server.
// SCENARIO=throughput  closed loop: VUS users, TOTAL requests, back to back. Needs the limits
//                  raised (COLOPHON_RATE_LIMIT_PER_ADDRESS=off) or it measures 429s.
//
// Each request is one of what a first visit makes: the Studio's page, the installation check
// it starts with, readiness (what `colophon doctor` and the image's health check read), and
// the build's version. Every response is checked.

import http from 'k6/http';
import { check } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080';
const SCENARIO = __ENV.SCENARIO || 'steady';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '32', 10);
const RATE = parseInt(__ENV.RATE || '200', 10);
const DURATION = __ENV.DURATION || '30s';
const P99_MS = parseFloat(__ENV.P99_MS || '50');

const scenarios = {
  steady: {
    executor: 'constant-arrival-rate',
    rate: RATE,
    timeUnit: '1s',
    duration: DURATION,
    preAllocatedVUs: VUS,
    maxVUs: VUS * 2,
  },
  throughput: {
    executor: 'shared-iterations',
    vus: VUS,
    iterations: TOTAL,
    maxDuration: '15m',
  },
};

if (!(SCENARIO in scenarios)) {
  throw new Error(`SCENARIO must be one of ${Object.keys(scenarios).join(', ')}, got ${SCENARIO}`);
}

export const options = {
  scenarios: { quickstart: scenarios[SCENARIO] },
  thresholds: {
    http_req_failed: ['rate==0'],
    checks: ['rate==1'],
    http_req_duration: [`p(99)<${P99_MS}`],
  },
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'max'],
};

const requests = [
  {
    path: '/studio/',
    ok: (r) => r.status === 200 && String(r.body).includes('<html'),
  },
  {
    path: '/api/v1/installation',
    ok: (r) => r.status === 200 && r.json('data.install_kind') === 'container',
  },
  {
    path: '/readyz',
    ok: (r) => r.status === 200 && r.json('status') === 'ready',
  },
  {
    path: '/version',
    ok: (r) => r.status === 200 && typeof r.json('version') === 'string',
  },
];

export default function () {
  const request = requests[(__ITER + __VU) % requests.length];
  const response = http.get(`${BASE_URL}${request.path}`, { tags: { path: request.path } });
  check(response, { [`${request.path} answers`]: request.ok });
}
