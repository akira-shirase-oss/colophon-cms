#!/usr/bin/env node
// The SDK's load evidence (D-018, D-170). The SDK serves no traffic, so its unit is the SDK call:
// TOTAL delivery reads (`list` and `get`, alternating) through the *packed* @colophon-cms/sdk,
// against the same server once more with bare `fetch` — the same URLs, headers and JSON parse —
// so the difference is what the client itself costs. It records the client's p50/p99 overhead
// (budget: p99 at most 1 ms over bare fetch) and the heap growth across the run after a forced
// collection (budget: 20 MB — no leak from the per-call AbortSignal and timeout).
//
//   node --expose-gc perf/node/sdk-delivery.mjs                    # a stub server, its own process
//   ORIGIN=http://127.0.0.1:58000 PROJECT=p COLLECTION=articles ENTRY=<id> \
//     node --expose-gc perf/node/sdk-delivery.mjs                  # a real server's published content
//
// TOTAL (default 1,000,000 — the testing rule's full run; a smoke passes TOTAL=20000), CONCURRENCY
// (default 8), PORT (the stub's; default any free one), KEEP=0 (don't write a result file), and
// SDK_TARBALL (a packed SDK; by default this script packs packages/sdk itself). The result goes in
// perf/results/ like the k6 runs, naming the machine and commit; laptop numbers are evidence about
// the code on that machine, never production capacity.
import { execFileSync, spawn } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { cpus, tmpdir, totalmem, release, type } from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const TOTAL = Number(process.env.TOTAL ?? 1_000_000);
const CONCURRENCY = Number(process.env.CONCURRENCY ?? 8);
// The budgets (module 13). A smoke on a busy laptop may loosen the overhead one through
// OVERHEAD_P99_MS; the result file records the budget the run was held to.
const OVERHEAD_P99_BUDGET_MS = Number(process.env.OVERHEAD_P99_MS ?? 1);
const HEAP_BUDGET_MB = 20;
if (!Number.isInteger(TOTAL) || TOTAL < 2 || !Number.isInteger(CONCURRENCY) || CONCURRENCY < 1) {
	console.error('TOTAL must be an integer of at least 2 and CONCURRENCY at least 1');
	process.exit(2);
}
if (typeof globalThis.gc !== 'function') {
	console.error('run with node --expose-gc, so the heap is measured after a collection');
	process.exit(2);
}

/* ---------------------------------------------------- the packed SDK, installed */

const scratch = mkdtempSync(path.join(tmpdir(), 'colophon-sdk-load-'));
let tarball = process.env.SDK_TARBALL;
if (tarball === undefined) {
	const [packed] = JSON.parse(
		execFileSync('npm', ['pack', '--json', '--pack-destination', scratch], { cwd: path.join(repository, 'packages/sdk'), encoding: 'utf8', stdio: ['ignore', 'pipe', 'inherit'] })
	);
	tarball = path.join(scratch, packed.filename);
}
const consumer = path.join(scratch, 'consumer');
mkdirSync(consumer);
writeFileSync(path.join(consumer, 'package.json'), '{"name":"load","private":true,"type":"module"}');
execFileSync('npm', ['install', '--no-audit', '--no-fund', '--ignore-scripts', '--no-package-lock', path.resolve(tarball)], { cwd: consumer, stdio: 'ignore' });
const sdk = await import(pathToFileURL(path.join(consumer, 'node_modules/@colophon-cms/sdk/dist/index.js')).href);

/* ------------------------------------------------------------- the server */

