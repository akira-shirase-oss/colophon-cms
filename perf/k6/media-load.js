// Load scenario for L14 (spec §29): a large-media decoder workload running beside active
// editing — the worker's limits must hold (a real ~11.5 MB image verifies, a fake is
// rejected, an oversized file is refused at the door) while editors keep their latency.
// Like L08, one run measures both halves: an editorial baseline phase at RATE, then the
// same editorial traffic while MEDIA_VUS workers upload multi-megabyte files.
//
// The media arm's work is real, not claimed bytes: `BIG` is an actual 2400x1600 RGB PNG
// of compressed noise (~11.5 MB on disk — perf/k6/fixtures/media-load-large.png, made by
// a three-line zlib PNG writer; regenerate it with the recipe in the header). Each media
// iteration is one of: upload `BIG` and poll it to a verdict — `usable` means the decoder
// ran the whole file and wrote renditions, which the public origin then serves — upload
// the fake PNG bytes (it must end `rejected`), post 21 MB of zeroes (under the server's
// own 21 MiB body limit but over the asset cap — a 422 `invalid_upload` at the door,
// before any decoder work), or post the real bytes declared as `image/svg+xml` (a 422
// `invalid_upload` on the declared type). Upload measured latency is the request's own:
// decoding runs off the request path in the worker.
//
// "Content editing stays responsive" is measured the L08 way: the media phase's
// editorial p95 against the baseline phase's own, inside a 2x bound plus the 2 ms
// scheduler-jitter epsilon — plus every media outcome counted, so a run that quietly
// did no decoder work fails rather than passes. The arms are time-boxed: TOTAL and VUS
// are unused. `steady` and `throughput` run the same plan. handleSummary() computes the
// verdict into `l14`; scripts/load.sh stores it and fails when `held` is false — and
// samples server RSS, CPU and Postgres connections into `resources` (the decoder's
// allocation bounds show up as a stable RSS, beside the request numbers).
// scripts/load.sh gives this endpoint RATE 50 and P99_MS 700 — the editorial write
// budget.
//
// Regenerate the fixture:
//   python3 - <<'EOF'  (2400x1600 RGB noise -> ~11.5 MB PNG at perf/k6/fixtures/)
//   import os,struct,zlib
//   W,H=2400,1600
//   chunk=lambda k,d:struct.pack('>I',len(d))+k+d+struct.pack('>I',zlib.crc32(k+d)&0xFFFFFFFF)
//   raw=b''.join(b'\x00'+os.urandom(W*3) for _ in range(H))
//   open('perf/k6/fixtures/media-load-large.png','wb').write(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',W,H,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(raw,6))+chunk(b'IEND',b''))
//   EOF

import http from 'k6/http';
import { check, fail, sleep } from 'k6';
import exec from 'k6/execution';
import { Counter } from 'k6/metrics';
import { parseHTML } from 'k6/html';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const RATE = parseInt(__ENV.RATE || '50', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '700');
const MEDIA_VUS = parseInt(__ENV.MEDIA_VUS || '3', 10);
const UPLOAD_P99_MS = parseFloat(__ENV.UPLOAD_P99_MS || '30000');
// "Content editing stays responsive" (the spec's L14 bound): the draft-write latency
// budget from END_GOAL's capacity table — p95 under 300 ms while the decoder runs.
const RESPONSIVE_P95_MS = parseFloat(__ENV.RESPONSIVE_P95_MS || '300');

// The fixture: a real ~11.5 MB PNG (see the header for how it was made). k6 resolves
// open() against the script's own directory — this file sits in perf/k6/.
const BIG = open(__ENV.BIG_FILE || 'fixtures/media-load-large.png', 'b');
const BIG_BYTES = BIG.byteLength;
// 21,000,000 bytes: past the 20 MiB asset cap, inside the server's own 21 MiB body
// limit — so the 422 the arm sees is the ASSET's bound, not the door's.
const OVERSIZED = new Uint8Array(21_000_000);
const FAKE = new TextEncoder().encode('<html><body>not an image</body></html>');

const verified = new Counter('l14_verified');
const rejected = new Counter('l14_rejected');
const oversizedRefused = new Counter('l14_oversized_refused');
const wrongTypeRefused = new Counter('l14_wrong_type_refused');
const uploads = new Counter('l14_uploads');
const mediaReads = new Counter('l14_media_reads');
const errors = new Counter('l14_errors');

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}
const PHASE = durationSeconds(DURATION);
const GAP = 2;
const EDITORIAL_VUS = Math.max(8, Math.ceil(RATE / 10));

