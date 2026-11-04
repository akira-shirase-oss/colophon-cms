// A webhook and CDN destination for the publishing load test (scripts/load.sh publishing): answers
// every POST (announcements to /hook, purges to /purge) with 204 and counts what it got, by
// idempotency key. GET /stats returns the counts, so
// the scenario can check that every announcement arrived and how many were repeats.
//
// It is also the fault driver for the dependency-failure scenarios (L11-L13, D-127): POST
// /control {"fault": NAME, "action": "down"|"up", ...} with a real outage behind each name, so the
// scenario's own conductor arm times the window — load.sh can't, it doesn't share k6's clock:
//   postgres / valkey   — `docker compose pause`/`unpause` the stack container (a real freeze:
//                          open connections hang, not refuse)
//   worker              — SIGKILL the worker process load.sh spawned (WORKER_PID_FILE holds its
//                          pid; "up" respawns it through WORKER_CMD and writes the new pid back)
//   purge               — POST /purge answers `status` (with `retry_after` seconds) instead of 204
//   site                — GET /site keeps reporting the generation it held when the fault went
//                          down: a site whose rebuild is late, for L13's delayed-rebuild half
// Sites are per-path: POST /hook/<key> advances only the site GET /site/<key> reports (the bare
// paths share key '', for publishing.js's single project) — a run over several projects needs
// per-project sites or one project's announcements would "rebuild" another's.
// Each action is stamped into stdout as `fault <name> <action> <iso>` so scripts/load.sh can scope
// the server's warning gate to the injected window. A paused container unpause itself after
// FAULT_MAX_SECONDS (default 600) even if the run dies first.
// Usage: node perf/receiver/receiver.mjs <port>

import { createServer } from 'node:http';
import { spawn } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';

const port = Number(process.argv[2] ?? '58096');
const composeFile = process.env.COMPOSE_FILE ?? 'deploy/compose.yaml';
const workerPidFile = process.env.WORKER_PID_FILE ?? '';
const workerCmd = process.env.WORKER_CMD ?? '';
const faultMaxMs = Number(process.env.FAULT_MAX_SECONDS ?? '600') * 1000;

let requests = 0;
const keys = new Set();
const paths = { hook: 0, purge: 0, site: 0, control: 0 };
const purgeAttempts = []; // {at, answered, key} for the newest 256, for per-job backoff spacing
const sites = new Map(); // site key -> {generation, frozen}; frozen null while live
let siteDown = false; // while 'site' is down every site reports the generation it had frozen at
let purgeDown = null; // {status, retry_after} while 'purge' is down
const containers = new Map(); // service name -> safety timer while paused

// One site's state, created frozen at nothing if the fault is already down (a site that
// never heard an announcement has nothing to report — a rebuild that is late either way).
const siteOf = (key) => {
	if (!sites.has(key)) sites.set(key, { generation: 0, frozen: siteDown ? 0 : null });
	return sites.get(key);
};

const iso = () => new Date().toISOString();
const logFault = (fault, action) => console.log(`fault ${fault} ${action} ${iso()}`);

const run = (command, args) =>
	new Promise((resolve, reject) => {
		const child = spawn(command, args, { stdio: ['ignore', 'pipe', 'pipe'] });
		let stderr = '';
		child.stderr.on('data', (chunk) => {
			stderr += chunk;
		});
		child.on('error', reject);
		child.on('close', (code) => {
			if (code === 0) resolve();
			else reject(new Error(`${command} ${args.join(' ')}: ${stderr.trim() || `exit ${code}`}`));
		});
	});

// pause/unpause keeps TCP alive but the process frozen — the honest shape of a dependency that
// is unreachable-but-not-refused. The safety timer means a crashed run can't leave it dead.
async function container(service, action) {
	if (action === 'down') {
		logFault(service, 'down');
		await run('docker', ['compose', '-f', composeFile, 'pause', service]);
		containers.set(
			service,
			setTimeout(() => {
				containers.delete(service);
				run('docker', ['compose', '-f', composeFile, 'unpause', service]).catch((error) =>
					console.log(`safety unpause of ${service} failed: ${error.message}`),
				);
			}, faultMaxMs),
		);
		return;
	}
	await run('docker', ['compose', '-f', composeFile, 'unpause', service]);
	const timer = containers.get(service);
	if (timer) {
		clearTimeout(timer);
		containers.delete(service);
	}
	logFault(service, 'up');
}

const readWorkerPid = () => {
	try {
		return Number(readFileSync(workerPidFile, 'utf8').trim()) || 0;
	} catch {
		return 0;
	}
};

