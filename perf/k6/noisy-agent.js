// Load scenario for L08 (spec §30; D-120): measured proof that a noisy agent is throttled
// while the editors beside it keep their latency, and that nothing about the throttling
// reaches another project. One run holds both halves of the evidence in two sequential
// phases of DURATION each: first editorial traffic alone — the mixed-workload baseline,
// project A's editors reading, listing, searching and saving drafts — then the same
// traffic at the same rate while an API token with a small `requests_per_minute` ceiling
// (AGENT_RPM, 60 by default) hammers the collection's entries list at AGENT_RATE attempts
// a second, and an editor in a SECOND project works through both phases untouched.
//
// The agent's ceiling is a GCRA cell in the admission limiter checked where its scoped
// transaction opens, before any handler work (D-118): at hundreds of attempts a second
// almost every request answers 429 `rate_limited` with `Retry-After` — and the refusal is
// real work (the token row read, the Valkey eval, the audit episode's write), which is
// exactly the noise L08 wants measured rather than assumed away.
//
// Every request is tagged by arm (`baseline`, `noise`, `b_baseline`, `b_noise`, `agent`),
// so k6 keeps each arm's own latency series, and handleSummary() then computes the
// comparison the spec asks for — under-noise editorial p95 against the baseline p95 the
// same run just measured, and the second project's p95 against its own before/after —
// into the exported summary's `l08` object: both phases' numbers, both ratios, the agent's
// refused share, and the verdict. scripts/load.sh stores that object in the result file
// and fails the run when `held` is false. The bound beside each 2x is a 2 ms
// scheduler-jitter epsilon — the same convention load.sh's regression gate already allows
// (+1 ms on its p99 comparison) — so a sub-millisecond baseline on a fast machine can't
// fail on noise the run never made.
//
// The arms are time-boxed, not iteration-counted: TOTAL and VUS are unused, each arm
// sizes its own VUs from its rate (A_VUS, B_VUS, AGENT_VUS override). RATE is project A's
// editorial rate, B_RATE the second project's, AGENT_RATE the agent's attempts; DURATION
// is the length of EACH phase, so a run lasts a little over 2x DURATION (the noise phase
// starts GAP seconds after the baseline phase ends, so nothing in flight bleeds across).
// `steady` and `throughput` run the same phased plan — the evidence run is steady (60 s
// an arm, ~15k requests an editorial arm); CI's smoke passes a short DURATION.
// scripts/load.sh gives this endpoint RATE 250 and P99_MS 250.

import http from 'k6/http';
import { check, fail } from 'k6';
import exec from 'k6/execution';
import { Rate } from 'k6/metrics';

const BASE_URL = __ENV.BASE_URL || 'http://127.0.0.1:58000';
const PUBLIC_URL = __ENV.PUBLIC_URL || 'http://localhost:58000';
const SCENARIO = __ENV.SCENARIO || 'throughput';
const RATE = parseInt(__ENV.RATE || '250', 10);
const B_RATE = parseInt(__ENV.B_RATE || `${Math.max(10, Math.round(RATE / 5))}`, 10);
const AGENT_RATE = parseInt(__ENV.AGENT_RATE || '600', 10);
const AGENT_RPM = parseInt(__ENV.AGENT_RPM || '60', 10);
const DURATION = __ENV.DURATION || '60s';
const P99_MS = parseFloat(__ENV.P99_MS || '250');
const P95_CEIL = parseFloat(__ENV.P95_CEIL || '700');
const A_VUS = parseInt(__ENV.A_VUS || `${Math.max(16, Math.ceil(RATE / 10))}`, 10);
const B_VUS = parseInt(__ENV.B_VUS || `${Math.max(8, Math.ceil(B_RATE / 10))}`, 10);
const AGENT_VUS = parseInt(__ENV.AGENT_VUS || `${Math.max(32, Math.ceil(AGENT_RATE / 15))}`, 10);

function durationSeconds(text) {
  const match = /^(\d+)(s|m)$/.exec(text);
  if (!match) throw new Error(`DURATION must look like 60s or 5m, got ${text}`);
  return parseInt(match[1], 10) * (match[2] === 'm' ? 60 : 1);
}

