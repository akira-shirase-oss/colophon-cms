// The allocator measurement's two loads (D-176), run by scripts/allocator-bench.sh against a
// server with no database and rate limits off:
//
// MODE=rich-text  the realistic blog post through /api/v1/rich-text/validate: parse, validate
//                 and re-serialize JSON, allocation-heavy, and it must come back canonical.
// MODE=livez      /livez only: almost no allocation, the control.
//
// Closed loop: VUS users send TOTAL requests back to back. Every response is checked; there is
// no latency threshold, since this measures rather than gates.
import http from 'k6/http';
import { check } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58032';
const MODE = __ENV.MODE || 'rich-text';
const TOTAL = parseInt(__ENV.TOTAL || '200000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);

const post = JSON.parse(open('../../fixtures/rich-text/v1/valid/12-blog-post.json'));
const BODY = JSON.stringify(post.value);
const CANONICAL = `"value":${BODY}`;
const PARAMS = { headers: { 'Content-Type': 'application/json' }, tags: { mode: MODE } };

if (MODE !== 'rich-text' && MODE !== 'livez') {
  throw new Error(`MODE must be rich-text or livez, got ${MODE}`);
}

export const options = {
  scenarios: { load: { executor: 'shared-iterations', vus: VUS, iterations: TOTAL, maxDuration: '10m' } },
  thresholds: { http_req_failed: ['rate==0'], checks: ['rate==1'] },
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'max'],
};

export default function () {
  if (MODE === 'rich-text') {
    const res = http.post(`${BASE_URL}/api/v1/rich-text/validate`, BODY, PARAMS);
    check(res, { 'valid and canonical': (r) => r.status === 200 && String(r.body).includes(CANONICAL) });
  } else {
    const res = http.get(`${BASE_URL}/livez`, PARAMS);
    check(res, { live: (r) => r.status === 200 });
  }
}
