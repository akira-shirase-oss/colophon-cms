// A caching HTTP proxy that plays a CDN in front of the testbed's site, for the preview spec. It
// is deliberately the worst common configuration — "cache everything": its key is the method
// and URL only (no cookies, no `Vary`), and an answer without freshness gets a default TTL — but
// it honours the rules every shared cache keeps: nothing marked `no-store`, `private` or
// `no-cache`, nothing with `Set-Cookie`, nothing answering `Authorization` unless `public`.
// With `bypassCookie` set it also applies the rule docs/guides/preview.md asks a CDN for: a
// request carrying the preview cookie goes straight to the origin.

import { createServer, request as forward, type IncomingHttpHeaders, type IncomingMessage, type Server, type ServerResponse } from 'node:http';

export type Outcome = 'HIT' | 'MISS' | 'BYPASS' | 'PASS';

export interface Logged {
	method: string;
	path: string;
	status: number;
	outcome: Outcome;
	/** Whether the answer was stored; `reason` says why not. */
	stored: boolean;
	reason: string;
}

interface Copy {
	status: number;
	headers: IncomingHttpHeaders;
	body: Buffer;
	expires: number;
}

/** Statuses a shared cache may store without explicit freshness (RFC 9111 §4.2.2). */
const HEURISTIC = new Set([200, 203, 204, 206, 300, 301, 308, 404, 405, 410, 414, 501]);
const HOP_BY_HOP = ['connection', 'keep-alive', 'proxy-authenticate', 'proxy-authorization', 'te', 'trailer', 'transfer-encoding', 'upgrade'];
const DEFAULT_TTL_MS = 60_000;

function directives(headers: IncomingHttpHeaders): Map<string, string | true> {
	const found = new Map<string, string | true>();
	for (const part of String(headers['cache-control'] ?? '').split(',')) {
		const [name, value] = part.trim().toLowerCase().split('=', 2);
		if (name) found.set(name, value ?? true);
	}
	return found;
}

/** Whether a shared cache may keep this answer, for how long, and if not, why. */
export function storable(method: string, request: IncomingHttpHeaders, status: number, response: IncomingHttpHeaders): { ttlMs: number; reason: string } {
	if (method !== 'GET') return { ttlMs: 0, reason: `method ${method}` };
	// What the answer itself says comes first, so the log names the header that kept it out.
	const cc = directives(response);
	for (const refusal of ['no-store', 'private', 'no-cache']) {
		if (cc.has(refusal)) return { ttlMs: 0, reason: refusal };
	}
	if (response['set-cookie'] !== undefined) return { ttlMs: 0, reason: 'set-cookie' };
	if (request['authorization'] !== undefined && !cc.has('public') && !cc.has('s-maxage')) return { ttlMs: 0, reason: 'authorization' };
	if (!HEURISTIC.has(status)) return { ttlMs: 0, reason: `status ${status}` };
	const lifetime = cc.get('s-maxage') ?? cc.get('max-age');
	const ttlMs = typeof lifetime === 'string' ? Number(lifetime) * 1000 : DEFAULT_TTL_MS;
	return Number.isFinite(ttlMs) && ttlMs > 0 ? { ttlMs, reason: 'stored' } : { ttlMs: 0, reason: 'max-age=0' };
}

function hasCookie(headers: IncomingHttpHeaders, name: string): boolean {
	return String(headers['cookie'] ?? '')
		.split(';')
		.some((pair) => pair.trim().startsWith(`${name}=`));
}

function strip(headers: IncomingHttpHeaders): IncomingHttpHeaders {
	const kept: IncomingHttpHeaders = { ...headers };
	for (const name of [...HOP_BY_HOP, 'content-length']) delete kept[name];
	return kept;
}

export class FakeCdn {
	readonly log: Logged[] = [];
	readonly #copies = new Map<string, Copy>();
	readonly #server: Server;
	readonly #upstream: { host: string; port: number };
	/** Where to reach the proxy, such as `http://localhost:58089`. */
	readonly origin: string;
	/** The preview cookie the guide's CDN rule bypasses on; `null` turns the rule off. */
	bypassCookie: string | null = null;

