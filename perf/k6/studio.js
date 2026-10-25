// Load scenario for the Studio's static files (step 1.3): colophon-server serving the built
// single-page app at /studio. Half the requests load a page (the root or a deep route, both
// answered with the app's fallback page), half load a content-hashed asset found in that page.
// Same SCENARIO, TOTAL, VUS, RATE, DURATION and P99_MS settings as health.js; scripts/load.sh
// gives this endpoint RATE 5000.

import http from 'k6/http';
import { check, fail } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '20000', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '25');

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
};

export function setup() {
  const page = http.get(`${BASE_URL}/studio/`);
  if (page.status !== 200) fail(`the Studio isn't served: HTTP ${page.status}`);
  const assets = [...new Set(page.body.match(/\/studio\/_app\/immutable\/[^"']+\.(?:js|css)/g) || [])];
  if (assets.length === 0) fail('no hashed assets in the Studio page');
  return { assets };
}

const pages = ['/studio/', '/studio/projects/0192/articles/0193'];

export default function (data) {
  if (__ITER % 2 === 0) {
    const res = http.get(`${BASE_URL}${pages[__ITER % 4 === 0 ? 0 : 1]}`, { tags: { endpoint: 'page' } });
    check(res, {
      'page 200': (r) => r.status === 200,
      'page revalidates': (r) => r.headers['Cache-Control'] === 'no-cache',
      'page is the app': (r) => r.body.indexOf('/studio/_app/immutable/') !== -1,
    });
  } else {
    const asset = data.assets[__ITER % data.assets.length];
    const res = http.get(`${BASE_URL}${asset}`, { tags: { endpoint: 'asset' } });
    check(res, {
      'asset 200': (r) => r.status === 200,
      'asset cached for a year': (r) => r.headers['Cache-Control'] === 'public, max-age=31536000, immutable',
    });
  }
}
