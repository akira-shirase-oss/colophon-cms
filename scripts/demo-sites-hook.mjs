#!/usr/bin/env node
// The five demo sites' build hook (apps/sites/README.md; D-138): a website connection — the
// project's `webhook` destination — and the rebuild it triggers, in one process.
//
// - POST /hook: a release announcement from Colophon's worker (`release.committed`). Answered
//   202 at once; the newest generation announced is the one wanted, and announcements of older
//   ones change nothing. A duplicate (the worker retries) is harmless: it names a generation
//   already built, so no second rebuild runs.
// - GET /healthz: what the hook has done — the generation it has built, whether a build is
//   running, how many rebuilds it has completed, and the last failure if any.
//
// One rebuild runs at a time and always builds the newest wanted release: `scripts/
// serve-demo-sites.sh build` then `serve` (which stops the old servers first). A failed build
// leaves the running sites as they were and is retried after a pause. Publishing in the Studio
// therefore updates all five sites without a person running anything.
//
// On start, if COLOPHON_ADMIN_USER is set, the hook signs in and makes sure the project has a
// webhook destination pointing at itself (`POST /api/v1/projects/{P}/environments/production/
// destinations`), so a fresh install only needs this one command. Settings come from the
// environment: HOOK_PORT (default 58120), COLOPHON_SERVER (default http://localhost:58000),
// COLOPHON_ADMIN_USER / COLOPHON_ADMIN_PASSWORD (default the dev stack's dev-owner), and
// HOOK_BUILD_ON_START=0 to skip the first build.

import { execFile } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = dirname(dirname(fileURLToPath(import.meta.url)));
const RECORD = join(ROOT, 'apps/sites/.colophon.json');
const PORT = Number(process.env['HOOK_PORT'] ?? 58120);
const SERVER = process.env['COLOPHON_SERVER'] ?? 'http://localhost:58000';
const USER = process.env['COLOPHON_ADMIN_USER'] ?? 'dev-owner';
const PASSWORD = process.env['COLOPHON_ADMIN_PASSWORD'] ?? 'colophon-dev';
const BUILD_ON_START = process.env['HOOK_BUILD_ON_START'] !== '0';

/** The largest announcement read. */
const MAX_HOOK_BYTES = 64 * 1024;
/** How long after a failed build the hook tries again on its own. */
const RETRY_MS = 5_000;

const state = {
	building: false,
	wanted: 0,
	built: 0,
	rebuilds: 0,
	lastError: null,
	lastBuildAt: null
};

async function record() {
	return JSON.parse(await readFile(RECORD, 'utf8'));
}

function run(script, args) {
	return new Promise((resolve, reject) => {
		execFile('bash', [join(ROOT, 'scripts', script), ...args], { cwd: ROOT, maxBuffer: 16 * 1024 * 1024 }, (error, stdout, stderr) => {
			if (error) reject(new Error(`${script} ${args.join(' ')} failed\n${stderr || stdout || String(error)}`));
			else resolve(stdout);
		});
	});
}

async function drain() {
	if (state.building) return;
	state.building = true;
	try {
		while (state.wanted > state.built) {
			const before = state.built;
			await run('serve-demo-sites.sh', ['build']);
			await ensureServing();
			// What was built is what the delivery API now serves — never the announced number,
			// which a duplicate or a mistaken announcement could overstate.
			state.built = await liveGeneration().catch(() => state.wanted);
			state.rebuilds += 1;
			state.lastBuildAt = new Date().toISOString();
			state.lastError = null;
			console.log(JSON.stringify({ message: 'rebuilt the five demo sites', generation: state.built, rebuilds: state.rebuilds }));
			// A build that did not advance the live generation means there is nothing newer to
			// build; stop rather than rebuild in a loop on a bogus announcement.
			if (state.built <= before) break;
		}
	} catch (error) {
		state.lastError = String(error);
		console.log(JSON.stringify({ message: 'rebuild failed; the running sites are unchanged', error: state.lastError }));
		setTimeout(() => {
			if (state.wanted > state.built) void drain();
		}, RETRY_MS);
	} finally {
		state.building = false;
	}
}

/** The release the delivery API is serving right now, from its own header. */
async function liveGeneration() {
	const { project, environment, collections } = await record();
	const collection = collections?.[0] ?? 'posts';
	const response = await fetch(`${SERVER}/content/v1/${project}/${environment}/${collection}`, { method: 'HEAD' });
	const header = Number(response.headers.get('colophon-generation'));
	if (!Number.isSafeInteger(header) || header < 1) throw new Error('no colophon-generation header');
	return header;
}

/** `build` promotes beside the live directories; only a site that stays down needs `serve`. */
async function ensureServing() {
	const ports = [58110, 58111, 58112, 58113, 58114];
	const check = () =>
		Promise.all(
			ports.map(async (port) => {
				try {
					const response = await fetch(`http://127.0.0.1:${port}/`, { signal: AbortSignal.timeout(2000) });
					return response.ok;
				} catch {
					return false;
				}
			})
		);
	// The blog restarts as part of promotion, so a port can be down for a moment: retry before
	// taking the whole set down for `serve`, which is only for sites that never came back.
	for (let attempt = 0; attempt < 5; attempt += 1) {
		if ((await check()).every(Boolean)) return;
		await new Promise((resolve) => setTimeout(resolve, 1000));
	}
	await run('serve-demo-sites.sh', ['serve']);
}

