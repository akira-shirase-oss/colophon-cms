// Draft preview on the customer's own site (the `/preview/v1` surface): framework-agnostic helpers
// on the Web `Request`/`Response` types, so one module serves SvelteKit, Astro, Next.js, Hono,
// Remix or a bare Node/Deno/Bun handler.
//
// The flow: the Studio issues a one-time code (`cpc_…`, 60 s, single use) and loads the site's
// handler — `https://site.example/api/preview?code=cpc_…&path=/blog/post`. The handler exchanges
// the code server-side for a short-lived bearer token (`cpv_…`), keeps it in the site's own
// `HttpOnly` cookie, and redirects to `path`. The site's pages then read drafts through
// `previewClient(…).reader(token)` with `Authorization: Bearer` — Colophon never relies on a cookie
// of its own inside the frame — and answer with `previewHeaders(…)`, so no shared cache keeps a
// draft and no search engine indexes one. In the browser, `listenForDraftUpdates` tells the Studio
// the page is ready (or that its cookie was blocked) and reloads it after each acknowledged save.
//
// What each helper guarantees:
//   - `path` is a same-site relative path or `/`: never an open redirect, even through `/..//`;
//   - every answer that touches a token or a draft says `no-store` and `noindex`;
//   - the token never reaches the browser's JavaScript (an `HttpOnly` cookie) nor a `Referer`;
//   - messages are posted to, and accepted from, exactly one Studio origin — never `*`;
//   - every failure from Colophon is an `ApiError` keeping `details.reason` (`preview_expired`,
//     `preview_revoked`, `preview_stale`, `preview_published`, `preview_not_found`, …).

import { ApiError, errorOf } from './error.js';
import type { ReplyJson, RequestJson } from './client.js';
import type { components } from './api.types.js';

/* ------------------------------------------------------------------ types */

/** What a preview grant covers: one entry (and its translation group, following their latest
 *  drafts) or one publish request's pinned revisions. */
export interface PreviewScope {
	kind: 'entry' | 'changeset';
	/** The entry previewed, for an `entry` scope. */
	entry?: string;
	/** The changeset previewed, for a `changeset` scope. */
	changeset?: string;
	/** The entry's collection, when the scope names one. */
	collection?: string;
}

/** A code exchanged: the bearer token to keep in the site's own cookie, when it ends, and what
 *  it may read. */
export interface PreviewExchange {
	/** `cpv_…` — send as `Authorization: Bearer`; never expose it to the page's JavaScript. */
	token: string;
	/** When the token stops working (ISO 8601). */
	expiresAt: string;
	scope: PreviewScope;
}

/** The live session a token names — for the site's "Preview mode" banner. */
export interface PreviewSession {
	scope: PreviewScope;
	/** When the token stops working (ISO 8601); a Studio renewal moves it forward. */
	expiresAt: string;
}

/** One row of the preview surface: the published view's shape, byte for byte (wire names kept),
 *  plus `preview` — where the content came from (`draft`: the entry's latest saved draft;
 *  `changeset`: a publish request's pinned revision; `published`: an entry outside the scope, as
 *  the public sees it) and `hidden_fields`, the fields the issuer's role may not read. */
export type PreviewEntry = components['schemas']['PreviewEntryView'];

/** A page of preview rows (`GET /preview/v1/…/{collection}`). */
export type PreviewPage = ReplyJson<'previewList'>;

/** A page of a preview query (`POST /preview/v1/…/{collection}/query`). */
export type PreviewQueryPage = ReplyJson<'previewQuery'>;

/** The query document a preview query takes — the same bounded document as delivery's. */
export type PreviewQueryDocument = RequestJson<'previewQuery'>;

/** What a preview read said besides its body. */
export interface PreviewMeta {
	readonly status: number;
	readonly requestId: string | null;
	/** `Colophon-Preview: entry|changeset` — the scope the answer was read under. */
	readonly preview: string | null;
	/** `Colophon-Generation` — the release the non-draft rows were read at. */
	readonly generation: number | null;
}

/** A preview read's answer: the parsed body in its wire shape, plus the response's metadata. */
export type PreviewReply<B> = B & { response: PreviewMeta };

