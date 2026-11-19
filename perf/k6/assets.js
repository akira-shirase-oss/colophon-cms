// Load scenario for uploads and deletes (M.0, D-098): the asset lifecycle both ways. setup()
// signs in, makes a project and verifies one warm asset; each iteration uploads a real or a
// fake PNG into quarantine (the object's write to the store plus the asset and job rows in one
// transaction), lists the environment's assets a page and its next at a time, searches them,
// changes the warm asset's metadata under If-Match (D-179), reads one back, or runs the delete arm — upload,
// wait for the worker's verdict, then DELETE, where 202 is requested, not gone. Verification
// and removal run off the request path, so the measured latency is the request's own.
// RATE counts iterations here (about 1.5 requests each, the delete arm more); scripts/load.sh
// gives this endpoint RATE 250 and P99_MS 700 — the same arm as the other external-call writes.

import http from 'k6/http';
import { check, fail, sleep } from 'k6';
import { Counter, Rate, Trend } from 'k6/metrics';
import exec from 'k6/execution';

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

// D-218's evidence: how long an upload waits to leave quarantine, as the delete arm sees it (from the
// upload's answer to the first read that isn't quarantined, or to giving up), and how many deletes met an
// upload still being verified — the server's honest `409 asset_quarantined`, counted apart from failures.
const verifyWait = new Trend('verify_wait_ms', true);
const notYetVerified = new Counter('not_yet_verified');

// Uploads refused for load (429 too_many_uploads, 503): a hard failure of the smoke when 1% or more of them are
// (the record's hard_failures.upload_refused), so a run that measured refusals can't pass as uploads.
const uploadRefused = new Rate('upload_refused');

// Every state the API documents for an asset, read from the committed OpenAPI document (docs/api/openapi.json,
// `AssetView.state`), so a state added there can't make a correct read fail this smoke. The newest asset a
// read finds may be one another VU just asked to delete (`deleting`), which the hand-written list of three
// refused: 10 reads in CI 44630577830. open() runs at init, relative to this file.
const ASSET_STATES = (() => {
  const spec = JSON.parse(open('../../docs/api/openapi.json'));
  const states = spec.components?.schemas?.AssetView?.properties?.state?.enum;
  if (!Array.isArray(states) || !states.includes('usable')) throw new Error('docs/api/openapi.json has no AssetView.state enum: the read check needs it');
  return states;
})();

const thresholds = { http_req_failed: ['rate==0'], checks: ['rate==1'], http_req_duration: [`p(99)<${P99_MS}`], upload_refused: ['rate<0.01'] };
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
  // One session per VU, as clients have (D-215): a session or token may have 4 uploads in flight and
  // the Studio sends 3 at a time, so 64 VUs sharing one session measured 429 too_many_uploads, not
  // uploads (CI 44608263509: 3,598 of 4,096 uploads refused). load.sh lifts the password limit.
  const vus = SCENARIO === 'steady' ? VUS * 2 : VUS;
  const sessions = [session];
  for (let i = 1; i < vus; i++) {
    const again = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }), { headers: { 'Content-Type': 'application/json' } });
    if (again.status !== 204) fail(`sign-in ${i + 1} of ${vus} failed: HTTP ${again.status}`);
    const cookies = http.cookieJar().cookiesForURL(PUBLIC_URL);
    sessions.push({ cookie: `colophon_session=${cookies.colophon_session[0]}`, csrf: cookies.colophon_csrf[0] });
  }

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
  // Poll on a clock, not a count: back-to-back GETs once spent their 100 tries in a
  // quarter of a second, before the worker had picked the job up (CI 44447828333).
  const started = Date.now();
  for (;;) {
    const one = http.get(`${assets}/${mediaId}`, { headers: { Cookie: session.cookie }, redirects: 0 });
    const state = one.json('data.state');
    if (state === 'usable') break;
    const waited = Date.now() - started;
    if (waited >= 30000) fail(`warm asset never verified: still ${state} after ${waited} ms: ${one.body}`);
    sleep(0.1);
  }

  return { session, sessions, assets, media: `${BASE_URL}/media/v1/${id}/production`, mediaId };
}