function note(generation, project) {
	if (generation > state.wanted) state.wanted = generation;
	void drain();
	return true;
}

// --- the website connection, made by the hook itself -------------------------

/** Signs in the way the seed does: the server's /auth/login redirects into Keycloak's form. */
async function signIn() {
	const cookies = new Map();
	const remember = (response) => {
		for (const header of response.headers.getSetCookie()) {
			const [pair] = header.split(';');
			const index = pair.indexOf('=');
			if (index > 0) cookies.set(pair.slice(0, index).trim(), pair.slice(index + 1));
		}
	};
	const header = () => [...cookies].map(([name, value]) => `${name}=${value}`).join('; ');
	const visit = async (url, init = {}) => {
		let next = url;
		let options = init;
		for (let hop = 0; hop < 10; hop += 1) {
			const response = await fetch(next, {
				...options,
				redirect: 'manual',
				headers: { ...(options.headers ?? {}), cookie: header() }
			});
			remember(response);
			const location = response.headers.get('location');
			if (response.status < 300 || response.status >= 400 || location === null) return response;
			next = new URL(location, next).toString();
			options = {};
		}
		throw new Error('too many redirects signing in');
	};

	const form = await visit(`${SERVER}/auth/login?return_to=/me`);
	const action = /<form[^>]*id="kc-form-login"[^>]*action="([^"]+)"/.exec(await form.text())?.[1];
	if (action === undefined) throw new Error('no Keycloak sign-in form at /auth/login');
	const signedIn = await visit(action.replaceAll('&amp;', '&'), {
		method: 'POST',
		headers: { 'content-type': 'application/x-www-form-urlencoded' },
		body: new URLSearchParams({ username: USER, password: PASSWORD })
	});
	if (signedIn.status !== 200) throw new Error(`sign-in failed: HTTP ${signedIn.status}`);

	return async (method, path, body) => {
		const response = await fetch(`${SERVER}${path}`, {
			method,
			headers: {
				cookie: header(),
				origin: SERVER,
				'x-csrf-token': cookies.get('colophon_csrf') ?? '',
				...(body === undefined ? {} : { 'content-type': 'application/json' })
			},
			body: body === undefined ? undefined : JSON.stringify(body)
		});
		const text = await response.text();
		if (!response.ok) throw new Error(`${method} ${path}: HTTP ${response.status} ${text.slice(0, 200)}`);
		return text === '' ? null : JSON.parse(text);
	};
}

async function connect() {
	if (USER === '') {
		console.log(JSON.stringify({ message: 'no COLOPHON_ADMIN_USER; not registering the connection' }));
		return;
	}
	try {
		const { project } = await record();
		const api = await signIn();
		const base = `/api/v1/projects/${project}/environments/production/destinations`;
		const url = `http://127.0.0.1:${PORT}/hook`;
		const existing = await api('GET', base);
		if (existing.data.some((destination) => destination.url === url)) {
			console.log(JSON.stringify({ message: 'website connection already present', url }));
			return;
		}
		await api('POST', base, { kind: 'webhook', name: 'Five demo sites', url });
		console.log(JSON.stringify({ message: 'website connection added', url }));
	} catch (error) {
		console.log(JSON.stringify({ message: 'could not register the website connection', error: String(error) }));
	}
}

// --- the server -------------------------------------------------------------

const server = createServer((request, response) => {
	handle(request, response).catch((error) => {
		console.log(JSON.stringify({ message: 'request failed', error: String(error) }));
		if (!response.headersSent) response.writeHead(500).end();
	});
});

async function handle(request, response) {
	if (request.method === 'POST' && request.url === '/hook') {
		let size = 0;
		const chunks = [];
		for await (const chunk of request) {
			size += chunk.length;
			if (size > MAX_HOOK_BYTES) {
				response.writeHead(413).end();
				return;
			}
			chunks.push(chunk);
		}
		let announced = {};
		try {
			announced = JSON.parse(Buffer.concat(chunks).toString('utf8'));
		} catch {
			announced = {};
		}
		const { event, generation, project } = announced;
		const wanted = await record().catch(() => null);
		if (wanted === null) {
			response.writeHead(503, { 'content-type': 'application/json' }).end(JSON.stringify({ error: 'no apps/sites/.colophon.json; seed the sites first' }));
			return;
		}
		if (event !== 'release.committed' || !Number.isSafeInteger(generation) || generation < 1 || project !== wanted.project) {
			response.writeHead(400, { 'content-type': 'application/json' }).end(JSON.stringify({ error: 'not this project\'s release announcement' }));
			return;
		}
		note(generation, project);
		response.writeHead(202, { 'content-type': 'application/json' }).end(JSON.stringify({ queued: generation, built: state.built }));
		return;
	}
	if (request.method === 'GET' && (request.url === '/healthz' || request.url === '/')) {
		response.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(state));
		return;
	}
	response.writeHead(404).end();
}

server.listen(PORT, '127.0.0.1', () => {
	console.log(JSON.stringify({ message: 'demo sites hook listening', url: `http://127.0.0.1:${PORT}/hook` }));
	void connect();
	if (BUILD_ON_START) {
		record()
			.then(({ generation }) => {
				state.built = 0;
				state.wanted = Math.max(state.wanted, generation ?? 1);
				void drain();
			})
			.catch(() => undefined);
	}
});

for (const signal of ['SIGINT', 'SIGTERM']) {
	process.on(signal, () => server.close(() => process.exit(0)));
}