export interface PreviewClientOptions {
	/** Colophon's origin, such as `https://cms.example.com`. */
	baseUrl: string;
	/** The project's id or slug — the one the Studio issues previews for. */
	project: string;
	/** The environment; `production` by default. */
	environment?: string;
	/** The `fetch` to use; defaults to the runtime's. */
	fetch?: typeof fetch;
	/** How long one request may take. Default 10,000 ms — a page render waits on it. */
	timeoutMs?: number;
	/** The most a response body may run to, in bytes. Default 16 MiB. */
	maxBodyBytes?: number;
}

/** Reads with one preview token. */
export interface PreviewReader {
	/** `GET /preview/v1/{project}/{environment}/{collection}` — a page of rows, drafts where the
	 *  scope covers them. */
	list(collection: string, opts?: { limit?: number; cursor?: string; signal?: AbortSignal }): Promise<PreviewReply<PreviewPage>>;
	/** `GET /preview/v1/{project}/{environment}/{collection}/{entry}` — one row. */
	get(collection: string, id: string, opts?: { signal?: AbortSignal }): Promise<PreviewReply<ReplyJson<'previewGet'>>>;
	/** `POST /preview/v1/{project}/{environment}/{collection}/query` — the bounded query document. */
	query(collection: string, document: PreviewQueryDocument, opts?: { cursor?: string; signal?: AbortSignal }): Promise<PreviewReply<PreviewQueryPage>>;
	/** `GET /preview/v1/{project}/{environment}/session` — the scope and end of this token. */
	session(opts?: { signal?: AbortSignal }): Promise<PreviewSession>;
}

export interface PreviewClient {
	/** `POST /preview/v1/{project}/{environment}/exchange` — trade a one-time code for a token.
	 *  Unknown, used and expired codes all fail as a 404 `ApiError` (`preview_not_found`). */
	exchange(code: string, opts?: { signal?: AbortSignal }): Promise<PreviewExchange>;
	/** The reads one token may make. */
	reader(token: string): PreviewReader;
}

/* ------------------------------------------------------------- the transport */

const DEFAULT_TIMEOUT_MS = 10_000;
const DEFAULT_MAX_BODY_BYTES = 16 * 1024 * 1024;
/** The exchange body's cap on the server is 1 KiB; a code never comes near it. */
const MAX_CODE_LENGTH = 512;
const segment = encodeURIComponent;

interface Call {
	method: 'GET' | 'POST';
	path: string;
	query?: Record<string, string | number | undefined>;
	token?: string;
	body?: unknown;
	signal?: AbortSignal | undefined;
}

interface Answer {
	body: unknown;
	meta: PreviewMeta;
}

async function readBounded(response: Response, cap: number, status: number, requestId: string | null): Promise<string> {
	const tooLarge = () => new ApiError(status, 'response_too_large', `the response ran past the ${cap}-byte cap`, { requestId });
	const declared = Number(response.headers.get('content-length'));
	if (Number.isFinite(declared) && declared > cap) {
		await response.body?.cancel().catch(() => {});
		throw tooLarge();
	}
	if (response.body === null) {
		const text = await response.text();
		if (text.length > cap) throw tooLarge();
		return text;
	}
	const reader = response.body.getReader();
	const chunks: Uint8Array[] = [];
	let total = 0;
	try {
		for (;;) {
			const { done, value } = await reader.read();
			if (done) break;
			total += value.byteLength;
			if (total > cap) {
				await reader.cancel().catch(() => {});
				throw tooLarge();
			}
			chunks.push(value);
		}
	} finally {
		reader.releaseLock();
	}
	const body = new Uint8Array(total);
	let at = 0;
	for (const chunk of chunks) {
		body.set(chunk, at);
		at += chunk.byteLength;
	}
	return new TextDecoder().decode(body);
}

function retryAfterMs(header: string | null): number | null {
	const text = header?.trim() ?? '';
	if (!/^\d+$/.test(text)) return null;
	return Math.min(Number(text) * 1000, 60_000);
}

function generationOf(header: string | null): number | null {
	if (header === null || !/^\d+$/.test(header.trim())) return null;
	return Number(header.trim());
}

