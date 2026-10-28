// Load scenario for content modelling (steps M.1 to M.4, D-064 to D-066, D-069 to D-072): pages
// built from structures, with references. setup() signs in, defines `people` (a unique name, a
// role whose default is `member`, and an editor date only editors may have) and `pages` (a
// unique title, so every save replaces its page's claim; a hero object, tags each matched against
// a pattern, a list of questions, and blocks whose quotes name a person), makes 20 people and
// gives each VU a page of its own. Each VU reads its page (50%; the ETag must be its last accepted
// save), saves new structures naming two random people (20%; each save checks its references in
// the database and keeps them), tries a quote naming nobody and must get 422 reference_not_found
// at /sections/1/by (10%), reads a person, which lists what references them (10%), lists pages
// (5%), or creates a person with a name of its own and no role, which must come back `member`
// (5%). Requests are named by route, so k6 keeps one series per route. scripts/load.sh gives this
// endpoint RATE 500 and P99_MS 250.

import http from 'k6/http';
import { check, fail } from 'k6';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '500', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '250');
const PEOPLE = 20;

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
const PAGES = SCENARIO === 'steady' ? VUS * 2 : VUS;

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`] };
for (const endpoint of ['read', 'save', 'broken', 'person', 'list', 'create']) {
  thresholds[`http_req_duration{endpoint:${endpoint}}`] = ['p(99)<60000'];
}
if (SCENARIO === 'steady') {
  thresholds.dropped_iterations = [`count<=${Math.floor(RATE * durationSeconds(DURATION) * 0.0001)}`];
}

export const options = {
  scenarios: { [SCENARIO]: scenarios[SCENARIO] },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '180s',
};

const PEOPLE_TYPE = {
  name: 'People',
  title_field: 'name',
  fields: [
    { id: 'name', label: 'Name', type: 'text', required: true, unique: true },
    { id: 'role', label: 'Role', type: 'enum', values: ['member', 'editor'], required: true, default: 'member' },
    { id: 'since', label: 'Editor since', type: 'date', when: { field: 'role', is: 'editor' } },
  ],
};

const PAGES_TYPE = {
  name: 'Pages',
  title_field: 'title',
  fields: [
    { id: 'title', label: 'Title', type: 'text', required: true, unique: true },
    { id: 'hero', label: 'Hero', type: 'object', fields: [
      { id: 'heading', label: 'Heading', type: 'text', required: true },
      { id: 'accent', label: 'Accent', type: 'color' },
    ] },
    { id: 'tags', label: 'Tags', type: 'list', of: { type: 'text', max_chars: 30, pattern: '[a-z0-9]+' }, max: 5 },
    { id: 'faq', label: 'Questions', type: 'list', max: 10, of: { type: 'object', fields: [
      { id: 'q', label: 'Question', type: 'text', required: true },
      { id: 'a', label: 'Answer', type: 'markdown' },
    ] } },
    { id: 'sections', label: 'Sections', type: 'blocks', max: 20, blocks: [
      { name: 'text_block', label: 'Text', fields: [{ id: 'body', label: 'Body', type: 'markdown', required: true }] },
      { name: 'quote', label: 'Quote', fields: [
        { id: 'text', label: 'Quote', type: 'text', required: true },
        { id: 'by', label: 'By', type: 'reference', to: 'people' },
      ] },
    ] },
  ],
};

/** A page whose two quotes name `first` and `second`. */
function page(title, n, first, second) {
  return JSON.stringify({
    title,
    hero: { heading: `Edition ${n}`, accent: `#${(0x100000 + (n % 0xeffff)).toString(16)}` },
    tags: ['news', `n${n % 100}`],
    faq: [{ q: 'Free?', a: `Yes, edition ${n}.` }, { q: 'Open?', a: 'Yes.' }],
    sections: [
      { _block: 'text_block', body: `# Edition ${n}` },
      { _block: 'quote', text: 'Ship it', by: first },
      { _block: 'quote', text: 'Prove it', by: second },
    ],
  });
}

