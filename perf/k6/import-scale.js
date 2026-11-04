// Load scenario for L09 (spec §29): a real 10,000-record draft import — not a fixture that
// says "imagine ten thousand". setup() signs in, makes a project and gives it a `posts`
// collection; the run then pushes TOTAL_RECORDS documents through the import endpoint in
// 512-item chunks (the endpoint's own cap, IMPORT_MAX_ITEMS) with IMPORT_VUS workers,
// every record carrying a deterministic Sanity `_id` so the import's own adoption rules
// make the whole job resumable: re-sending a chunk can never duplicate its entries.
//
// Each chunk is checked for the import's honest answers: 201, `imported` equal to the
// chunk size, every item's outcome `created` with an entry id. The LAST chunk's iteration
// then verifies the job end to end, because the spec asks for resumability, per-item
// outcomes and no accidental publication, not just bulk writes:
//
//   - REPLAY: chunk 0 posted again with the SAME Idempotency-Key — the stored response
//     must come back verbatim (the retry-safe answer a dropped connection needs), and the
//     per-item outcomes still say `created`: nothing was written twice.
//   - UPSERT: chunk 1 posted again with a FRESH key — every item must now answer
//     `updated`, because its `_id` was adopted as its entry's id the first time through.
//   - COUNT: the collection's export must list exactly TOTAL_RECORDS documents — no
//     duplicates from either re-send, none lost. It polls a little, in case the other
//     worker's last chunk is still committing.
//   - NOT PUBLISHED: the public delivery endpoint must answer an empty list at generation
//     0 — ten thousand drafts changed nothing public, so there is no accidental
//     publication to find.
//
// "Stable memory" is not asserted by the verdict object — it is measured beside it:
// scripts/load.sh samples the server's RSS/CPU and Postgres connections into `resources`
// for this endpoint; the verdict records the job's own numbers. The arms are
// iteration-counted (one iteration per chunk), so TOTAL, VUS, RATE and DURATION do not
// shape the job; `steady` and `throughput` run the same plan. handleSummary() computes
// the verdict into `l09`; scripts/load.sh stores it and fails when `held` is false.
// scripts/load.sh gives this endpoint P99_MS 15000 — a chunk is a transaction of 512
// revision writes, not a single row.

import http from 'k6/http';
import { check, fail, sleep } from 'k6';
import exec from 'k6/execution';
import { Counter } from 'k6/metrics';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '15000');
const TOTAL_RECORDS = parseInt(__ENV.TOTAL_RECORDS || '10000', 10);
const CHUNK = 512; // the endpoint's own cap (IMPORT_MAX_ITEMS)
const CHUNKS = Math.ceil(TOTAL_RECORDS / CHUNK);
const IMPORT_VUS = parseInt(__ENV.IMPORT_VUS || '2', 10);

const chunks = new Counter('l09_chunks');
const created = new Counter('l09_created');
const updated = new Counter('l09_updated');
const skipped = new Counter('l09_skipped');
const invalid = new Counter('l09_invalid');
const errors = new Counter('l09_errors');
const exported = new Counter('l09_exported');
const emptyDelivery = new Counter('l09_delivery_empty');

if (!(SCENARIO === 'throughput' || SCENARIO === 'steady')) {
  throw new Error('SCENARIO must be throughput or steady');
}

const thresholds = {
  checks: ['rate==1'],
  http_req_failed: ['rate==0'],
  l09_errors: ['count==0'],
  l09_invalid: ['count==0'],
  l09_skipped: ['count==0'],
  l09_created: [`count==${TOTAL_RECORDS}`],
  l09_exported: [`count==${TOTAL_RECORDS}`],
  l09_updated: ['count>0'], // the fresh-key replay upserted rather than duplicated
  l09_delivery_empty: ['count>0'], // ten thousand drafts changed nothing public
  http_req_duration: [`p(99)<${P99_MS}`],
  'http_req_duration{endpoint:import_chunk}': ['p(99)<120000'],
};

export const options = {
  // The job is a fixed number of chunks; both modes run it to completion.
  scenarios: { import: { executor: 'shared-iterations', vus: IMPORT_VUS, iterations: CHUNKS, maxDuration: '3h', exec: 'importChunk' } },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '300s',
};