/** A `fetch` loop for `/preview/v1`: bounded wait, bounded body, every failure an `ApiError`. */
function transport(options: PreviewClientOptions): (call: Call) => Promise<Answer> {
	const base = options.baseUrl.replace(/\/+$/, '');
	const parsed = new URL(base);
	if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
		throw new TypeError(`baseUrl ${JSON.stringify(options.baseUrl)} is not an http(s) URL`);
	}
	if (options.project === '') throw new TypeError('previewClient needs a project');
	const agent = options.fetch ?? globalThis.fetch;
	if (agent === undefined) throw new TypeError('no fetch in this runtime: pass `fetch`');
	const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
	const cap = options.maxBodyBytes ?? DEFAULT_MAX_BODY_BYTES;

	return async (call) => {
		const url = new URL(base + call.path);
		for (const [key, value] of Object.entries(call.query ?? {})) {
			if (value !== undefined) url.searchParams.set(key, String(value));
		}
		const headers: Record<string, string> = { accept: 'application/json' };
		if (call.token !== undefined) headers['authorization'] = `Bearer ${call.token}`;
		if (call.body !== undefined) headers['content-type'] = 'application/json';
		const signals = [AbortSignal.timeout(timeoutMs)];
		if (call.signal !== undefined) signals.push(call.signal);

		let response: Response;
		try {
			response = await agent(url, {
				method: call.method,
				headers,
				...(call.body === undefined ? {} : { body: JSON.stringify(call.body) }),
				signal: AbortSignal.any(signals),
				cache: 'no-store',
				// A preview read never follows a redirect: a bearer token goes to Colophon only, and
				// a 3xx fails below as `invalid_response`.
				redirect: 'manual'
			});
		} catch (cause) {
			throw new ApiError(0, 'no_answer', `no answer from ${base}`, { cause });
		}

		const requestId = response.headers.get('x-request-id');
		const meta: PreviewMeta = {
			status: response.status,
			requestId,
			preview: response.headers.get('colophon-preview'),
			generation: generationOf(response.headers.get('colophon-generation'))
		};
		let text: string;
		try {
			text = await readBounded(response, cap, response.status, requestId);
		} catch (cause) {
			if (cause instanceof ApiError) throw cause;
			// The deadline (or the caller) cut the body off: nothing usable came back.
			throw new ApiError(0, 'no_answer', `the answer from ${base} was cut off`, { requestId, cause });
		}
		let body: unknown = null;
		if (text !== '') {
			try {
				body = JSON.parse(text);
			} catch (cause) {
				throw new ApiError(response.status, 'invalid_response', `a non-JSON body answered HTTP ${response.status}`, { requestId, cause });
			}
		}
		if (response.ok) return { body, meta };
		const error = errorOf(body);
		throw new ApiError(response.status, error?.code ?? 'invalid_response', error?.message ?? `request failed (HTTP ${response.status})`, {
			requestId: requestId ?? error?.request_id ?? null,
			details: error?.details ?? null,
			retryAfterMs: retryAfterMs(response.headers.get('retry-after'))
		});
	};
}

function record(value: unknown): Record<string, unknown> | null {
	return typeof value === 'object' && value !== null && !Array.isArray(value) ? (value as Record<string, unknown>) : null;
}

function invalid(status: number, requestId: string | null, what: string): ApiError {
	return new ApiError(status, 'invalid_response', `the preview ${what} is not in the documented shape`, { requestId });
}

function scopeOf(value: unknown): PreviewScope | null {
	const fields = record(value);
	if (fields === null || (fields['kind'] !== 'entry' && fields['kind'] !== 'changeset')) return null;
	const scope: PreviewScope = { kind: fields['kind'] };
	for (const key of ['entry', 'changeset', 'collection'] as const) {
		const named = fields[key];
		if (typeof named === 'string') scope[key] = named;
	}
	return scope;
}

/** A token as the cookie and the `Authorization` header carry it: RFC 6265 cookie octets only. */
const TOKEN = /^[\x21\x23-\x2b\x2d-\x3a\x3c-\x5b\x5d-\x7e]{1,1024}$/;

/* ---------------------------------------------------------------- the client */

/**
 * The preview surface of one project's environment, for the site's server: `exchange` a one-time
 * code, then `reader(token)` for the reads a page makes.
 *
 *     const preview = previewClient({ baseUrl: CMS, project: 'blog' });
 *     const { data } = await preview.reader(token).get('articles', id);
 */