	private constructor(origin: string, upstream: { host: string; port: number }) {
		this.origin = origin;
		this.#upstream = upstream;
		this.#server = createServer((request, response) => {
			this.#handle(request, response).catch((cause: unknown) => {
				this.#record(request.method ?? 'GET', request.url ?? '/', 502, 'PASS', false, `upstream: ${String(cause)}`);
				if (!response.headersSent) response.writeHead(502, { 'content-type': 'text/plain' });
				response.end('fake CDN: the origin did not answer');
			});
		});
	}

	/** Starts the proxy on `127.0.0.1:port`, in front of `upstream` (an http origin). */
	static async start(port: number, upstream: string): Promise<FakeCdn> {
		const target = new URL(upstream);
		const cdn = new FakeCdn(`http://localhost:${port}`, { host: target.hostname, port: Number(target.port) });
		await new Promise<void>((done, fail) => {
			cdn.#server.once('error', fail);
			cdn.#server.listen(port, '127.0.0.1', () => done());
		});
		return cdn;
	}

	/** The paths it holds a copy of. */
	stored(): string[] {
		return [...this.#copies.keys()];
	}

	async stop(): Promise<void> {
		this.#server.closeAllConnections();
		await new Promise<void>((done) => this.#server.close(() => done()));
	}

	async #handle(request: IncomingMessage, response: ServerResponse): Promise<void> {
		const method = request.method ?? 'GET';
		const path = request.url ?? '/';
		const key = `${method} ${path}`;

		if (this.bypassCookie !== null && hasCookie(request.headers, this.bypassCookie)) {
			const answer = await this.#fetch(request);
			this.#record(method, path, answer.status, 'BYPASS', false, `cookie ${this.bypassCookie}`);
			return this.#reply(response, answer, 'BYPASS', 'no: bypassed');
		}

		const copy = this.#copies.get(key);
		if (copy !== undefined && copy.expires > Date.now()) {
			this.#record(method, path, copy.status, 'HIT', true, 'stored');
			return this.#reply(response, copy, 'HIT', 'yes');
		}

		const answer = await this.#fetch(request);
		const verdict = storable(method, request.headers, answer.status, answer.headers);
		const stored = verdict.ttlMs > 0;
		if (stored) this.#copies.set(key, { ...answer, expires: Date.now() + verdict.ttlMs });
		this.#record(method, path, answer.status, 'MISS', stored, verdict.reason);
		this.#reply(response, answer, 'MISS', stored ? 'yes' : `no: ${verdict.reason}`);
	}

	#record(method: string, path: string, status: number, outcome: Outcome, stored: boolean, reason: string) {
		this.log.push({ method, path, status, outcome, stored, reason });
	}

	#reply(response: ServerResponse, answer: { status: number; headers: IncomingHttpHeaders; body: Buffer }, outcome: Outcome, stored: string) {
		response.writeHead(answer.status, { ...strip(answer.headers), 'content-length': String(answer.body.length), 'x-fake-cdn': outcome, 'x-fake-cdn-stored': stored });
		response.end(answer.body);
	}

	async #fetch(request: IncomingMessage): Promise<{ status: number; headers: IncomingHttpHeaders; body: Buffer }> {
		const body = await collect(request);
		return new Promise((done, fail) => {
			const upstream = forward(
				{ host: this.#upstream.host, port: this.#upstream.port, method: request.method, path: request.url, headers: strip(request.headers) },
				(answer) => {
					void collect(answer).then((buffer) => done({ status: answer.statusCode ?? 502, headers: answer.headers, body: buffer }), fail);
				}
			);
			upstream.once('error', fail);
			upstream.end(body);
		});
	}
}

function collect(stream: IncomingMessage): Promise<Buffer> {
	return new Promise((done, fail) => {
		const chunks: Buffer[] = [];
		stream.on('data', (chunk: Buffer) => chunks.push(chunk));
		stream.once('end', () => done(Buffer.concat(chunks)));
		stream.once('error', fail);
	});
}
