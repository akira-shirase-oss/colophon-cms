// Load scenario for the website starter (W.0): the entries the bundle installs, under the write
// profile the page builder creates. setup() signs in, creates a project and installs the bundle
// the same way the Studio and testbed do — the section-style component, the pages/navigation/
// theme types, then the seed entries — and gives each VU an extra page. Each VU reads its page
// (50%), rewrites it with a fresh mix of sections so every save runs blocks validation against
// six kinds with nested lists, objects and the shared component (25%), reads the theme entry
// (10%), reads the navigation entry (10%), or lists pages (5%); a page read must carry the ETag
// of its last accepted save.
// Same settings as content-types.js; scripts/load.sh gives this endpoint RATE 500 and P99_MS 250.

import http from 'k6/http';
import { check, fail } from 'k6';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const TOTAL = parseInt(__ENV.TOTAL || '1000000', 10);
const VUS = parseInt(__ENV.VUS || '64', 10);
const RATE = parseInt(__ENV.RATE || '500', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '250');

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
const ENTRIES = SCENARIO === 'steady' ? VUS * 2 : VUS;

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

// The bundle, as the manifest in packages/site-template sends it; kept inline because k6 cannot
// import the workspace package. Section kinds cover the nested shapes the schema allows: a list
// of objects (features, faq) and the shared component echoed as an object field (style).
const STYLE = {
  id: 'style',
  label: 'Style',
  required: false,
  type: 'object',
  component: 'section-style',
  fields: [
    { id: 'scheme', label: 'Colour treatment', required: false, type: 'enum', values: ['inherit', 'surface', 'primary', 'secondary', 'invert'] },
    { id: 'spacing', label: 'Space above and below', required: false, type: 'enum', values: ['compact', 'comfortable', 'spacious'] },
    { id: 'align', label: 'Text direction', required: false, type: 'enum', values: ['start', 'center', 'end'] },
  ],
};

const SECTION_STYLE = {
  slug: 'section-style',
  definition: {
    name: 'Section style',
    fields: STYLE.fields,
  },
};

const PAGES = {
  slug: 'pages',
  definition: {
    name: 'Pages',
    title_field: 'title',
    fields: [
      { id: 'title', label: 'Title', required: true, type: 'text', max_chars: 200, multiline: false },
      { id: 'slug', label: 'Slug', required: true, type: 'slug', source: 'title' },
      {
        id: 'sections',
        label: 'Sections',
        required: false,
        type: 'blocks',
        max: 24,
        blocks: [
          {
            name: 'hero',
            label: 'Hero',
            fields: [
              { id: 'heading', label: 'Heading', required: true, type: 'text', max_chars: 120, multiline: false },
              { id: 'tagline', label: 'Tagline', required: false, type: 'text', max_chars: 280, multiline: true },
              { id: 'label', label: 'Button label', required: false, type: 'text', max_chars: 40, multiline: false },
              { id: 'url', label: 'Button link', required: false, type: 'url' },
              STYLE,
            ],
          },
          {
            name: 'features',
            label: 'Feature list',
            fields: [
              { id: 'heading', label: 'Heading', required: false, type: 'text', max_chars: 120, multiline: false },
              {
                id: 'items',
                label: 'Features',
                required: false,
                type: 'list',
                max: 12,
                of: {
                  type: 'object',
                  fields: [
                    { id: 'title', label: 'Title', required: true, type: 'text', max_chars: 80, multiline: false },
                    { id: 'text', label: 'Text', required: false, type: 'text', max_chars: 200, multiline: false },
                  ],
                },
              },
              STYLE,
            ],
          },
          {
            name: 'faq',
            label: 'Questions and answers',
            fields: [
              { id: 'heading', label: 'Heading', required: false, type: 'text', max_chars: 120, multiline: false },
              {
                id: 'items',
                label: 'Questions',
                required: false,
                type: 'list',
                max: 20,
                of: {
                  type: 'object',
                  fields: [
                    { id: 'question', label: 'Question', required: true, type: 'text', max_chars: 200, multiline: false },
                    { id: 'answer', label: 'Answer', required: true, type: 'markdown', max_chars: 1000 },
                  ],
                },
              },
              STYLE,
            ],
          },
        ],
      },
    ],
  },
};