export function previewClient(options: PreviewClientOptions): PreviewClient {
	const send = transport(options);
	const root = `/preview/v1/${segment(options.project)}/${segment(options.environment ?? 'production')}`;

	const reader = (token: string): PreviewReader => {
		if (!TOKEN.test(token)) throw new TypeError('not a preview token');
		const collectionPath = (collection: string) => `${root}/${segment(collection)}`;
		return {
			async list(collection, opts = {}) {
				const { body, meta } = await send({ method: 'GET', path: collectionPath(collection), token, query: { limit: opts.limit, cursor: opts.cursor }, signal: opts.signal });
				return { ...(body as PreviewPage), response: meta };
			},
			async get(collection, id, opts = {}) {
				const { body, meta } = await send({ method: 'GET', path: `${collectionPath(collection)}/${segment(id)}`, token, signal: opts.signal });
				return { ...(body as ReplyJson<'previewGet'>), response: meta };
			},
			async query(collection, document, opts = {}) {
				const { body, meta } = await send({ method: 'POST', path: `${collectionPath(collection)}/query`, token, query: { cursor: opts.cursor }, body: document, signal: opts.signal });
				return { ...(body as PreviewQueryPage), response: meta };
			},
			async session(opts = {}) {
				const { body, meta } = await send({ method: 'GET', path: `${root}/session`, token, signal: opts.signal });
				const data = record(record(body)?.['data']);
				const scope = scopeOf(data?.['scope']);
				const expiresAt = data?.['expires_at'];
				if (scope === null || typeof expiresAt !== 'string') throw invalid(meta.status, meta.requestId, 'session');
				return { scope, expiresAt };
			}
		};
	};

	return {
		async exchange(code, opts = {}) {
			if (code === '' || code.length > MAX_CODE_LENGTH) {
				throw new ApiError(404, 'not_found', 'this preview link has expired or was already used', { details: { reason: 'preview_not_found' } });
			}
			const { body, meta } = await send({ method: 'POST', path: `${root}/exchange`, body: { code }, signal: opts.signal });
			const data = record(record(body)?.['data']);
			const token = data?.['token'];
			const expiresAt = data?.['expires_at'];
			const scope = scopeOf(data?.['scope']);
			if (typeof token !== 'string' || !TOKEN.test(token) || typeof expiresAt !== 'string' || Number.isNaN(Date.parse(expiresAt)) || scope === null) {
				throw invalid(meta.status, meta.requestId, 'exchange');
			}
			return { token, expiresAt, scope };
		},
		reader
	};
}

/* ------------------------------------------------------- the site's server */

/** The site's preview cookie, unless the handler names another. */
export const PREVIEW_COOKIE = 'colophon_preview';
/** The longest a preview cookie may live: a renewed grant ends 8 hours after it was made. */
const MAX_COOKIE_SECONDS = 8 * 60 * 60;
/** The longest `path` a handler follows. */
const MAX_PATH_LENGTH = 2048;
/** RFC 6265 `token`: what a cookie name may be. */
const COOKIE_NAME = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;
const CONTROL = /[\x00-\x1f\x7f]/;

function cookieNameOf(name: string | undefined): string {
	const cookieName = name ?? PREVIEW_COOKIE;
	if (!COOKIE_NAME.test(cookieName)) throw new TypeError(`${JSON.stringify(cookieName)} is not a valid cookie name`);
	return cookieName;
}

/**
 * `path` as a same-site redirect target, or `null`: a relative path starting with one `/`, no
 * backslash or control character, at most 2,048 characters, that still names a path on this
 * site once resolved (so `/..//evil.example` is refused too). The answer is the resolved,
 * percent-encoded form, which is what `isAllowedPath` sees and what `Location` carries.
 */
export function safePreviewPath(path: string | null | undefined, isAllowedPath?: (path: string) => boolean): string | null {
	if (typeof path !== 'string' || path.length === 0 || path.length > MAX_PATH_LENGTH) return null;
	if (!path.startsWith('/') || path.startsWith('//') || path.includes('\\') || CONTROL.test(path)) return null;
	const probe = 'http://site.invalid';
	let resolved: URL;
	try {
		resolved = new URL(path, probe);
	} catch {
		return null;
	}
	if (resolved.origin !== probe) return null;
	const safe = resolved.pathname + resolved.search + resolved.hash;
	if (!safe.startsWith('/') || safe.startsWith('//') || safe.includes('\\') || safe.length > MAX_PATH_LENGTH) return null;
	if (isAllowedPath !== undefined && !isAllowedPath(safe)) return null;
	return safe;
}

/** The headers every answer of the handler carries. */
function guarded(extra: Record<string, string> = {}): Headers {
	return new Headers({ 'Cache-Control': 'no-store', 'Referrer-Policy': 'no-referrer', 'X-Robots-Tag': 'noindex', ...extra });
}

