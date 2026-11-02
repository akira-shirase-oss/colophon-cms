// Load scenario for collection transfer (D-086): each VU owns a notes collection it seeds
// by import, then every iteration either exports the collection's drafts as JSONL (90%:
// one read of every draft revision's content, up to the 10,000-entry cap) or imports a
// three-line JSONL file (10%, up to ten imports: one transaction validating and writing
// three revisions). Requests are named by route, so k6 keeps one series per route.
// scripts/load.sh gives this endpoint RATE 250 and P99_MS 700 (END_GOAL's editorial write
// budget: an import is several revision writes, not one).

import http from 'k6/http';
import { check, fail } from 'k6';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '250', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '700');

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

const NOTES = {
  name: 'Notes',
  title_field: 'title',
  fields: [
    { id: 'title', label: 'Title', type: 'text', required: true },
    { id: 'note', label: 'Note', type: 'text' },
  ],
};

function key() {
  return `k6-${__VU}-${__ITER}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export function setup() {
  const page = http.get(`${PUBLIC_URL}/auth/login?return_to=/me`);
  const action = parseHTML(page.body).find('#kc-form-login').attr('action');
  if (!action) fail('no Keycloak login form');
  const me = http.post(action.replace(/&amp;/g, '&'), { username: 'dev-owner', password: 'colophon-dev' });
  if (me.status !== 200) fail(`sign-in failed: HTTP ${me.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const session = { cookie: `colophon_session=${jar.colophon_session[0]}`, csrf: jar.colophon_csrf[0] };
  const write = { Cookie: session.cookie, 'X-CSRF-Token': session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };

  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 transfer' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const env = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;

  const vus = [];
  for (let i = 0; i < VUS; i++) {
    const collection = `notes${i}`;
    const made = http.post(`${env}/content-types`, JSON.stringify({ slug: collection, definition: NOTES }), {
      headers: Object.assign({ 'Idempotency-Key': `k6-type-${slug}-${i}` }, write),
    });
    if (made.status !== 201) fail(`content type ${collection}: HTTP ${made.status} ${made.body}`);
    const seed = `{"title":"Seed ${i}","note":"first"}\n{"title":"Also ${i}"}\n`;
    const imported = http.post(`${env}/collections/${collection}/import`, seed, {
      headers: Object.assign({}, write, { 'Content-Type': 'application/jsonlines', 'Idempotency-Key': `k6-seed-${slug}-${i}` }),
    });
    if (imported.status !== 201) fail(`seed import: HTTP ${imported.status} ${imported.body}`);
    vus.push(`${env}/collections/${collection}`);
  }
  return { session, vus };
}

let imports = 0; // how many of this VU's ten imports have run

export default function (data) {
  const collection = data.vus[(__VU - 1) % data.vus.length];
  if (imports < 10 && __ITER % 10 === 0) {
    const file = `{"title":"Batch ${imports}","note":"k6"}\n{"type":"api::note.note","data":{"title":"Strapi ${imports}"}}\n{"title":"Array ${imports}","note":"three"}\n`;
    const res = http.post(`${collection}/import`, file, {
      headers: { Cookie: data.session.cookie, 'X-CSRF-Token': data.session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/jsonlines', 'Idempotency-Key': key() },
      redirects: 0,
      tags: { endpoint: 'import', name: 'import' },
    });
    if (check(res, { 'import 201': (r) => r.status === 201, 'imports the file': (r) => r.json('data.imported') === 3 })) imports++;
  } else {
    const res = http.get(`${collection}/export`, {
      headers: { Cookie: data.session.cookie },
      redirects: 0,
      tags: { endpoint: 'export', name: 'export' },
    });
    check(res, {
      'export 200': (r) => r.status === 200,
      'export is JSONL': (r) => String(r.headers['Content-Type']).startsWith('application/jsonlines'),
      'export lines parse': (r) => r.body.trim().split('\n').every((line) => {
        try { JSON.parse(line); return true; } catch { return false; }
      }),
    });
  }
}