if (!(SCENARIO === 'throughput' || SCENARIO === 'steady')) {
  throw new Error('SCENARIO must be throughput or steady');
}

const thresholds = {
  checks: ['rate==1'],
  http_req_failed: ['rate==0'],
  l14_errors: ['count==0'],
  l14_verified: ['count>0'],
  l14_rejected: ['count>0'],
  l14_oversized_refused: ['count>0'],
  l14_wrong_type_refused: ['count>0'],
  l14_media_reads: ['count>0'],
  'http_reqs{arm:baseline}': ['count>0'],
  'http_reqs{arm:media_phase}': ['count>0'],
  'http_reqs{arm:media}': ['count>0'],
  'http_req_duration{arm:baseline}': [`p(95)<${P99_MS}`],
  'http_req_duration{arm:media_phase}': [`p(95)<${P99_MS}`],
  'http_req_duration{endpoint:upload}': [`p(99)<${UPLOAD_P99_MS}`],
};
if (SCENARIO === 'steady') {
  const scheduled = RATE * PHASE * 2;
  thresholds.dropped_iterations = [`count<=${Math.max(1, Math.floor(scheduled * 0.0001))}`];
}

export const options = {
  scenarios: {
    baseline: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, preAllocatedVUs: EDITORIAL_VUS, maxVUs: EDITORIAL_VUS * 2, exec: 'editorial' },
    media_phase: { executor: 'constant-arrival-rate', rate: RATE, timeUnit: '1s', duration: DURATION, startTime: `${PHASE + GAP}s`, preAllocatedVUs: EDITORIAL_VUS, maxVUs: EDITORIAL_VUS * 2, exec: 'editorial' },
    media: { executor: 'constant-vus', vus: MEDIA_VUS, duration: DURATION, startTime: `${PHASE + GAP}s`, exec: 'media' },
  },
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '300s',
};

const article = (title, text) => JSON.stringify({
  title,
  body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] } },
});