const POSTS = {
  name: 'Posts',
  title_field: 'headline',
  fields: [
    { id: 'headline', label: 'Headline', type: 'text', required: true, index: true },
    { id: 'dek', label: 'Dek', type: 'text' },
    { id: 'section', label: 'Section', type: 'enum', values: ['News', 'Guides'], index: true },
  ],
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

  const slug = `k6-${Date.now().toString(36)}`;
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 import scale' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const id = project.json('data.id');
  const env = `${BASE_URL}/api/v1/projects/${id}/environments/production`;
  const made = http.post(`${env}/content-types`, JSON.stringify({ slug: 'posts', definition: POSTS }), {
    headers: Object.assign({ 'Idempotency-Key': `k6-type-${slug}` }, write),
  });
  if (made.status !== 201) fail(`content type: HTTP ${made.status} ${made.body}`);
  return {
    session, env, write,
    importUrl: `${env}/collections/posts/import`,
    exportUrl: `${env}/collections/posts/export`,
    delivery: `${BASE_URL}/content/v1/${id}/production/posts`,
  };
}

// Record `i`'s deterministic Sanity id: a valid UUID spelling unique to the record, so a
// re-imported chunk meets its own rows (adopted on create, updated on replay).
function sourceId(i) {
  const low = String(i).padStart(12, '0');
  return `00000000-0000-4000-8000-${low}`;
}

function chunkFile(c) {
  let file = '';
  for (let n = c * CHUNK; n < Math.min((c + 1) * CHUNK, TOTAL_RECORDS); n += 1) {
    file += JSON.stringify({
      _id: sourceId(n), _type: 'posts',
      headline: `Record ${n}`, dek: `dek ${n}`, section: n % 2 === 0 ? 'News' : 'Guides',
    }) + '\n';
  }
  return file;
}

function importKey(c) {
  // 16-128 letters, digits, '-' or '_' — the API's key rule; the zero-pad keeps it ≥16.
  return `k6-l09-chunk-${String(c).padStart(4, '0')}`;
}

function post(data, file, idemKey, endpoint) {
  return http.post(data.importUrl, file, {
    headers: Object.assign({}, data.write, { 'Content-Type': 'application/jsonlines', 'Idempotency-Key': idemKey }),
    redirects: 0, tags: { endpoint, name: 'collections/{collection}/import' },
  });
}

function outcomes(res, want) {
  const rows = res.json('data.outcomes');
  if (!Array.isArray(rows)) return 0;
  let matched = 0;
  for (const row of rows) {
    if (row.status === want) matched += 1;
    if (row.status === 'skipped') skipped.add(1);
    if (row.status === 'invalid') invalid.add(1);
  }
  return matched;
}

export function importChunk(data) {
  // The global iteration index — shared-iterations hands each chunk to exactly one VU.
  const c = exec.scenario.iterationInInstance;
  const size = Math.min(CHUNK, TOTAL_RECORDS - c * CHUNK);
  const res = post(data, chunkFile(c), importKey(c), 'import_chunk');
  const ok = check(res, {
    'chunk 201': (r) => r.status === 201,
    'chunk reports every item': (r) => (r.json('data.outcomes') || []).length === size,
    'chunk imported every item': (r) => r.json('data.imported') === size,
  });
  if (ok) {
    const made = outcomes(res, 'created');
    created.add(made);
    check(res, { 'every item created with an entry': (r) =>
      (r.json('data.outcomes') || []).every((o) => o.status === 'created' && typeof o.entry === 'string') });
    chunks.add(1);
  } else {
    errors.add(1);
    console.log(`chunk ${c}: ${res.status} ${String(res.body).slice(0, 300)}`);
    return;
  }
  // The last chunk's iteration verifies the whole job, with a little patience for a
  // sibling chunk still committing beside it.
  if (c === CHUNKS - 1) verify(data);
}

