// Load scenario `uploads-slow` (D-215): slow uploads against the upload memory budget. k6 can't pace
// a request body, so this one is plain Node over raw sockets.
//
// It signs in SESSIONS sessions of dev-owner, makes a project, and opens SLOW uploads that each
// declare the body limit (21 MiB) and then send one byte every PACE_MS — never idle long enough to
// be given up (15 s), so each holds its share of the budget for the whole run. Uploads beyond the
// budget (12 at 21 MiB in the default 256 MiB) are refused `503 uploads_busy`, a fifth from one
// session `429 too_many_uploads`. Meanwhile a small upload from a fresh session must still be
// stored (202) quickly: the budget bounds memory, not the server's ability to take small work.
//
//   BASE_URL=http://localhost:58164 node perf/uploads-slow.mjs
//
// Env: BASE_URL (default http://localhost:58000), SLOW (default 16), SESSIONS (default 4),
// PACE_MS (default 5000), HOLD_SECONDS (default 20), BODY_LIMIT (default 21 MiB), FAST_P99_MS
// (default 2000). Prints one JSON line; exits 1 if the fast upload wasn't stored in time or any
// slow upload got an answer other than 503/429 while held.
import net from 'node:net';
import { deflateSync } from 'node:zlib';

const BASE = new URL(process.env.BASE_URL ?? 'http://localhost:58000');
const SLOW = Number(process.env.SLOW ?? 16);
const SESSIONS = Number(process.env.SESSIONS ?? 4);
const PACE_MS = Number(process.env.PACE_MS ?? 5000);
const HOLD_SECONDS = Number(process.env.HOLD_SECONDS ?? 20);
const BODY_LIMIT = Number(process.env.BODY_LIMIT ?? 21 * 1024 * 1024);
const FAST_P99_MS = Number(process.env.FAST_P99_MS ?? 2000);

async function signIn() {
  const response = await fetch(new URL('/auth/signin', BASE), {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Origin: BASE.origin },
    body: JSON.stringify({ email: 'dev-owner@colophon.test', password: 'colophon-dev' }),
    redirect: 'manual'
  });
  if (response.status !== 204) throw new Error(`sign-in: ${response.status}`);
  const cookies = response.headers.getSetCookie().map((line) => line.split(';')[0]);
  const csrf = cookies.find((pair) => pair.startsWith('colophon_csrf='))?.split('=')[1] ?? '';
  return { cookie: cookies.join('; '), csrf };
}

async function api(session, method, path, body) {
  const response = await fetch(new URL(path, BASE), {
    method,
    headers: { Cookie: session.cookie, 'X-CSRF-Token': session.csrf, Origin: BASE.origin, 'Content-Type': 'application/json', 'Idempotency-Key': crypto.randomUUID().replaceAll('-', '') },
    body: body === undefined ? undefined : JSON.stringify(body)
  });
  return { status: response.status, body: await response.json().catch(() => null) };
}

/** A real 2×2 PNG: what the fast upload sends. */
function png() {
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc = (bytes) => {
    let c = 0xffffffff;
    for (const byte of bytes) c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
  const chunk = (type, data) => {
    const length = Buffer.alloc(4);
    length.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
    const sum = Buffer.alloc(4);
    sum.writeUInt32BE(crc(body));
    return Buffer.concat([length, body, sum]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(2, 0);
  header.writeUInt32BE(2, 4);
  header[8] = 8;
  header[9] = 2;
  const row = Buffer.from([0, 10, 20, 30, 40, 50, 60]);
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', header), chunk('IDAT', deflateSync(Buffer.concat([row, row]))), chunk('IEND', Buffer.alloc(0))]);
}

/** An upload on a raw socket declaring `length` bytes: resolves with its status once answered. */
function openUpload(session, path, length) {
  const socket = net.connect({ host: BASE.hostname, port: Number(BASE.port || 80) });
  let text = '';
  const answered = new Promise((resolve) => {
    socket.on('data', (data) => (text += data.toString('latin1')));
    socket.on('close', () => resolve(text.split(' ')[1] ?? 'closed'));
    socket.on('error', () => resolve('error'));
  });
  socket.write(
    `POST ${path} HTTP/1.1\r\nHost: ${BASE.host}\r\nOrigin: ${BASE.origin}\r\nCookie: ${session.cookie}\r\nX-CSRF-Token: ${session.csrf}\r\n` +
      `Idempotency-Key: ${crypto.randomUUID().replaceAll('-', '')}\r\nX-Colophon-File-Name: slow.png\r\nContent-Type: image/png\r\n` +
      `Content-Length: ${length}\r\nConnection: close\r\n\r\n`
  );
  return { socket, answered };
}

const owner = await signIn();
const project = (await api(owner, 'POST', '/api/v1/projects', { slug: `slow-${Date.now().toString(36)}`, name: 'Slow uploads' })).body.data.id;
const path = `/api/v1/projects/${project}/environments/production/assets`;
const sessions = [owner];
while (sessions.length < SESSIONS) sessions.push(await signIn());

const slow = [];
for (let n = 0; n < SLOW; n += 1) slow.push(openUpload(sessions[n % SESSIONS], path, BODY_LIMIT));
const early = new Map();
for (const [index, upload] of slow.entries()) upload.answered.then((status) => early.set(index, status));
const pacer = setInterval(() => {
  for (const [index, upload] of slow.entries()) if (!early.has(index)) upload.socket.write(Buffer.from([0x89]));
}, PACE_MS);

await new Promise((resolve) => setTimeout(resolve, 3000));
// A small upload from a fresh session while the budget is held.
const fresh = await signIn();
const started = performance.now();
const fast = await fetch(new URL(path, BASE), {
  method: 'POST',
  headers: { Cookie: fresh.cookie, 'X-CSRF-Token': fresh.csrf, Origin: BASE.origin, 'Content-Type': 'image/png', 'X-Colophon-File-Name': 'fast.png', 'Idempotency-Key': crypto.randomUUID().replaceAll('-', '') },
  body: png()
});
const fastMs = Math.round(performance.now() - started);

await new Promise((resolve) => setTimeout(resolve, HOLD_SECONDS * 1000));
clearInterval(pacer);
const held = SLOW - early.size;
const refused = {};
for (const status of early.values()) refused[status] = (refused[status] ?? 0) + 1;
for (const upload of slow) upload.socket.destroy();

const unexpected = Object.keys(refused).filter((status) => status !== '503' && status !== '429');
const result = { scenario: 'uploads-slow', slow: SLOW, sessions: SESSIONS, held, refused, fast: { status: fast.status, ms: fastMs }, passed: fast.status === 202 && fastMs <= FAST_P99_MS && unexpected.length === 0 };
console.log(JSON.stringify(result));
process.exit(result.passed ? 0 : 1);