export default function (data) {
  const session = data.sessions[(__VU - 1) % data.sessions.length];
  const read = { headers: { Cookie: session.cookie }, redirects: 0 };
  const write = (fileName) => ({
    headers: {
      Cookie: session.cookie,
      'X-CSRF-Token': session.csrf,
      Origin: PUBLIC_URL,
      'Content-Type': 'image/png',
      'X-Colophon-File-Name': fileName,
      'Idempotency-Key': key(),
    },
    redirects: 0,
  });
  const roll = exec.scenario.iterationInTest % 10;
  if (roll < 4) {
    // Half the uploads are real, one in ten a fake the verifier will reject.
    const fake = exec.scenario.iterationInTest % 50 === 49;
    const body = fake ? FAKE : PNG;
    const res = http.post(data.assets, body.buffer, Object.assign({ tags: { endpoint: 'upload', name: 'assets' } }, write(fake ? 'photo.png' : 'dot.png')));
    uploadRefused.add(res.status === 429 || res.status === 503);
    check(res, {
      'upload 202': (r) => r.status === 202,
      'upload lands quarantined': (r) => r.json('data.state') === 'quarantined',
    });
  } else if (roll === 4) {
    // D-098: an unreferenced upload asked to go — 202 means the delete job is queued, not that
    // the object is gone. A delete can't touch quarantine, so the upload's verdict comes first.
    const made = http.post(data.assets, PNG.buffer, Object.assign({ tags: { endpoint: 'delete-upload', name: 'assets' } }, write('going.png')));
    uploadRefused.add(made.status === 429 || made.status === 503);
    check(made, { 'delete upload 202': (r) => r.status === 202 });
    const id = made.json('data.id');
    if (typeof id === 'string') {
      const uploaded = Date.now();
      for (let waits = 0; waits < 100; waits += 1) {
        const one = http.get(`${data.assets}/${id}`, Object.assign({ tags: { endpoint: 'delete-wait', name: 'assets/{asset}' } }, read));
        if (one.json('data.state') !== 'quarantined') break;
        sleep(0.05);
      }
      verifyWait.add(Date.now() - uploaded);
      const gone = http.del(`${data.assets}/${id}`, null, Object.assign({ tags: { endpoint: 'delete', name: 'assets/{asset}' } }, {
        headers: {
          Cookie: session.cookie,
          'X-CSRF-Token': session.csrf,
          Origin: PUBLIC_URL,
          'Idempotency-Key': key(),
        },
        redirects: 0,
        // A delete of an upload still being verified is refused, honestly: not a failed request.
        responseCallback: http.expectedStatuses(202, 409),
      }));
      const quarantined = gone.status === 409 && gone.json('error.details.reason') === 'asset_quarantined';
      if (quarantined) notYetVerified.add(1);
      check(gone, {
        'delete 202, or 409 asset_quarantined (not yet verified)': (r) => r.status === 202 || quarantined,
        'delete names the asset': (r) => r.status !== 202 || r.json('data.id') === id,
      });
    }
  } else if (roll === 5) {
    // D-179: the first page and, through its cursor, the next — keyset, so neither shifts.
    const res = http.get(`${data.assets}?limit=50`, Object.assign({ tags: { endpoint: 'list', name: 'assets' } }, read));
    check(res, { 'list 200': (r) => r.status === 200 });
    const next = res.json('next_cursor');
    if (typeof next === 'string') {
      const page = http.get(`${data.assets}?limit=50&cursor=${encodeURIComponent(next)}`, Object.assign({ tags: { endpoint: 'list-page', name: 'assets?cursor' } }, read));
      check(page, { 'next page 200': (r) => r.status === 200 && Array.isArray(r.json('data')) });
    }
  } else if (roll === 6) {
    // D-179: search over the trigram index — a common term and a rare one.
    const q = exec.scenario.iterationInTest % 20 === 6 ? 'warm' : 'dot';
    const res = http.get(`${data.assets}?q=${q}&limit=50`, Object.assign({ tags: { endpoint: 'search', name: 'assets?q' } }, read));
    check(res, { 'search 200': (r) => r.status === 200 && Array.isArray(r.json('data')) });
  } else if (roll === 7) {
    // D-179: metadata under If-Match on the warm asset every VU shares — a lost race is the
    // honest 409 stale_version, never a silent overwrite.
    const one = http.get(`${data.assets}/${data.mediaId}`, Object.assign({ tags: { endpoint: 'read', name: 'assets/{asset}' } }, read));
    const etag = one.headers.Etag;
    const res = http.patch(`${data.assets}/${data.mediaId}`, JSON.stringify({ title: `Warm ${__VU}-${__ITER}`, tags: ['warm', `vu-${__VU}`] }), {
      headers: { Cookie: session.cookie, 'X-CSRF-Token': session.csrf, Origin: PUBLIC_URL, 'Content-Type': 'application/json', 'If-Match': etag, 'Idempotency-Key': key() },
      redirects: 0,
      tags: { endpoint: 'patch', name: 'assets/{asset}' },
      responseCallback: http.expectedStatuses(200, 409),
    });
    check(res, {
      'patch 200, or 409 stale_version after a lost race': (r) => r.status === 200 || (r.status === 409 && r.json('error.details.reason') === 'stale_version'),
    });
  } else if (roll === 8) {
    const res = http.get(`${data.assets}?limit=1`, Object.assign({ tags: { endpoint: 'list-one', name: 'assets?limit' } }, read));
    const id = res.json('data.0.id');
    check(res, { 'list-one 200': (r) => r.status === 200 });
    if (typeof id === 'string') {
      const one = http.get(`${data.assets}/${id}`, Object.assign({ tags: { endpoint: 'read', name: 'assets/{asset}' } }, read));
      check(one, {
        'read 200': (r) => r.status === 200,
        'read settles or waits': (r) => ASSET_STATES.includes(r.json('data.state')),
      });
    }
  } else {
    // M.1/M.3: the public origin serves the setup's verified object — the original half the
    // time, its w320 rendition the other half — no sign-in, no row reads.
    const at = exec.scenario.iterationInTest % 2 === 0 ? `${data.media}/${data.mediaId}` : `${data.media}/${data.mediaId}/w320`;
    const object = http.get(at, { tags: { endpoint: 'media', name: 'media/v1/{asset}' }, redirects: 0 });
    check(object, {
      'media 200': (r) => r.status === 200,
      // The bare address follows the current version, so it revalidates (D-157).
        'media revalidates': (r) => r.headers['Cache-Control'] === 'public, max-age=60, must-revalidate',
    });
  }
}