function page(status: number, title: string, text: string, extra: Record<string, string> = {}): Response {
	const html = `<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="robots" content="noindex"><meta name="viewport" content="width=device-width, initial-scale=1"><title>${title}</title></head><body><main><h1>${title}</h1><p>${text}</p></main></body></html>`;
	return new Response(html, { status, headers: guarded({ 'Content-Type': 'text/html; charset=utf-8', ...extra }) });
}

function cookie(name: string, value: string, maxAge: number): string {
	return `${name}=${value}; HttpOnly; Secure; SameSite=None; Partitioned; Path=/; Max-Age=${maxAge}`;
}

/** Seconds from now until `expiresAt`, within `0 ..= 8 h`. */
function secondsLeft(expiresAt: string, now: number): number {
	const left = Math.floor((Date.parse(expiresAt) - now) / 1000);
	return Number.isFinite(left) ? Math.min(Math.max(left, 0), MAX_COOKIE_SECONDS) : 0;
}

export interface PreviewHandlerOptions extends PreviewClientOptions {
	/** The cookie the token is kept in. Default `colophon_preview`. */
	cookieName?: string;
	/** A further rule for `path` — say, only `/blog/…` — given the resolved path. A refused path
	 *  sends the visitor to `/` instead. */
	isAllowedPath?: (path: string) => boolean;
	/** The clock, for tests. */
	now?: () => number;
}

/**
 * The site's preview endpoint, `GET /api/preview?code=…&path=…` (any route works: the Studio's
 * preview target names it). It exchanges the code, keeps the token in an `HttpOnly; Secure;
 * SameSite=None; Partitioned` cookie that lives exactly as long as the token, and answers `303`
 * to `path` — or to `/` when `path` is missing, foreign or refused. An unknown, used or expired
 * code answers a small `404` page. Every answer is `no-store`, `noindex` and `no-referrer`.
 */
export function createPreviewHandler(options: PreviewHandlerOptions): (request: Request) => Promise<Response> {
	const client = previewClient(options);
	const cookieName = cookieNameOf(options.cookieName);
	const now = options.now ?? Date.now;

	return async (request) => {
		if (request.method !== 'GET') {
			return page(405, 'Method not allowed', 'Open the preview link from the Studio.', { Allow: 'GET' });
		}
		// A speculative prefetch would spend the one-time code before the person ever looks.
		if ((request.headers.get('sec-purpose') ?? request.headers.get('purpose') ?? '').includes('prefetch')) {
			return page(503, 'Not prefetched', 'Preview links are opened, not prefetched.');
		}
		const url = new URL(request.url);
		const code = url.searchParams.get('code') ?? '';
		const target = safePreviewPath(url.searchParams.get('path'), options.isAllowedPath) ?? '/';

		let exchanged: PreviewExchange;
		try {
			// Not `request.signal`: some servers abort it once the request body is read, and a
			// code spent by an aborted exchange is gone for good. The client's deadline bounds it.
			exchanged = await client.exchange(code);
		} catch (error) {
			if (error instanceof ApiError && error.status === 404) {
				return page(404, 'Preview link expired', 'This preview link has expired or was already used. Open the preview again from the Studio.');
			}
			if (error instanceof ApiError && error.status === 429) {
				return page(503, 'Preview is busy', 'Too many previews are open right now. Try again in a moment.', { 'Retry-After': String(Math.ceil((error.retryAfterMs ?? 1000) / 1000)) });
			}
			return page(502, 'Preview unavailable', 'The preview could not be started. Try again from the Studio.');
		}

		return new Response(null, {
			status: 303,
			headers: guarded({
				Location: target,
				'Set-Cookie': cookie(cookieName, exchanged.token, secondsLeft(exchanged.expiresAt, now()))
			})
		});
	};
}

/** The preview token the request's cookie carries, or `null` when there is none (or it is not a
 *  token at all). */
export function previewTokenFromRequest(request: Request, cookieName: string = PREVIEW_COOKIE): string | null {
	const name = cookieNameOf(cookieName);
	const header = request.headers.get('cookie');
	if (header === null) return null;
	for (const pair of header.split(';')) {
		const at = pair.indexOf('=');
		if (at < 0 || pair.slice(0, at).trim() !== name) continue;
		const value = pair.slice(at + 1).trim();
		if (TOKEN.test(value)) return value;
	}
	return null;
}