let origin = process.env.ORIGIN;
let project = process.env.PROJECT ?? 'site';
let collection = process.env.COLLECTION ?? 'articles';
let entry = process.env.ENTRY ?? '0190a4b0-0000-7000-8000-0000000000aa';
let stub = null;
if (origin === undefined) {
	// A stub in its own process, so it never shares this event loop with the client being
	// measured. Answers shaped like /content/v1's, prepared once: its cost is the same for both arms.
	const one = { id: entry, collection, slug: 'hello', title: 'Hello', published_in: 7, content: { title: 'Hello', body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Hello.' }] }] } } } };
	const answers = {
		[`/content/v1/${project}/production/${collection}`]: JSON.stringify({ data: Array.from({ length: 10 }, (_, i) => ({ ...one, slug: `post-${i}` })), next_cursor: null, generation: 7 }),
		[`/content/v1/${project}/production/${collection}/${entry}`]: JSON.stringify({ data: one, generation: 7 })
	};
	const program = `
		const answers = ${JSON.stringify(answers)};
		const server = require('node:http').createServer((request, response) => {
			const body = answers[new URL(request.url, 'http://stub').pathname];
			response.writeHead(body === undefined ? 404 : 200, { 'content-type': 'application/json', etag: '"g7"' });
			response.end(body ?? '{"error":{"code":"route_not_found","message":"no route","request_id":"stub"}}');
		});
		server.keepAliveTimeout = 60000;
		server.listen(${Number(process.env.PORT ?? 0)}, '127.0.0.1', () => process.stdout.write(String(server.address().port) + '\\n'));
	`;
	stub = spawn(process.execPath, ['-e', program], { stdio: ['ignore', 'pipe', 'inherit'] });
	const port = await new Promise((resolve, reject) => {
		stub.stdout.once('data', (chunk) => resolve(Number(String(chunk).trim())));
		stub.once('exit', (code) => reject(new Error(`the stub server exited (${code})`)));
	});
	origin = `http://127.0.0.1:${port}`;
}

const client = new sdk.Client({ baseUrl: origin, compat: 'off' });
const site = client.delivery(project);
const base = `${origin}/content/v1/${encodeURIComponent(project)}/production/${encodeURIComponent(collection)}`;

/** The same request bare: URL, headers, a deadline and the JSON parse — everything but the SDK. */
async function bare(i) {
	const url = i % 2 === 0 ? `${base}?limit=10` : `${base}/${encodeURIComponent(entry)}`;
	const response = await fetch(url, { headers: { accept: 'application/json' }, signal: AbortSignal.timeout(15_000), cache: 'no-store' });
	if (!response.ok) throw new Error(`bare fetch answered ${response.status}`);
	return JSON.parse(await response.text());
}
function viaSdk(i) {
	return i % 2 === 0 ? site.list(collection, { limit: 10 }) : site.get(collection, entry);
}

/** TOTAL calls of `call`, CONCURRENCY at a time; each call's wall time in microseconds. */
async function arm(name, call, total) {
	const times = new Float64Array(total);
	let next = 0;
	let failed = 0;
	const started = performance.now();
	await Promise.all(
		Array.from({ length: CONCURRENCY }, async () => {
			for (let i = next++; i < total; i = next++) {
				const t0 = performance.now();
				try {
					await call(i);
				} catch {
					failed++;
				}
				times[i] = (performance.now() - t0) * 1000;
			}
		})
	);
	const seconds = (performance.now() - started) / 1000;
	times.sort();
	const at = (q) => times[Math.min(total - 1, Math.floor(q * total))] / 1000;
	const result = { calls: total, failed, rps: Math.round(total / seconds), latency_ms: { p50: at(0.5), p90: at(0.9), p99: at(0.99), max: times[total - 1] / 1000 } };
	console.log(`${name}: ${total} calls, ${failed} failed, ${result.rps}/s, p50 ${result.latency_ms.p50.toFixed(3)} ms, p99 ${result.latency_ms.p99.toFixed(3)} ms`);
	return result;
}

// Warm both paths (connections, JIT), then measure: bare first, then the SDK, then bare again so
// drift over the run shows up in the comparison; the heap is measured before the SDK's calls and
// after everything settled.
await arm('warm-up (bare)', bare, Math.min(2000, TOTAL));
await arm('warm-up (sdk)', viaSdk, Math.min(2000, TOTAL));
const half = Math.floor(TOTAL / 2);
const bareBefore = await arm('bare fetch', bare, Math.floor(half / 2));
// Every call's deadline is an AbortSignal.timeout(15 s) timer, alive until it fires whether or not
// the call finished (bare fetch holds the same): measure the heap only once the earlier calls'
// deadlines fired, before and after, so what it shows is growth.
const settle = () => new Promise((resolve) => setTimeout(resolve, 16_000));
await settle();
globalThis.gc();
const heapBefore = process.memoryUsage().heapUsed;
const viaSdkResult = await arm('sdk', viaSdk, TOTAL - half);
const bareAfter = await arm('bare fetch', bare, half - Math.floor(half / 2));
await settle();
globalThis.gc();
const heapAfter = process.memoryUsage().heapUsed;
stub?.kill();
rmSync(scratch, { recursive: true, force: true });

const bareP50 = (bareBefore.latency_ms.p50 + bareAfter.latency_ms.p50) / 2;
const bareP99 = (bareBefore.latency_ms.p99 + bareAfter.latency_ms.p99) / 2;
const overhead = { p50_ms: viaSdkResult.latency_ms.p50 - bareP50, p99_ms: viaSdkResult.latency_ms.p99 - bareP99 };
const heapGrowthMb = (heapAfter - heapBefore) / 1024 / 1024;
const failed = bareBefore.failed + viaSdkResult.failed + bareAfter.failed;
console.log(`overhead: p50 ${overhead.p50_ms.toFixed(3)} ms, p99 ${overhead.p99_ms.toFixed(3)} ms (budget ${OVERHEAD_P99_BUDGET_MS} ms); heap growth ${heapGrowthMb.toFixed(2)} MB (budget ${HEAP_BUDGET_MB} MB)`);

/* ---------------------------------------------------------------- the record */

const git = (...args) => {
	try {
		return execFileSync('git', args, { cwd: repository, encoding: 'utf8' }).trim();
	} catch {
		return 'unknown';
	}
};
const cpu = cpus()[0]?.model.trim() ?? 'unknown';
const cores = cpus().length;
const machine = `${cpu}-${cores}c`.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/-$/, '');
const date = new Date().toISOString().replace(/\.\d+Z$/, 'Z');
const record = {
	scenario: 'throughput',
	endpoint: 'sdk-delivery',
	date,
	commit: git('rev-parse', '--short', 'HEAD'),
	dirty: git('status', '--porcelain', '--untracked-files=no') !== '',
	machine: { id: machine, cpu, cores, memory_gb: Math.round(totalmem() / 1024 ** 3), os: `${type()} ${release()}` },
	node: process.version,
	target: stub === null ? 'server' : 'stub',
	params: { total: TOTAL, concurrency: CONCURRENCY },
	requests: TOTAL,
	failed,
	sdk: viaSdkResult,
	bare: { before: bareBefore, after: bareAfter },
	overhead_ms: overhead,
	heap_growth_mb: Number(heapGrowthMb.toFixed(2)),
	budgets: { overhead_p99_ms: OVERHEAD_P99_BUDGET_MS, heap_growth_mb: HEAP_BUDGET_MB }
};
if (process.env.KEEP !== '0') {
	const file = path.join(repository, 'perf/results', `${date.replaceAll(':', '')}-sdk-delivery-throughput-${machine}.json`);
	writeFileSync(file, `${JSON.stringify(record, null, 2)}\n`);
	console.log(`wrote ${path.relative(repository, file)}`);
}
const misses = [];
if (failed > 0) misses.push(`${failed} calls failed`);
if (overhead.p99_ms > OVERHEAD_P99_BUDGET_MS) misses.push(`p99 overhead ${overhead.p99_ms.toFixed(3)} ms over ${OVERHEAD_P99_BUDGET_MS} ms`);
if (heapGrowthMb > HEAP_BUDGET_MB) misses.push(`heap grew ${heapGrowthMb.toFixed(2)} MB, over ${HEAP_BUDGET_MB} MB`);
if (misses.length > 0) {
	console.error(`sdk-delivery: ${misses.join('; ')}`);
	process.exit(1);
}