const PHASE = durationSeconds(DURATION);
const GAP = 2; // seconds between the phases, so nothing in flight bleeds across
// D-221: the quiet and noisy phases alternate in short windows, A B A B A B, and the ratio is taken from the
// medians of each phase's per-window p95s, so a stall of the runner corrupts one window, not the verdict (one
// 8-second baseline either caught a stall or didn't: 4.76 ms → 8.64x on one run, 53.7 ms → 1.04x on another).
const WINDOWS = parseInt(__ENV.L08_WINDOWS || '3', 10);
const WINDOW = Math.max(2, Math.ceil(PHASE / WINDOWS));
// Each executor's VUs can grow to twice their preallocated count; a VU's idInTest is
// global across every scenario, so the article pools must cover the largest id any VU
// can hold — the sum of all five executors' maxima.
const ARTICLES = A_VUS * 4 + B_VUS * 4 + AGENT_VUS * 2;

if (!(SCENARIO === 'throughput' || SCENARIO === 'steady')) {
  throw new Error('SCENARIO must be throughput or steady');
}

// A constant-rate arm of `vus` VUs for one window, starting `start` seconds in.
function arm(rate, start, vus, execFn) {
  return {
    executor: 'constant-arrival-rate',
    rate,
    timeUnit: '1s',
    duration: `${WINDOW}s`,
    // Shorter than the gap, so windows never overlap: with k6's default 30 s, the agent's overrun spilled into the
    // next quiet window and k6 planned VUs for all of them at once, more than the article pools cover.
    gracefulStop: `${GAP - 1}s`,
    startTime: `${start}s`,
    preAllocatedVUs: vus,
    maxVUs: vus * 2,
    exec: execFn,
  };
}

// The agent's share that is refused: over a 60-a-minute window at hundreds of attempts a
// second almost every request is refused; the window's own earnings (~1 a second plus
// whatever the idle baseline phase let it save) admit the rest.
const refusals = new Rate('agent_refusals');

const thresholds = {
  checks: ['rate==1'],
  http_req_failed: ['rate==0'],
  agent_refusals: ['rate>0.9'],
  http_req_duration: [`p(99)<${P99_MS}`],
  // Every arm must actually have run, and each arm's series lands in the exported
  // summary because a threshold names it; the p95 ceilings are sanity bounds (END_GOAL's
  // editorial write budget), the sharp gate is the ratio handleSummary() computes.
  'http_reqs{arm:baseline}': ['count>0'],
  'http_reqs{arm:noise}': ['count>0'],
  'http_reqs{arm:b_baseline}': ['count>0'],
  'http_reqs{arm:b_noise}': ['count>0'],
  'http_reqs{arm:agent}': ['count>0'],
  'http_req_duration{arm:baseline}': [`p(95)<${P95_CEIL}`],
  'http_req_duration{arm:noise}': [`p(95)<${P95_CEIL}`],
  'http_req_duration{arm:b_baseline}': [`p(95)<${P95_CEIL}`],
  'http_req_duration{arm:b_noise}': [`p(95)<${P95_CEIL}`],
  'http_req_duration{arm:agent}': ['p(99)<60000'],
};
// Each window's series lands in the exported summary because a threshold names it.
for (let w = 0; w < WINDOWS; w += 1) {
  for (const which of ['baseline', 'noise', 'b_baseline', 'b_noise']) {
    thresholds[`http_req_duration{arm:${which},w:${w}}`] = [`p(95)<${P95_CEIL}`];
  }
}
if (SCENARIO === 'steady') {
  const scheduled = ((RATE + B_RATE) * 2 + AGENT_RATE) * WINDOW * WINDOWS;
  thresholds.dropped_iterations = [`count<=${Math.max(1, Math.floor(scheduled * 0.0001))}`];
}

export const options = {
  // Window w: quiet from w*(2*(WINDOW+GAP)), noisy WINDOW+GAP later; the agent hammers only in the noisy ones.
  scenarios: Object.fromEntries(
    Array.from({ length: WINDOWS }, (_, w) => {
      const quiet = w * 2 * (WINDOW + GAP);
      const noisy = quiet + WINDOW + GAP;
      return [
        [`a_baseline_${w}`, arm(RATE, quiet, A_VUS, 'editorial')],
        [`b_baseline_${w}`, arm(B_RATE, quiet, B_VUS, 'editorial')],
        [`a_noise_${w}`, arm(RATE, noisy, A_VUS, 'editorial')],
        [`b_noise_${w}`, arm(B_RATE, noisy, B_VUS, 'editorial')],
        [`agent_${w}`, arm(AGENT_RATE, noisy, AGENT_VUS, 'hammer')],
      ];
    }).flat(),
  ),
  thresholds,
  summaryTrendStats: ['avg', 'min', 'med', 'p(90)', 'p(95)', 'p(99)', 'p(99.9)', 'max'],
  setupTimeout: '300s',
};