/** `studioOrigin` as an exact `scheme://host[:port]`, or a `TypeError`: it lands in a CSP and in
 *  `postMessage`'s `targetOrigin`, where a `*`, a path or a credential would widen who is trusted. */
function originOf(studioOrigin: string): string {
	let parsed: URL | null = null;
	try {
		parsed = new URL(studioOrigin);
	} catch {
		// reported below
	}
	if (
		parsed === null ||
		(parsed.protocol !== 'https:' && parsed.protocol !== 'http:') ||
		parsed.pathname !== '/' ||
		parsed.search !== '' ||
		parsed.hash !== '' ||
		parsed.username !== '' ||
		parsed.password !== '' ||
		studioOrigin.includes('*')
	) {
		throw new TypeError(`studioOrigin ${JSON.stringify(studioOrigin)} is not an origin (scheme://host[:port], nothing after)`);
	}
	return parsed.origin;
}

/**
 * The headers a page rendered in preview mode answers with: never stored by any cache, never
 * indexed, and framable only by the Studio.
 */
export function previewHeaders(options: { studioOrigin: string }): Record<string, string> {
	return {
		'Cache-Control': 'private, no-store',
		'X-Robots-Tag': 'noindex',
		'Content-Security-Policy': `frame-ancestors ${originOf(options.studioOrigin)}`
	};
}

export interface ExitPreviewOptions {
	cookieName?: string;
	/** Where to go when the request names no (acceptable) `?path=`. Default `/`. */
	redirectTo?: string;
	/** The same rule as the preview handler's, for `?path=`. */
	isAllowedPath?: (path: string) => boolean;
}

/**
 * The site's "exit preview" endpoint: clears the preview cookie and answers `303` to `?path=`
 * (checked exactly as the preview handler checks it) or `redirectTo`.
 */
export function exitPreviewHandler(options: ExitPreviewOptions = {}): (request: Request) => Response {
	const cookieName = cookieNameOf(options.cookieName);
	const fallback = safePreviewPath(options.redirectTo ?? '/');
	if (fallback === null) throw new TypeError(`redirectTo ${JSON.stringify(options.redirectTo)} is not a same-site path`);
	return (request) => {
		const target = safePreviewPath(new URL(request.url).searchParams.get('path'), options.isAllowedPath) ?? fallback;
		return new Response(null, {
			status: 303,
			headers: guarded({ Location: target, 'Set-Cookie': cookie(cookieName, '', 0) })
		});
	};
}

/* --------------------------------------------------------------- the browser */

/** The Studio's message after each acknowledged save. */
export interface DraftSavedMessage {
	type: 'colophon:draft-saved';
	entry: string;
	revision: string;
}

export interface DraftUpdateOptions {
	/** The Studio's exact origin — messages from anywhere else are ignored. */
	studioOrigin: string;
	/** Whether this page holds a preview session (the server saw the cookie). `false` tells the
	 *  Studio the frame's cookie was blocked, so it can offer "Open in a new tab". */
	previewing: boolean;
	/** What to do after a save; reloads the page by default. */
	onUpdate?: (message: DraftSavedMessage) => void;
}

function isDraftSaved(data: unknown): data is DraftSavedMessage {
	const fields = record(data);
	return fields !== null && fields['type'] === 'colophon:draft-saved' && typeof fields['entry'] === 'string' && typeof fields['revision'] === 'string';
}

/**
 * In the browser: tells the framing Studio this page is ready (`colophon:preview-ready`, with
 * whether it holds a preview session), then calls `onUpdate` on each `colophon:draft-saved` the
 * Studio posts — from exactly `studioOrigin`, nothing else. Returns an unsubscribe. Does nothing
 * outside a browser, so it is safe to call from shared component code.
 */
export function listenForDraftUpdates(options: DraftUpdateOptions): () => void {
	const studioOrigin = originOf(options.studioOrigin);
	if (typeof window === 'undefined') return () => {};
	const onUpdate = options.onUpdate ?? (() => window.location.reload());

	const listener = (event: MessageEvent) => {
		if (event.origin !== studioOrigin) return;
		if (isDraftSaved(event.data)) onUpdate(event.data);
	};
	window.addEventListener('message', listener);

	let framed = false;
	try {
		framed = window.parent !== window;
	} catch {
		framed = true;
	}
	if (framed) {
		window.parent.postMessage({ type: 'colophon:preview-ready', previewing: options.previewing }, studioOrigin);
	}
	return () => window.removeEventListener('message', listener);
}