const NAVIGATION = {
  slug: 'navigation',
  definition: {
    name: 'Navigation',
    title_field: 'title',
    single: true,
    fields: [
      { id: 'title', label: 'Title', required: true, type: 'text', max_chars: 60, multiline: false, default: 'Navigation' },
      {
        id: 'items',
        label: 'Menu items',
        required: false,
        type: 'list',
        max: 20,
        of: {
          type: 'object',
          fields: [
            { id: 'label', label: 'Label', required: true, type: 'text', max_chars: 40, multiline: false },
            { id: 'page', label: 'Page', required: false, type: 'reference', to: 'pages' },
            { id: 'url', label: 'Outside link', required: false, type: 'url' },
          ],
        },
      },
    ],
  },
};

const THEME = {
  slug: 'theme',
  definition: {
    name: 'Theme',
    title_field: 'title',
    single: true,
    fields: [
      { id: 'title', label: 'Title', required: true, type: 'text', max_chars: 60, multiline: false, default: 'Site theme' },
      {
        id: 'light',
        label: 'Light mode',
        required: true,
        type: 'object',
        fields: [
          { id: 'primary', label: 'Primary', required: true, type: 'color' },
          { id: 'text', label: 'Text', required: true, type: 'color' },
          { id: 'radius_md', label: 'Medium corners (px)', required: true, type: 'number', integer: true, min: 0, max: 32 },
          { id: 'base_size', label: 'Base text size (px)', required: true, type: 'number', integer: true, min: 14, max: 20 },
        ],
      },
    ],
  },
};

// A page body with a rotating mix of sections, valid against the blocks above. The VU saves it
// over its own page, so schema validation — block kind dispatch, nested lists, the echoed
// component — runs on every write. Slugs must be passed in: __VU is 0 inside setup().
function page(title, n, slug = `page-${n % 1000}-${__VU}`) {
  const sections = [
    {
      _block: 'hero',
      heading: `${title} ${n}`,
      tagline: `Edition ${n} of the page builder's page.`,
      label: 'Start writing',
      url: 'https://example.com/',
      style: { scheme: ['inherit', 'surface', 'primary'][n % 3], spacing: ['compact', 'comfortable', 'spacious'][n % 3], align: 'center' },
    },
    {
      _block: 'features',
      heading: 'What it ships',
      items: [
        { title: 'One', text: `Feature ${n}` },
        { title: 'Two', text: 'Validated on every save' },
      ],
    },
    {
      _block: 'faq',
      items: [{ question: 'Is this content?', answer: `Yes, revision **${n}** of it.` }],
    },
  ];
  return JSON.stringify({ title: `${title} ${n}`, slug, sections: sections.slice(0, 1 + (n % 3)) });
}