// The end-to-end checks the spec names: a same-key replay is verbatim-safe, a fresh-key
// replay upserts, the export counts every record once, and nothing is public.
function verify(data) {
  const replayed = post(data, chunkFile(0), importKey(0), 'replay');
  check(replayed, {
    'same-key replay answers verbatim': (r) => r.status === 201 && r.json('data.imported') === CHUNK
      && (r.json('data.outcomes') || []).every((o) => o.status === 'created'),
  });
  const upsert = post(data, chunkFile(1), `${importKey(1)}-again`, 'upsert');
  const upserted = outcomes(upsert, 'updated');
  updated.add(upserted);
  check(upsert, {
    'fresh-key replay updates, not duplicates': (r) => r.status === 201 && upserted === CHUNK,
  });
  // The exact final count: one export line per record — no duplicates, none lost.
  let count = -1;
  for (let waits = 0; waits < 60 && count !== TOTAL_RECORDS; waits += 1) {
    const dump = http.get(data.exportUrl, {
      headers: { Cookie: data.session.cookie }, redirects: 0, tags: { endpoint: 'export', name: 'collections/{collection}/export' },
    });
    if (dump.status === 200) {
      count = dump.body.split('\n').filter((line) => line.trim() !== '').length;
    }
    if (count !== TOTAL_RECORDS) sleep(1);
  }
  if (count === TOTAL_RECORDS) exported.add(count);
  check(count, { 'export holds exactly the corpus': (n) => n === TOTAL_RECORDS });
  // Ten thousand drafts, and the public side stays exactly as it was: nothing published.
  const live = http.get(data.delivery, { redirects: 0, tags: { endpoint: 'delivery', name: 'content' } });
  if (check(live, {
    'nothing published': (r) => r.status === 200 && r.json('data').length === 0 && r.headers['Colophon-Generation'] === '0',
  })) emptyDelivery.add(1);
}

/** Computes the L09 verdict: the whole corpus landed with per-item `created` outcomes,
 *  both replays behaved, the export counted exactly TOTAL_RECORDS and delivery stayed
 *  empty. Memory stability evidence lives in the result's `resources` samples. */
export function handleSummary(data) {
  const count = (name) => (data.metrics[name] ? data.metrics[name].values.count || 0 : 0);
  const dur = (data.metrics['http_req_duration{endpoint:import_chunk}'] || {}).values || {};
  const gates = {
    corpus_imported: count('l09_created') === TOTAL_RECORDS && count('l09_chunks') === CHUNKS,
    per_item_outcomes: count('l09_invalid') === 0 && count('l09_skipped') === 0,
    resumable: count('l09_updated') === CHUNK, // chunk 1's fresh-key replay updated all 512
    exact_final_count: count('l09_exported') === TOTAL_RECORDS,
    nothing_published: count('l09_delivery_empty') > 0,
    no_errors: count('l09_errors') === 0,
  };
  data.l09 = {
    spec: 'L09: 10,000-record draft import — resumable chunks, per-item outcomes, stable memory; no accidental publication',
    gate: `${TOTAL_RECORDS} records in ${CHUNKS} chunks of <=512, every outcome 'created'; same-key replay verbatim, fresh-key replay 'updated'; export count == ${TOTAL_RECORDS}; delivery empty at generation 0; memory held by the result's resources samples`,
    records: TOTAL_RECORDS,
    chunks: count('l09_chunks'),
    outcomes: {
      created: count('l09_created'),
      updated: count('l09_updated'),
      skipped: count('l09_skipped'),
      invalid: count('l09_invalid'),
    },
    exported: count('l09_exported'),
    delivery_answers_empty: count('l09_delivery_empty'),
    errors: count('l09_errors'),
    chunk_latency_ms: { p50: dur.med, p95: dur['p(95)'], p99: dur['p(99)'], max: dur.max },
    gates,
    held: Object.values(gates).every(Boolean),
  };
  return {
    stdout: [
      '',
      '  L09 — 10,000-record draft import',
      `    ${count('l09_created')} created over ${count('l09_chunks')} chunks, chunk p95 ${(dur['p(95)'] || 0).toFixed(0)} ms / max ${(dur.max || 0).toFixed(0)} ms`,
      `    replay: ${count('l09_updated')} updated on fresh-key re-send; export ${count('l09_exported')} lines; delivery empty checks ${count('l09_delivery_empty')}`,
      `    verdict: ${data.l09.held ? 'HELD' : 'BROKE'}`,
      '',
    ].join('\n'),
  };
}
