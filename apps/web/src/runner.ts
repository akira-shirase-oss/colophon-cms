// The site runner (docs/modules/15-web-blog.md, "build → verify → atomically promote"; D-052): a
// webhook destination for Colophon's worker and the site's server in one process.
//
// - POST /hook: a release announcement. Answered 202 at once; the newest release announced is the
//   one wanted, and announcements of older ones change nothing.
// - GET anything else: the live site, from `root/current`, with 404.html for what isn't there.
//
// One build runs at a time and always builds the newest wanted release, so a burst of releases
// costs one build each at most, and promote.ts refuses anything older than what's live. A failed
// build leaves the live site as it was (B3) and is tried again on the next announcement or after
// a pause. Probes read each page's marker, so a receipt can say the site really shows a release.
//
// Settings come from the environment: SITE_API (Colophon's origin), SITE_PROJECT,
// SITE_ENVIRONMENT (default production), SITE_ROOT (default ./site), SITE_LISTEN (default
// 127.0.0.1:58099). Without SITE_PROJECT the runner adopts the project of the first announcement
// it gets and refuses every other (scripts/load.sh starts it before the project exists).

import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { readFile, stat } from 'node:fs/promises';
import { join, normalize, sep } from 'node:path';

import { build } from './build.ts';
import { live, promote } from './promote.ts';

/** The largest announcement read. */
const MAX_HOOK_BYTES = 64 * 1024;
/** How long after a failed build the runner tries again on its own. */
const RETRY_MS = 5_000;

const TYPES: Record<string, string> = {
	'.html': 'text/html; charset=utf-8',
	'.css': 'text/css; charset=utf-8',
	'.txt': 'text/plain; charset=utf-8'
};

export interface RunnerSettings {
	api: string;
	/** Empty until the first announcement names one, when not configured. */
	project: string;
	environment: string;
	root: string;
	/** For tests. */
	fetch?: typeof fetch;
}

export class Runner {
	readonly settings: RunnerSettings;
	#wanted = 0;
	#building = false;
	#retry: NodeJS.Timeout | null = null;

	constructor(settings: RunnerSettings) {
		this.settings = settings;
	}

	/** Notes that `generation` of `project` was published, and builds if nothing is building; false
	 *  if it's another project's. */
	announce(generation: number, project = this.settings.project): boolean {
		if (this.settings.project === '') this.settings.project = project;
		if (project !== this.settings.project) return false;
		if (generation > this.#wanted) this.#wanted = generation;
		void this.#drain();
		return true;
	}

	async #drain(): Promise<void> {
		if (this.#building) return;
		this.#building = true;
		try {
			while (this.#wanted > (await live(this.settings.root))) {
				const generation = this.#wanted;
				const built = await build({ ...this.settings, generation });
				const outcome = await promote(this.settings.root, built);
				console.log(JSON.stringify({ message: 'build', generation, ...outcome }));
				if (!outcome.promoted) break;
			}
		} catch (error) {
			console.log(JSON.stringify({ message: 'build failed; the live site is unchanged', error: String(error) }));
			this.#retry ??= setTimeout(() => {
				this.#retry = null;
				void this.#drain();
			}, RETRY_MS);
		} finally {
			this.#building = false;
		}
	}

	async handle(request: IncomingMessage, response: ServerResponse): Promise<void> {
		if (request.method === 'POST' && request.url === '/hook') return this.#hook(request, response);
		if (request.method !== 'GET' && request.method !== 'HEAD') {
			response.writeHead(405).end();
			return;
		}
		await this.#serve(request, response);
	}

	async #hook(request: IncomingMessage, response: ServerResponse): Promise<void> {
		let size = 0;
		const chunks: Buffer[] = [];
		for await (const chunk of request) {
			size += (chunk as Buffer).length;
			if (size > MAX_HOOK_BYTES) {
				response.writeHead(413).end();
				return;
			}
			chunks.push(chunk as Buffer);
		}
		let announced: { generation?: unknown; project?: unknown } = {};
		try {
			announced = JSON.parse(Buffer.concat(chunks).toString('utf8')) as typeof announced;
		} catch {
			announced = {};
		}
		const { generation, project } = announced;
		const named = typeof project === 'string' && project !== '' ? project : this.settings.project;
		if (typeof generation !== 'number' || !Number.isSafeInteger(generation) || generation < 1 || named === '') {
			response.writeHead(400).end();
			return;
		}
		response.writeHead(this.announce(generation, named) ? 202 : 400).end();
	}

	async #serve(request: IncomingMessage, response: ServerResponse): Promise<void> {
		const site = join(this.settings.root, 'current');
		const path = decodeURIComponent(new URL(request.url ?? '/', 'http://site').pathname);
		const file = normalize(join(site, path.endsWith('/') ? `${path}index.html` : path));
		const inside = file.startsWith(`${site}${sep}`);
		const found = inside && (await stat(file).catch(() => null))?.isFile() === true;
		const shown = found ? file : join(site, '404.html');
		const body = await readFile(shown).catch(() => null);
		if (body === null) {
			response.writeHead(503, { 'content-type': 'text/plain' }).end('nothing is live yet\n');
			return;
		}
		const type = TYPES[shown.slice(shown.lastIndexOf('.'))] ?? 'application/octet-stream';
		response.writeHead(found ? 200 : 404, { 'content-type': type, 'cache-control': 'no-cache' });
		response.end(request.method === 'HEAD' ? undefined : body);
	}
}

function setting(name: string, fallback?: string): string {
	const value = process.env[name] ?? fallback;
	if (value === undefined || value.trim() === '') throw new Error(`${name} is required`);
	return value;
}

if (import.meta.main) {
	const runner = new Runner({
		api: setting('SITE_API'),
		project: process.env['SITE_PROJECT']?.trim() ?? '',
		environment: setting('SITE_ENVIRONMENT', 'production'),
		root: setting('SITE_ROOT', 'site')
	});
	const [host, port] = setting('SITE_LISTEN', '127.0.0.1:58099').split(':');
	const server = createServer((request, response) => {
		runner.handle(request, response).catch((error: unknown) => {
			console.log(JSON.stringify({ message: 'request failed', error: String(error) }));
			if (!response.headersSent) response.writeHead(500).end();
		});
	});
	server.listen(Number(port), host, () => {
		console.log(JSON.stringify({ message: 'site runner listening', host, port: Number(port) }));
		// Whatever is published already: build the latest release on start.
		if (runner.settings.project === '') return;
		void build({ ...runner.settings, root: runner.settings.root })
			.then((built) => promote(runner.settings.root, built))
			.then((outcome) => console.log(JSON.stringify({ message: 'first build', ...outcome })))
			.catch((error: unknown) => console.log(JSON.stringify({ message: 'first build failed', error: String(error) })));
	});
	for (const signal of ['SIGINT', 'SIGTERM'] as const) {
		process.on(signal, () => server.close(() => process.exit(0)));
	}
}