function key() {
  return `k6-${__VU}-${__ITER}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export function setup() {
  const signedIn = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }), { headers: { 'Content-Type': 'application/json' } });
  if (signedIn.status !== 204) fail(`sign-in failed: HTTP ${signedIn.status}`);
  const jar = http.cookieJar().cookiesForURL(PUBLIC_URL);
  const session = { cookie: `colophon_session=${jar.colophon_session[0]}`, csrf: jar.colophon_csrf[0] };
  const write = { Cookie: session.cookie, 'X-CSRF-Token': session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json' };

  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 site starter' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const env = `${BASE_URL}/api/v1/projects/${project.json('data.id')}/environments/production`;

  const component = http.post(`${env}/components`, JSON.stringify(SECTION_STYLE), {
    headers: Object.assign({ 'Idempotency-Key': `k6-component-${slug}` }, write),
  });
  if (component.status !== 201) fail(`component: HTTP ${component.status} ${component.body}`);
  for (const type of [PAGES, NAVIGATION, THEME]) {
    const made = http.post(`${env}/content-types`, JSON.stringify(type), {
      headers: Object.assign({ 'Idempotency-Key': `k6-type-${slug}-${type.slug}` }, write),
    });
    if (made.status !== 201) fail(`type ${type.slug}: HTTP ${made.status} ${made.body}`);
  }

  const pages = `${env}/collections/pages/entries`;
  const home = http.post(pages, page('Home', 0, 'home'), { headers: Object.assign({ 'Idempotency-Key': `k6-home-${slug}` }, write) });
  if (home.status !== 201) fail(`home page: HTTP ${home.status} ${home.body}`);
  const navigation = http.post(
    `${env}/collections/navigation/entries`,
    JSON.stringify({ title: 'Navigation', items: [{ label: 'Home', page: home.json('data.id') }] }),
    { headers: Object.assign({ 'Idempotency-Key': `k6-nav-${slug}` }, write) }
  );
  if (navigation.status !== 201) fail(`navigation: HTTP ${navigation.status} ${navigation.body}`);
  const theme = http.post(
    `${env}/collections/theme/entries`,
    JSON.stringify({ title: 'Site theme', light: { primary: '#6d28d9', text: '#18181b', radius_md: 8, base_size: 16 } }),
    { headers: Object.assign({ 'Idempotency-Key': `k6-theme-${slug}` }, write) }
  );
  if (theme.status !== 201) fail(`theme: HTTP ${theme.status} ${theme.body}`);

  const entries = [];
  for (let i = 0; i < ENTRIES; i++) {
    const created = http.post(pages, page(`Page ${i}`, i, `setup-${i}`), { headers: Object.assign({ 'Idempotency-Key': `k6-setup-${slug}-${i}` }, write) });
    if (created.status !== 201) fail(`entry ${i}: HTTP ${created.status} ${created.body}`);
    entries.push({ id: created.json('data.id'), etag: created.headers['Etag'] });
  }
  return {
    session,
    pages,
    entries,
    navigation: `${env}/collections/navigation/entries`,
    theme: `${env}/collections/theme/entries`,
  };
}

let etag = null;

export default function (data) {
  const mine = data.entries[(__VU - 1) % data.entries.length];
  if (etag === null) etag = mine.etag;
  const url = `${data.pages}/${mine.id}`;
  const read = { headers: { Cookie: data.session.cookie }, redirects: 0 };
  const roll = __ITER % 20;

  if (roll < 5) {
    const headers = {
      Cookie: data.session.cookie, 'X-CSRF-Token': data.session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json',
      'If-Match': etag, 'Idempotency-Key': key(),
    };
    const res = http.put(url, page(`Page by VU ${__VU}`, __ITER + __VU), { headers, redirects: 0, tags: { endpoint: 'save', name: 'pages/{entry}' } });
    const saved = check(res, {
      'save 200': (r) => r.status === 200,
      'save gives a new ETag': (r) => typeof r.headers['Etag'] === 'string' && r.headers['Etag'] !== etag,
    });
    if (saved) etag = res.headers['Etag'];
  } else if (roll < 7) {
    const res = http.get(`${data.theme}?limit=1`, Object.assign({ tags: { endpoint: 'theme', name: 'theme/entries' } }, read));
    check(res, { 'theme 200': (r) => r.status === 200 });
  } else if (roll < 9) {
    const res = http.get(`${data.navigation}?limit=1`, Object.assign({ tags: { endpoint: 'navigation', name: 'navigation/entries' } }, read));
    check(res, { 'navigation 200': (r) => r.status === 200 });
  } else if (roll < 10) {
    const res = http.get(`${data.pages}?limit=20`, Object.assign({ tags: { endpoint: 'list', name: 'pages?limit' } }, read));
    check(res, { 'list 200': (r) => r.status === 200 });
  } else {
    const res = http.get(url, Object.assign({ tags: { endpoint: 'read', name: 'pages/{entry}' } }, read));
    check(res, {
      'read 200': (r) => r.status === 200,
      'read returns the last accepted save': (r) => r.headers['Etag'] === etag,
    });
  }
}