// Which pool, arm tag and window an `editorial` VU works, by the scenario that scheduled it (`a_noise_2`, …).
const ARMS = {
  a_baseline: { pool: 'a', arm: 'baseline' },
  b_baseline: { pool: 'b', arm: 'b_baseline' },
  a_noise: { pool: 'a', arm: 'noise' },
  b_noise: { pool: 'b', arm: 'b_noise' },
};
function editorialOf(scenario) {
  const cut = scenario.lastIndexOf('_');
  return { ...ARMS[scenario.slice(0, cut)], w: scenario.slice(cut + 1) };
}

/** Signs `username` in with a cookie jar of its own; the headers a browser tab would send. */
function signIn(username) {
  const jar = new http.CookieJar();
  const signedIn = http.post(`${PUBLIC_URL}/auth/signin`, JSON.stringify({ email: `${username}@colophon.test`, password: 'colophon-dev' }), { jar, headers: { 'Content-Type': 'application/json' } });
  if (signedIn.status !== 204) fail(`sign-in as ${username} failed: HTTP ${signedIn.status}`);
  const cookies = jar.cookiesForURL(PUBLIC_URL);
  return {
    Cookie: `colophon_session=${cookies.colophon_session[0]}`, 'X-CSRF-Token': cookies.colophon_csrf[0], Origin: PUBLIC_URL,
    'Content-Type': 'application/json',
  };
}

function article(title, text) {
  return JSON.stringify({
    title,
    body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] } },
  });
}

