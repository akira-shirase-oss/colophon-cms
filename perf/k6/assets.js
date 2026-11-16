// Load scenario for uploads and deletes (M.0, D-098): the asset lifecycle both ways. setup()
// signs in, makes a project and verifies one warm asset; each iteration uploads a real or a
// fake PNG into quarantine (the object's write to the store plus the asset and job rows in one
// transaction), lists the environment's assets, reads one back, or runs the delete arm — upload,
// wait for the worker's verdict, then DELETE, where 202 is requested, not gone. Verification
// and removal run off the request path, so the measured latency is the request's own.
// RATE counts iterations here (about 1.5 requests each, the delete arm more); scripts/load.sh
// gives this endpoint RATE 250 and P99_MS 700 — the same arm as the other external-call writes.

import http from 'k6/http';
import { check, fail, sleep } from 'k6';

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

// The suite's real 1x1 PNG and a fake that carries its name.
const PNG = new Uint8Array([
  137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0,
  0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 0, 0, 3, 1,
  1, 0, 201, 254, 146, 239, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
]);
const FAKE = new TextEncoder().encode('<html><body>not an image</body></html>');
// A 400×4 PNG: wider than w320, so the worker writes one rendition for the media arm to read.
const WIDE = new Uint8Array([
  137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 1, 144, 0, 0, 0, 4, 8, 2,
  0, 0, 0, 47, 93, 254, 122, 0, 0, 0, 42, 73, 68, 65, 84, 120, 156, 237, 206, 65, 9, 0, 48, 12,
  4, 176, 74, 159, 243, 206, 194, 253, 142, 66, 32, 2, 50, 243, 22, 224, 134, 254, 0, 32, 212,
  31, 0, 132, 250, 3, 128, 204, 7, 24, 121, 90, 72, 220, 227, 41, 34, 0, 0, 0, 0, 73, 69, 78,
  68, 174, 66, 96, 130,
]);

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
  const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: 'k6 assets' }), { headers: write });
  if (project.status !== 201) fail(`project: HTTP ${project.status} ${project.body}`);
  const id = project.json('data.id');
  const assets = `${BASE_URL}/api/v1/projects/${id}/environments/production/assets`;

  // One known-usable asset for the public origin to serve: upload, then wait for the
  // worker to verify it before the run starts.
  const made = http.post(assets, WIDE.buffer, {
    headers: { Cookie: session.cookie, 'X-CSRF-Token': session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'image/png', 'X-Colophon-File-Name': 'warm.png', 'Idempotency-Key': `k6-warm-${Date.now().toString(36)}` },
    redirects: 0,
  });
  if (made.status !== 202) fail(`warm upload: HTTP ${made.status} ${made.body}`);
  const mediaId = made.json('data.id');
  for (let waits = 0; waits < 100; waits += 1) {
    const one = http.get(`${assets}/${mediaId}`, { headers: { Cookie: session.cookie }, redirects: 0 });
    if (one.json('data.state') === 'usable') break;
    if (waits === 99) fail(`warm asset never verified: ${one.body}`);
  }

  return { session, assets, media: `${BASE_URL}/media/v1/${id}/production`, mediaId };
}

export default function (data) {
  const read = { headers: { Cookie: data.session.cookie }, redirects: 0 };
  const write = (fileName) => ({
    headers: {
      Cookie: data.session.cookie,
      'X-CSRF-Token': data.session.csrf,
      Origin: PUBLIC_URL,
      'Content-Type': 'image/png',
      'X-Colophon-File-Name': fileName,
      'Idempotency-Key': key(),
    },
    redirects: 0,
  });
  const roll = __ITER % 10;
  if (roll < 4) {
    // Half the uploads are real, one in ten a fake the verifier will reject.
    const fake = __ITER % 50 === 49;
    const body = fake ? FAKE : PNG;
    const res = http.post(data.assets, body.buffer, Object.assign({ tags: { endpoint: 'upload', name: 'assets' } }, write(fake ? 'photo.png' : 'dot.png')));
    check(res, {
      'upload 202': (r) => r.status === 202,
      'upload lands quarantined': (r) => r.json('data.state') === 'quarantined',
    });
  } else if (roll === 4) {
    // D-098: an unreferenced upload asked to go — 202 means the delete job is queued, not that
    // the object is gone. A delete can't touch quarantine, so the upload's verdict comes first.
    const made = http.post(data.assets, PNG.buffer, Object.assign({ tags: { endpoint: 'delete-upload', name: 'assets' } }, write('going.png')));
    check(made, { 'delete upload 202': (r) => r.status === 202 });
    const id = made.json('data.id');
    if (typeof id === 'string') {
      for (let waits = 0; waits < 100; waits += 1) {
        const one = http.get(`${data.assets}/${id}`, Object.assign({ tags: { endpoint: 'delete-wait', name: 'assets/{asset}' } }, read));
        if (one.json('data.state') !== 'quarantined') break;
        sleep(0.05);
      }
      const gone = http.del(`${data.assets}/${id}`, null, Object.assign({ tags: { endpoint: 'delete', name: 'assets/{asset}' } }, {
        headers: {
          Cookie: data.session.cookie,
          'X-CSRF-Token': data.session.csrf,
          Origin: PUBLIC_URL,
          'Idempotency-Key': key(),
        },
        redirects: 0,
      }));
      check(gone, {
        'delete 202': (r) => r.status === 202,
        'delete names the asset': (r) => r.json('data.id') === id,
      });
    }
  } else if (roll < 8) {
    const res = http.get(data.assets, Object.assign({ tags: { endpoint: 'list', name: 'assets' } }, read));
    check(res, { 'list 200': (r) => r.status === 200 });
  } else if (roll === 8) {
    const res = http.get(`${data.assets}?limit=1`, Object.assign({ tags: { endpoint: 'list-one', name: 'assets?limit' } }, read));
    const id = res.json('data.0.id');
    check(res, { 'list-one 200': (r) => r.status === 200 });
    if (typeof id === 'string') {
      const one = http.get(`${data.assets}/${id}`, Object.assign({ tags: { endpoint: 'read', name: 'assets/{asset}' } }, read));
      check(one, {
        'read 200': (r) => r.status === 200,
        'read settles or waits': (r) => ['quarantined', 'usable', 'rejected'].includes(r.json('data.state')),
      });
    }
  } else {
    // M.1/M.3: the public origin serves the setup's verified object — the original half the
    // time, its w320 rendition the other half — no sign-in, no row reads.
    const at = __ITER % 2 === 0 ? `${data.media}/${data.mediaId}` : `${data.media}/${data.mediaId}/w320`;
    const object = http.get(at, { tags: { endpoint: 'media', name: 'media/v1/{asset}' }, redirects: 0 });
    check(object, {
      'media 200': (r) => r.status === 200,
      'media immutable': (r) => r.headers['Cache-Control'] === 'public, max-age=31536000, immutable',
    });
  }
}