async function worker(action) {
	if (action === 'down') {
		const pid = readWorkerPid();
		if (!pid) throw new Error('WORKER_PID_FILE names no live worker');
		logFault('worker', 'down');
		process.kill(pid, 'SIGKILL');
		return;
	}
	if (!workerCmd) throw new Error('WORKER_CMD is not set');
	const running = readWorkerPid();
	if (running) {
		try {
			process.kill(running, 0);
			return; // a live worker already holds the pid file
		} catch {
			/* dead — respawn */
		}
	}
	const child = spawn('bash', ['-c', workerCmd], { detached: true, stdio: 'ignore' });
	child.unref();
	if (workerPidFile) writeFileSync(workerPidFile, `${child.pid}\n`);
	logFault('worker', 'up');
}

function bodyJson(request) {
	return new Promise((resolve, reject) => {
		const chunks = [];
		request.on('data', (chunk) => chunks.push(chunk));
		request.on('end', () => {
			try {
				resolve(chunks.length ? JSON.parse(Buffer.concat(chunks).toString('utf8')) : {});
			} catch (error) {
				reject(error);
			}
		});
		request.on('error', reject);
	});
}

async function control(request, response) {
	const order = await bodyJson(request);
	const { fault, action } = order;
	if (!['down', 'up'].includes(action)) throw new Error(`bad action ${action}`);
	switch (fault) {
		case 'postgres':
		case 'valkey':
			await container(fault, action);
			break;
		case 'worker':
			await worker(action);
			break;
		case 'purge':
			if (action === 'down') {
				purgeDown = {
					status: Number(order.status) || 503,
					retry_after: Number(order.retry_after) || 0,
				};
			} else {
				purgeDown = null;
			}
			logFault('purge', action);
			break;
		case 'site':
			siteDown = action === 'down';
			for (const site of sites.values()) {
				site.frozen = siteDown ? site.generation : null;
			}
			logFault('site', action);
			break;
		default:
			throw new Error(`unknown fault ${fault}`);
	}
	response.writeHead(200, { 'content-type': 'application/json' });
	response.end(JSON.stringify({ ok: true, fault, action }));
}

createServer((request, response) => {
	if (request.method === 'GET' && request.url === '/stats') {
		response.writeHead(200, { 'content-type': 'application/json' });
		response.end(
			JSON.stringify({
				requests,
				distinct_keys: keys.size,
				paths,
				generation: (() => { const s = sites.get(''); return s ? (s.frozen ?? s.generation) : 0; })(),
				generations: Object.fromEntries(
					[...sites].map(([key, s]) => [key, s.frozen ?? s.generation]),
				),
				purge_attempts: purgeAttempts,
			}),
		);
		return;
	}
	const siteKey = request.method === 'GET' && /^\/site(?:\/([^/?]+))?\/?$/.exec(request.url);
	if (siteKey) {
		// The rebuilt site a probe reads: the generation the site would have been built from.
		paths.site += 1;
		const site = siteOf(siteKey[1] ?? '');
		const generation = site.frozen ?? site.generation;
		response.writeHead(200, {
			'content-type': 'text/html',
			'colophon-generation': String(generation),
		});
		response.end(
			`<!doctype html><html><head><meta name="colophon-generation" content="${generation}"></head><body>release ${generation}</body></html>`,
		);
		return;
	}
	if (request.method === 'POST' && request.url === '/control') {
		paths.control += 1;
		control(request, response).catch((error) => {
			console.log(`control refused: ${error.message}`);
			response.writeHead(500, { 'content-type': 'application/json' });
			response.end(JSON.stringify({ ok: false, error: error.message }));
		});
		return;
	}
	bodyJson(request)
		.then((body) => {
			requests += 1;
			keys.add(String(request.headers['idempotency-key'] ?? ''));
			if (request.url === '/purge') {
				paths.purge += 1;
				const key = String(request.headers['idempotency-key'] ?? '');
				purgeAttempts.push({ at: Date.now(), answered: purgeDown ? purgeDown.status : 204, key });
				if (purgeAttempts.length > 256) purgeAttempts.shift();
				if (purgeDown) {
					const headers = { 'content-type': 'application/json' };
					if (purgeDown.retry_after) headers['retry-after'] = String(purgeDown.retry_after);
					response.writeHead(purgeDown.status, headers).end(JSON.stringify({ error: 'purge unavailable' }));
					return;
				}
				response.writeHead(204).end();
				return;
			}
			const hookKey = /^\/hook(?:\/([^/?]+))?\/?$/.exec(request.url);
			if (hookKey) {
				paths.hook += 1;
				const site = siteOf(hookKey[1] ?? '');
				if (typeof body.generation === 'number' && body.generation > site.generation) {
					site.generation = body.generation;
				}
			}
			response.writeHead(204).end();
		})
		.catch(() => {
			response.writeHead(400).end();
		});
}).listen(port, '127.0.0.1', () => console.log(`receiver listening on 127.0.0.1:${port}`));