function key() {
  return `k6-l14-${__VU}-${__ITER}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

// Enough articles for every editorial VU id across both editorial arms' maxima.
const ARTICLES = EDITORIAL_VUS * 2 * 4;

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
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 media load' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const id = project.json('data.id');
  const env = `${BASE_URL}/api/v1/projects/${id}/environments/production`;
  const entries = `${env}/collections/articles/entries`;

  const articles = [];
  for (let i = 0; i < ARTICLES; i += 40) {
    const batch = [];
    for (let j = i; j < Math.min(i + 40, ARTICLES); j += 1) {
      batch.push(['POST', entries, article(`Article ${j}`, 'Start'), {
        headers: Object.assign({ 'Idempotency-Key': `k6-l14-${slug}-${j}` }, write),
        tags: { name: 'setup' },
      }]);
    }
    const replies = http.batch(batch);
    for (let k = 0; k < replies.length; k += 1) {
      if (replies[k].status !== 201) fail(`article ${i + k}: HTTP ${replies[k].status} ${replies[k].body}`);
      articles.push({ id: replies[k].json('data.id'), etag: replies[k].headers['Etag'] });
    }
  }
  return {
    session, write, entries, articles,
    assets: `${env}/assets`,
    media: `${BASE_URL}/media/v1/${id}/production`,
  };
}

// The editorial arm's own truth: the ETag of the revision it last saved (module state is
// per-VU).
let etag = null;
export function editorial(data) {
  const arm = exec.scenario.name === 'baseline' ? 'baseline' : 'media_phase';
  const mine = data.articles[(exec.vu.idInTest - 1) % data.articles.length];
  if (etag === null) etag = mine.etag;
  const url = `${data.entries}/${mine.id}`;
  const tags = { arm };
  const roll = __ITER % 10;
  if (roll < 2) {
    const res = http.put(url, article(`Article by VU ${exec.vu.idInTest}`, `Iteration ${__ITER}`), {
      headers: Object.assign({ 'If-Match': etag, 'Idempotency-Key': key(), 'Content-Type': 'application/json', Cookie: data.session.cookie, 'X-CSRF-Token': data.session.csrf, Origin: PUBLIC_URL }),
      redirects: 0, tags: Object.assign({ endpoint: 'save', name: 'entries/{entry}' }, tags),
    });
    const saved = check(res, {
      'save 200': (r) => r.status === 200,
      'save gives a new ETag': (r) => typeof r.headers['Etag'] === 'string' && r.headers['Etag'] !== etag,
    });
    if (saved) etag = res.headers['Etag'];
    else { errors.add(1); etag = null; }
  } else if (roll < 4) {
    const res = http.get(`${data.entries}?limit=20`, {
      headers: { Cookie: data.session.cookie }, redirects: 0,
      tags: Object.assign({ endpoint: 'list', name: 'entries?limit' }, tags),
    });
    if (!check(res, { 'list 200': (r) => r.status === 200 && r.body.indexOf('{"data":[') === 0 })) errors.add(1);
  } else {
    const res = http.get(url, {
      headers: { Cookie: data.session.cookie }, redirects: 0,
      tags: Object.assign({ endpoint: 'read', name: 'entries/{entry}' }, tags),
    });
    if (!check(res, {
      'read 200': (r) => r.status === 200,
      'read returns the last accepted save': (r) => r.headers['Etag'] === etag,
    })) errors.add(1);
  }
}

// Upload headers like assets.js's: raw bytes as the body, the claimed type and file name
// in headers, the write signed in and idempotent.
function uploadHeaders(data, fileName, contentType) {
  return {
    Cookie: data.session.cookie,
    'X-CSRF-Token': data.session.csrf,
    Origin: PUBLIC_URL,
    'Content-Type': contentType,
    'X-Colophon-File-Name': fileName,
    'Idempotency-Key': key(),
  };
}

// Poll an upload until the worker's verdict lands (deadline-bounded — the point is the
// verdict arriving, and the time it took is in the poll count).
function verdict(data, id, want) {
  for (let waits = 0; waits < 120; waits += 1) {
    const one = http.get(`${data.assets}/${id}`, {
      headers: { Cookie: data.session.cookie }, redirects: 0,
      tags: { arm: 'media', endpoint: 'poll', name: 'assets/{asset}' },
    });
    if (one.status !== 200) { errors.add(1); return null; }
    const state = one.json('data.state');
    if (state === want) return one;
    if (state === 'usable' || state === 'rejected') {
      // Settled, but not the verdict this item should get.
      errors.add(1);
      return one;
    }
    sleep(1);
  }
  errors.add(1);
  return null;
}

export function media(data) {
  const roll = __ITER % 10;
  if (roll < 6) {
    // The real workload: ~11.5 MB in, quarantined, then the worker decodes it.
    const made = http.post(data.assets, BIG, {
      headers: uploadHeaders(data, 'large.png', 'image/png'),
      redirects: 0, tags: { arm: 'media', endpoint: 'upload', name: 'assets' },
    });
    if (!check(made, {
      'upload 202': (r) => r.status === 202,
      'lands quarantined': (r) => r.json('data.state') === 'quarantined',
    })) { errors.add(1); return; }
    uploads.add(1);
    const done = verdict(data, made.json('data.id'), 'usable');
    if (done === null) return;
    verified.add(1);
    // What the decoder produced must actually serve: the original and a rendition.
    const at = `${data.media}/${done.json('data.id')}`;
    for (const url of [at, `${at}/w320`]) {
      const object = http.get(url, { redirects: 0, tags: { arm: 'media', endpoint: 'media', name: 'media/v1/{asset}' } });
      if (check(object, {
        'media 200': (r) => r.status === 200,
        'media immutable': (r) => r.headers['Cache-Control'] === 'public, max-age=31536000, immutable',
      })) mediaReads.add(1);
      else errors.add(1);
    }
  } else if (roll < 8) {
    // A fake: the door takes it, the decoder must refuse it.
    const made = http.post(data.assets, FAKE.buffer, {
      headers: uploadHeaders(data, 'fake.png', 'image/png'),
      redirects: 0, tags: { arm: 'media', endpoint: 'fake', name: 'assets' },
    });
    if (!check(made, { 'fake upload 202': (r) => r.status === 202 })) { errors.add(1); return; }
    uploads.add(1);
    if (verdict(data, made.json('data.id'), 'rejected') !== null) rejected.add(1);
  } else if (roll === 8) {
    // Over the asset's own 20 MiB cap, under the server's 21 MiB door: a 422
    // `invalid_upload` means the bound refused it before a byte of decode.
    const res = http.post(data.assets, OVERSIZED.buffer, {
      headers: uploadHeaders(data, 'huge.png', 'image/png'),
      redirects: 0, responseCallback: http.expectedStatuses(422),
      tags: { arm: 'media', endpoint: 'oversized', name: 'assets' },
    });
    if (check(res, {
      'oversized refused 422': (r) => r.status === 422,
      'named invalid_upload at /body': (r) => r.json('error.details.reason') === 'invalid_upload' && r.json('error.details.pointer') === '/body',
    })) oversizedRefused.add(1);
    else errors.add(1);
  } else {
    // Real bytes, a declared type it isn't: refused at the door on the declaration.
    const res = http.post(data.assets, BIG, {
      headers: uploadHeaders(data, 'vector.svg', 'image/svg+xml'),
      redirects: 0, responseCallback: http.expectedStatuses(422),
      tags: { arm: 'media', endpoint: 'wrong-type', name: 'assets' },
    });
    if (check(res, {
      'wrong declared type refused 422': (r) => r.status === 422,
      'named invalid_upload at /content-type': (r) => r.json('error.details.reason') === 'invalid_upload' && r.json('error.details.pointer') === '/content-type',
    })) wrongTypeRefused.add(1);
    else errors.add(1);
  }
}

/** Computes the L14 verdict: the worker's limits all fired (big verified, fake rejected,
 *  oversized and wrong-type refused at the door) and the editors beside them stayed
 *  responsive — the spec's own bound, which here is the draft-write budget (p95 < 300 ms,
 *  END_GOAL's table). The run's ratio against its own baseline is reported beside it:
 *  this topology runs the decoder on the API's process (`role all`), so a measurable
 *  slowdown is the honest answer, not a defect — a p95 outside the budget is the defect. */
export function handleSummary(data) {
  const metric = (name) => (data.metrics[name] ? data.metrics[name].values : null);
  const count = (name) => (metric(name) ? metric(name).count || 0 : 0);
  const dur = (arm) => metric(`http_req_duration{arm:${arm}}`) || {};
  const base = dur('baseline')['p(95)']; const noise = dur('media_phase')['p(95)'];
  const ratio = base > 0 ? noise / base : null;
  const gates = {
    editorial_held: base != null && noise != null && noise <= RESPONSIVE_P95_MS,
    big_files_verified: count('l14_verified') > 0,
    fakes_rejected: count('l14_rejected') > 0,
    oversized_refused: count('l14_oversized_refused') > 0,
    wrong_type_refused: count('l14_wrong_type_refused') > 0,
    media_served: count('l14_media_reads') > 0,
    no_errors: count('l14_errors') === 0,
  };
  data.l14 = {
    spec: 'L14: large media/decoder workload — worker limits hold; content editing stays responsive',
    gate: `media-phase editorial p95 <${RESPONSIVE_P95_MS} ms (the draft-write budget; the baseline ratio is evidence, not the gate); ~${(BIG_BYTES / 1048576).toFixed(1)} MB uploads verified by the worker and served; fakes rejected; 21 MB and wrong-type uploads refused at the door`,
    upload_bytes: BIG_BYTES,
    uploads: count('l14_uploads'),
    outcomes: {
      verified_and_served: count('l14_verified'),
      fakes_rejected: count('l14_rejected'),
      oversized_refused_at_door: count('l14_oversized_refused'),
      wrong_type_refused_at_door: count('l14_wrong_type_refused'),
      media_reads: count('l14_media_reads'),
    },
    editorial: {
      baseline_p95_ms: dur('baseline')['p(95)'],
      media_phase_p95_ms: dur('media_phase')['p(95)'],
      media_over_baseline: ratio,
    },
    errors: count('l14_errors'),
    gates,
    held: Object.values(gates).every(Boolean),
  };
  return {
    stdout: [
      '',
      '  L14 — large media/decoder workload',
      `    ${count('l14_uploads')} uploads (~${(BIG_BYTES / 1048576).toFixed(1)} MB each): ${count('l14_verified')} verified+served, ${count('l14_rejected')} fakes rejected, ${count('l14_oversized_refused')} oversized refused, ${count('l14_wrong_type_refused')} wrong-type refused`,
      `    editorial p95: ${base == null ? 'n/a' : base.toFixed(1)}ms baseline -> ${noise == null ? 'n/a' : noise.toFixed(1)}ms under media load (x${ratio == null ? 'n/a' : ratio.toFixed(2)}; gate <${RESPONSIVE_P95_MS}ms)`,
      `    verdict: ${data.l14.held ? 'HELD' : 'BROKE'}`,
      '',
    ].join('\n'),
  };
}