function key() {
  return `k6-${__VU}-${__ITER}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export function setup() {
  const login = http.get(`${PUBLIC_URL}/auth/login?return_to=/me`);
  const action = parseHTML(login.body).find('#kc-form-login').attr('action');
  if (!action) fail('no Keycloak login form');
  const me = http.post(action.replace(/&amp;/g, '&'), { username: 'dev-owner', password: 'colophon-dev' });
  if (me.status !== 200) fail(`sign-in failed: HTTP ${me.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const session = { cookie: `colophon_session=${jar.colophon_session[0]}`, csrf: jar.colophon_csrf[0] };
  const write = { Cookie: session.cookie, 'X-CSRF-Token': session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };

  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 modelling' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const env = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;
  for (const [type, definition] of [['people', PEOPLE_TYPE], ['pages', PAGES_TYPE]]) {
    const made = http.post(`${env}/content-types`, JSON.stringify({ slug: type, definition }), {
      headers: Object.assign({ 'Idempotency-Key': `k6-type-${slug}-${type}` }, write),
    });
    if (made.status !== 201) fail(`content type ${type}: HTTP ${made.status} ${made.body}`);
  }
  const people = [];
  for (let i = 0; i < PEOPLE; i++) {
    const made = http.post(`${env}/collections/people/entries`, JSON.stringify({ name: `Person ${i}` }), {
      headers: Object.assign({ 'Idempotency-Key': `k6-person-${slug}-${i}` }, write),
    });
    if (made.status !== 201) fail(`person ${i}: HTTP ${made.status} ${made.body}`);
    people.push(made.json('data.id'));
  }
  const base = `${env}/collections/pages/entries`;
  const pages = [];
  for (let i = 0; i < PAGES; i++) {
    const made = http.post(base, page(`Page ${i}`, i, people[i % PEOPLE], people[(i + 1) % PEOPLE]), {
      headers: Object.assign({ 'Idempotency-Key': `k6-page-${slug}-${i}` }, write),
    });
    if (made.status !== 201) fail(`page ${i}: HTTP ${made.status} ${made.body}`);
    pages.push({ id: made.json('data.id'), etag: made.headers['Etag'] });
  }
  return { session, base, people, peopleBase: `${env}/collections/people/entries`, pages };
}

let etag = null;

export default function (data) {
  const mine = data.pages[(__VU - 1) % data.pages.length];
  if (etag === null) etag = mine.etag;
  const url = `${data.base}/${mine.id}`;
  const read = { headers: { Cookie: data.session.cookie }, redirects: 0 };
  const writing = () => ({
    Cookie: data.session.cookie, 'X-CSRF-Token': data.session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json',
    'If-Match': etag, 'Idempotency-Key': key(),
  });
  const someone = () => data.people[Math.floor(Math.random() * data.people.length)];
  const roll = __ITER % 10;

  if (roll < 2) {
    const res = http.put(url, page(`Page by VU ${__VU}`, __ITER + __VU, someone(), someone()), {
      headers: writing(), redirects: 0, tags: { endpoint: 'save', name: 'pages/{entry}' },
    });
    const saved = check(res, {
      'save 200': (r) => r.status === 200,
      'save gives a new ETag': (r) => typeof r.headers['Etag'] === 'string' && r.headers['Etag'] !== etag,
    });
    if (saved) etag = res.headers['Etag'];
  } else if (roll < 3) {
    const res = http.put(url, page('Broken', __ITER, '01a18f43-a955-755e-ab9c-1ded4406683c', someone()), {
      headers: writing(), redirects: 0, tags: { endpoint: 'broken', name: 'pages/{entry}' },
      responseCallback: http.expectedStatuses(422),
    });
    check(res, {
      'a quote naming nobody is 422': (r) => r.status === 422,
      'refused where it is': (r) => r.json('error.details.reason') === 'reference_not_found' && r.json('error.details.pointer') === '/sections/1/by',
    });
  } else if (roll < 4) {
    const res = http.get(`${data.peopleBase}/${someone()}`, Object.assign({ tags: { endpoint: 'person', name: 'people/{entry}' } }, read));
    check(res, {
      'person 200': (r) => r.status === 200,
      'says what references them': (r) => Array.isArray(r.json('data.referenced_by')),
    });
  } else if (roll < 5 && Math.floor(__ITER / 10) % 2 === 0) {
    const res = http.get(`${data.base}?limit=20`, Object.assign({ tags: { endpoint: 'list', name: 'pages?limit' } }, read));
    check(res, { 'list 200': (r) => r.status === 200 });
  } else if (roll < 5) {
    const headers = writing();
    delete headers['If-Match'];
    const res = http.post(data.peopleBase, JSON.stringify({ name: `VU ${__VU} at ${__ITER}` }), {
      headers, redirects: 0, tags: { endpoint: 'create', name: 'people' },
    });
    check(res, {
      'create 201': (r) => r.status === 201,
      'the default fills what it left out': (r) => r.json('data.content.role') === 'member',
    });
  } else {
    const res = http.get(url, Object.assign({ tags: { endpoint: 'read', name: 'pages/{entry}' } }, read));
    check(res, {
      'read 200': (r) => r.status === 200,
      'read returns the last accepted save': (r) => r.headers['Etag'] === etag,
    });
  }
}