function key() {
  return `k6-${__VU}-${__ITER}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

export function setup() {
  const owner = signIn('dev-owner');
  const stamp = Date.now().toString(36);
  const make = (name) => {
    const slug = `k6-${name}-${stamp}`;
    const project = http.post(`${BASE_URL}/api/v1/projects`, JSON.stringify({ slug, name: `k6 L08 ${name}` }), { headers: owner });
    if (project.status !== 201) fail(`project ${name}: HTTP ${project.status} ${project.body}`);
    const base = `${BASE_URL}/api/v1/projects/${project.json('data.id')}`;
    return { base, entries: `${base}/environments/production/collections/articles/entries` };
  };
  const a = make('a');
  const b = make('b');

  // The noisy agent: a read-only token on project A whose `requests_per_minute` ceiling
  // is a few hundred times below what the arm asks for (D-118's quota, set up through the
  // same route a project would use).
  const made = http.post(`${a.base}/tokens`, JSON.stringify({
    name: 'Noisy agent', kind: 'read-only', expires_in_days: 1,
    limits: { requests_per_minute: AGENT_RPM },
  }), { headers: owner });
  if (made.status !== 201) fail(`token: HTTP ${made.status} ${made.body}`);
  const secret = made.json('data.secret');

  // One article for every VU id that can exist (idInTest is global across the run's
  // scenarios), so no two editorial VUs ever save the same article or race an etag.
  const plant = (project) => {
    const articles = new Array(ARTICLES);
    for (let i = 0; i < ARTICLES; i += 40) {
      const batch = [];
      for (let j = i; j < Math.min(i + 40, ARTICLES); j += 1) {
        batch.push(['POST', project.entries, article(`Article ${j}`, 'Start'), {
          headers: Object.assign({ 'Idempotency-Key': `k6-l08-${stamp}-${project.base.slice(-6)}-${j}` }, owner),
          tags: { name: 'setup' },
        }]);
      }
      const replies = http.batch(batch);
      for (let k = 0; k < replies.length; k += 1) {
        if (replies[k].status !== 201) fail(`article ${i + k}: HTTP ${replies[k].status} ${replies[k].body}`);
        articles[i + k] = { id: replies[k].json('data.id'), etag: replies[k].headers['Etag'] };
      }
    }
    return articles;
  };
  a.articles = plant(a);
  b.articles = plant(b);
  return { owner, a, b, secret };
}

// Per VU, per article: the ETag of the last revision this VU saved. An article is reached only by the VU whose id
// picks it, so a tag is never another writer's; it is keyed by article because k6 reuses a VU across the short
// windows (D-221), so one VU can work pool A's article in one window and pool B's in the next.
const etags = {};

/** One editorial iteration — a read, a list, a title search or a draft save — on the
 *  project and under the arm tag of the scenario that scheduled this VU. */
export function editorial(data) {
  const which = editorialOf(exec.scenario.name);
  const project = data[which.pool];
  const mine = project.articles[(exec.vu.idInTest - 1) % project.articles.length];
  const etag = etags[mine.id] ?? mine.etag;
  const url = `${project.entries}/${mine.id}`;
  const tags = { arm: which.arm, w: which.w };
  const roll = exec.scenario.iterationInTest % 10;

  if (roll < 2) {
    const headers = {
      Cookie: data.owner.Cookie, 'X-CSRF-Token': data.owner['X-CSRF-Token'], Origin: PUBLIC_URL, 'Content-Type': 'application/json',
      'If-Match': etag, 'Idempotency-Key': key(),
    };
    const res = http.put(url, article(`Article by VU ${exec.vu.idInTest}`, `Iteration ${__ITER}`), {
      headers, redirects: 0, tags: Object.assign({ endpoint: 'save', name: 'entries/{entry}' }, tags),
    });
    const saved = check(res, {
      'save 200': (r) => r.status === 200,
      'save gives a new ETag': (r) => typeof r.headers['Etag'] === 'string' && r.headers['Etag'] !== etag,
    });
    if (saved) etags[mine.id] = res.headers['Etag'];
  } else if (roll < 4) {
    const res = http.get(`${project.entries}?limit=20`, {
      headers: { Cookie: data.owner.Cookie }, redirects: 0,
      tags: Object.assign({ endpoint: 'list', name: 'entries?limit' }, tags),
    });
    check(res, {
      'list 200': (r) => r.status === 200,
      'list is a page': (r) => r.body.indexOf('{"data":[') === 0,
    });
  } else if (roll < 5) {
    const q = `Article`;
    const res = http.get(`${project.entries}?q=${encodeURIComponent(q)}&limit=10`, {
      headers: { Cookie: data.owner.Cookie }, redirects: 0,
      tags: Object.assign({ endpoint: 'search', name: 'entries?q' }, tags),
    });
    check(res, {
      'search 200': (r) => r.status === 200,
      'search finds only matching titles': (r) => {
        const rows = r.json('data');
        return Array.isArray(rows) && rows.every((row) => row.revision.title.toLowerCase().indexOf(q.toLowerCase()) !== -1);
      },
    });
  } else {
    const res = http.get(url, {
      headers: { Cookie: data.owner.Cookie }, redirects: 0,
      tags: Object.assign({ endpoint: 'read', name: 'entries/{entry}' }, tags),
    });
    check(res, {
      'read 200': (r) => r.status === 200,
      'read returns the last accepted save': (r) => r.headers['Etag'] === etag,
    });
  }
}

/** One agent iteration: the token asks for the entries list again, whatever its window
 *  says — almost every answer is the quota's 429 `rate_limited` with `Retry-After`. */
export function hammer(data) {
  const res = http.get(`${data.a.entries}?limit=20`, {
    headers: { Authorization: `Bearer ${data.secret}` },
    redirects: 0,
    tags: { arm: 'agent', endpoint: 'agent', name: 'agent-list' },
    responseCallback: http.expectedStatuses(200, 429),
  });
  refusals.add(res.status === 429);
  check(res, {
    'agent: refused, or the window’s rare admit': (r) => r.status === 429 || r.status === 200,
    'a refusal says rate_limited with Retry-After': (r) =>
      r.status !== 429 || (r.headers['Retry-After'] !== undefined && r.json('error.details.reason') === 'rate_limited'),
  });
}

// The scheduler-jitter bound beside each 2x (see the header): what "held" is computed on.
const EPSILON_MS = 2;

/** Computes the L08 verdict from the run's own measurements and attaches it to the
 *  exported summary as `l08`: each arm's request count and p95, the two ratios the spec
 *  bounds, the agent's refused share, and whether the gate held. load.sh stores it and
 *  fails a run whose `held` is false. */
export function handleSummary(data) {
  const metric = (name) => {
    const m = data.metrics[name];
    if (!m) return null;
    return m.values || m;
  };
  const count = (arm) => (metric(`http_reqs{arm:${arm}}`) || {}).count || 0;
  const p95 = (arm) => (metric(`http_req_duration{arm:${arm}}`) || {})['p(95)'];
  const p99 = (arm) => (metric(`http_req_duration{arm:${arm}}`) || {})['p(99)'];
  const refused = metric('agent_refusals') || {};
  const refusedShare = refused.rate !== undefined ? refused.rate : refused.value || 0;
  const refusedCount = refused.passes || 0;

  // The median of an arm's per-window p95s: one stalled window moves it by one rank, not to its own value.
  const windowsOf = (arm) => Array.from({ length: WINDOWS }, (_, w) => (metric(`http_req_duration{arm:${arm},w:${w}}`) || {})['p(95)'])
    .filter((v) => v != null);
  const median = (values) => {
    if (!values.length) return null;
    const sorted = [...values].sort((x, y) => x - y);
    const mid = Math.floor(sorted.length / 2);
    return sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
  };
  const base = median(windowsOf('baseline'));
  const noise = median(windowsOf('noise'));
  const bBase = median(windowsOf('b_baseline'));
  const bNoise = median(windowsOf('b_noise'));
  const ratio = base > 0 ? noise / base : null;
  const bRatio = bBase > 0 ? bNoise / bBase : null;
  // The single-window ratio the gate used to take, kept beside the medians for a week as a comparison (D-221).
  const single = {
    editorial_noise_over_baseline: p95('baseline') > 0 ? p95('noise') / p95('baseline') : null,
    project_b_noise_over_baseline: p95('b_baseline') > 0 ? p95('b_noise') / p95('b_baseline') : null,
  };
  const gates = {
    agent_throttled: refusedShare > 0.9,
    editorial_held: base != null && noise != null && noise <= 2 * base + EPSILON_MS,
    cross_project_held: bBase != null && bNoise != null && bNoise <= 2 * bBase + EPSILON_MS,
  };
  data.l08 = {
    spec: 'L08: one noisy agent plus active editors — the agent is throttled; editorial p95 stays within 2x the mixed-workload baseline; no cross-project starvation',
    gate: 'medians of the per-window p95s — editorial: noise_p95 <= 2*baseline_p95 + 2ms; project B: b_noise_p95 <= 2*b_baseline_p95 + 2ms; agent: refused share > 0.9, 429 rate_limited with Retry-After',
    phases_seconds: { windows: WINDOWS, window: WINDOW, gap: GAP },
    window_p95_ms: { baseline: windowsOf('baseline'), noise: windowsOf('noise'), b_baseline: windowsOf('b_baseline'), b_noise: windowsOf('b_noise') },
    single_window: single,
    agent: { ceiling_per_minute: AGENT_RPM, attempted_per_second: AGENT_RATE },
    arms: {
      editorial_baseline: { requests: count('baseline'), p95_ms: base, p99_ms: p99('baseline') },
      editorial_noise: { requests: count('noise'), p95_ms: noise, p99_ms: p99('noise') },
      project_b_baseline: { requests: count('b_baseline'), p95_ms: bBase, p99_ms: p99('b_baseline') },
      project_b_noise: { requests: count('b_noise'), p95_ms: bNoise, p99_ms: p99('b_noise') },
      agent: { requests: count('agent'), refused: refusedCount, refused_share: refusedShare, p95_ms: p95('agent'), p99_ms: p99('agent') },
    },
    ratios: { editorial_noise_over_baseline: ratio, project_b_noise_over_baseline: bRatio },
    gates,
    held: gates.agent_throttled && gates.editorial_held && gates.cross_project_held,
  };
  const fmt = (v) => (v == null ? 'n/a' : `${v.toFixed(1)}ms`);
  return {
    stdout: [
      '',
      '  L08 — one noisy agent plus active editors',
      `    agent: ${(refusedShare * 100).toFixed(1)}% refused of ${count('agent')} (429 rate_limited + Retry-After, ceiling ${AGENT_RPM}/min at ~${AGENT_RATE}/s attempted)`,
      `    editorial p95: ${fmt(base)} baseline -> ${fmt(noise)} under noise (x${ratio == null ? 'n/a' : ratio.toFixed(2)}; gate <=x2 +${EPSILON_MS}ms): ${gates.editorial_held ? 'held' : 'BROKE'}`,
      `    project B p95: ${fmt(bBase)} baseline -> ${fmt(bNoise)} under noise (x${bRatio == null ? 'n/a' : bRatio.toFixed(2)}): ${gates.cross_project_held ? 'held' : 'BROKE'}`,
      `    (medians of ${WINDOWS} windows of ${WINDOW}s; the single-window ratio would be x${single.editorial_noise_over_baseline == null ? 'n/a' : single.editorial_noise_over_baseline.toFixed(2)})`,
      `    verdict: ${data.l08.held ? 'HELD' : 'BROKE'}`,
      '',
    ].join('\n'),
  };
}
