// The hand-written half of the SDK (D-112): one `fetch` loop under typed method names. Types
// come from `api.types.ts`, which `scripts/gen-types.mjs` regenerates from the committed OpenAPI
// document — the wire shapes can never drift from what the server serves, and the methods' names
// and semantics stay reviewed code rather than generator output.
//
// What every call guarantees:
//   - the credential the caller gave is attached, nothing more — this client never signs in
//     (one API, no bypass);
//   - a write names its `If-Match` and `Idempotency-Key` explicitly, so a retry of the same
//     logical write can reuse one key (invariant K2: a retried mutation reuses its key);
//   - the reply body is bounded (`maxBodyBytes`) and the wait is bounded (`timeoutMs` or the
//     caller's `signal`);
//   - every failure is an `ApiError` carrying the error envelope's code, message, request id and
//     details (refusal reason + JSON pointer), never a bare `TypeError` from fetch.

import { ApiError, errorOf } from './error.js';
import type { components, operations } from './api.types.js';

/* ------------------------------------------------------------------ types */

/** The operation named `O`'s JSON request body, `never` for a bodiless call. */
export type RequestJson<O extends keyof operations> =
	operations[O] extends { requestBody: { content: { 'application/json': infer B } } } ? B : never;

/** The operation named `O`'s success body — its 200, else its 201. */
export type ReplyJson<O extends keyof operations> =
	operations[O]['responses'] extends { 200: { content: { 'application/json': infer B } } }
		? B
		: operations[O]['responses'] extends { 201: { content: { 'application/json': infer B } } }
			? B
			: never;

/** The operation named `O`'s documented query parameters. */
export type QueryOf<O extends keyof operations> =
	operations[O]['parameters'] extends { query?: infer Q } ? NonNullable<Q> : Record<string, never>;

/** What a response said besides its body. */
export interface ReplyMeta {
	/** The HTTP status (200 or 201). */
	readonly status: number;
	/** The `ETag` header — a write's `ifMatch` names it back; `null` when the route sends none. */
	readonly etag: string | null;
	/** `Idempotent-Replayed: true` — the stored result of an earlier send of this key. */
	readonly replayed: boolean;
	/** `x-request-id`, to quote in support or match against server logs. */
	readonly requestId: string | null;
}

/** A call's answer: the parsed body, in its exact wire shape, plus the response's metadata. */
export type Reply<B> = B & { response: ReplyMeta };

/** Per-call options every method takes. */
export interface CallOptions {
	/** Aborts this request from the caller's side; joins the client's own `timeoutMs`. */
	signal?: AbortSignal;
}
export type ReadOptions = CallOptions;

/** A write's options: the `Idempotency-Key` the route requires (16–128 of `A-Z a-z 0-9 - _`,
 *  scoped to the actor, operation and environment). Retries of the same logical write reuse one
 *  key — `newIdempotencyKey()` mints them. */
export interface WriteOptions extends CallOptions {
	idempotencyKey: string;
}
/** A guarded write's options: the `If-Match` the route requires — the resource's `ETag`, exactly
 *  as a read's `response.etag` carried it. */
export interface MatchOptions extends CallOptions {
	ifMatch: string;
}
/** A write needing both `If-Match` and `Idempotency-Key`. */
export interface GuardedOptions extends WriteOptions, MatchOptions {}

/** A minted `Idempotency-Key`: 36 characters of the allowed alphabet. */
export function newIdempotencyKey(): string {
	return crypto.randomUUID();
}

/** How the caller proves who it is. Exactly one of `ClientOptions.token` / `.session`. */
export interface SessionCredential {
	/** The readable `colophon_csrf` cookie's value, echoed as `X-CSRF-Token` on every write —
	 *  cookie-authenticated writes are refused without it. */
	csrf: string;
	/** Outside a browser only: the `colophon_session` cookie's value, sent as the `Cookie`
	 *  header. Omit it in a browser — the session cookie travels by itself there (and the header
	 *  can't be set). */
	cookie?: string;
}

export interface ClientOptions {
	/** The server's origin, such as `http://localhost:58000`. */
	baseUrl: string;
	/** A bearer credential — a personal access token from `colophon login`, or a project API
	 *  token (`colophon_api_…`). The client attaches it; it never refreshes, stores or mints one. */
	token?: string;
	/** A cookie-session descriptor: the CSRF token every write echoes, and — outside a browser —
	 *  the session cookie's value. */
	session?: SessionCredential;
	/** The `fetch` to use; defaults to the runtime's. */
	fetch?: typeof fetch;
	/** Extra headers sent on every request — an `Origin` the server trusts, for cookie-authed
	 *  writes outside a browser. A credential's own headers (`Authorization`, `Cookie`,
	 *  `X-CSRF-Token`) are applied after these, so a configured credential always wins. */
	headers?: Record<string, string>;
	/** How long one request may take; `AbortSignal.timeout` underneath. Default 15,000 ms. */
	timeoutMs?: number;
	/** The most a response body may run to, in bytes — declared or measured; the read stops past
	 *  it with `response_too_large`. Default 16 MiB. */
	maxBodyBytes?: number;
}

/* ------------------------------------------------------------- the transport */

type QueryValue = string | number | boolean;

interface Call {
	method: string;
	path: string;
	query?: Record<string, QueryValue | undefined>;
	headers?: Record<string, string | undefined>;
	body?: unknown;
	signal?: AbortSignal;
}

type Send = <B>(call: Call) => Promise<Reply<B>>;

const JSON_MEDIA = 'application/json';
const DEFAULT_TIMEOUT_MS = 15_000;
const DEFAULT_MAX_BODY_BYTES = 16 * 1024 * 1024;

/** `Retry-After` as milliseconds: whole seconds or an HTTP-date, clamped to a minute; `null`
 *  when the header is absent or unreadable. */
function retryAfterMs(header: string | null, now = Date.now()): number | null {
	const text = header?.trim() ?? '';
	if (text === '') return null;
	const wait = /^\d+$/.test(text)
		? Number(text) * 1000
		: Number.isNaN(Date.parse(text))
			? null
			: Date.parse(text) - now;
	return wait === null ? null : Math.min(Math.max(wait, 0), 60_000);
}

/** The session cookie's name (the OpenAPI `sessionCookie` scheme). */
const SESSION_COOKIE = 'colophon_session';

const segment = encodeURIComponent;

/**
 * Reads a response body with a hard cap: past `cap` bytes — declared by `Content-Length` or
 * measured as chunks arrive — the stream is cancelled and `response_too_large` thrown. Bounded
 * everything: no answer may grow without limit.
 */
async function readBounded(response: Response, cap: number, meta: ReplyMeta): Promise<string> {
	const declared = Number(response.headers.get('content-length'));
	if (Number.isFinite(declared) && declared > cap) {
		await response.body?.cancel().catch(() => {});
		throw new ApiError(meta.status, 'response_too_large', `the response declares ${declared} bytes, over the ${cap}-byte cap`, {
			requestId: meta.requestId
		});
	}
	const stream = response.body;
	if (stream === null) {
		const text = await response.text();
		if (text.length > cap) {
			throw new ApiError(meta.status, 'response_too_large', `the response ran past the ${cap}-byte cap`, {
				requestId: meta.requestId
			});
		}
		return text;
	}
	const reader = stream.getReader();
	const chunks: Uint8Array[] = [];
	let total = 0;
	try {
		for (;;) {
			const { done, value } = await reader.read();
			if (done) break;
			total += value.byteLength;
			if (total > cap) {
				await reader.cancel().catch(() => {});
				throw new ApiError(meta.status, 'response_too_large', `the response ran past the ${cap}-byte cap`, {
					requestId: meta.requestId
				});
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

/* ----------------------------------------------------------------- Client */

/**
 * One server's API. `new Client({ baseUrl, token })` — then `client.environment(project)` for the
 * content surface, `client.delivery(project)` for published reads, or the root methods for health
 * and identity.
 */
export class Client {
	readonly #send: Send;

	constructor(options: ClientOptions) {
		const base = options.baseUrl.replace(/\/+$/, '');
		const parsed = new URL(base);
		if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
			throw new TypeError(`baseUrl ${JSON.stringify(options.baseUrl)} is not an http(s) URL`);
		}
		if (options.token !== undefined && options.session !== undefined) {
			throw new TypeError('pass `token` or `session`, not both');
		}
		const agent = options.fetch ?? fetch;
		if (agent === undefined) throw new TypeError('no fetch in this runtime: pass ClientOptions.fetch');
		const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
		const maxBodyBytes = options.maxBodyBytes ?? DEFAULT_MAX_BODY_BYTES;
		const token = options.token;
		const session = options.session;
		const fixed = options.headers ?? {};

		this.#send = async <B>(call: Call): Promise<Reply<B>> => {
			const url = new URL(base + call.path);
			for (const [key, value] of Object.entries(call.query ?? {})) {
				if (value !== undefined) url.searchParams.set(key, String(value));
			}
			const write = call.method !== 'GET' && call.method !== 'HEAD';
			const headers: Record<string, string> = { accept: JSON_MEDIA, ...fixed };
			if (token !== undefined) headers['authorization'] = `Bearer ${token}`;
			if (write) headers['content-type'] = JSON_MEDIA;
			if (session !== undefined) {
				if (write) headers['x-csrf-token'] = session.csrf;
				if (session.cookie !== undefined) headers['cookie'] = `${SESSION_COOKIE}=${session.cookie}`;
			}
			for (const [key, value] of Object.entries(call.headers ?? {})) {
				if (value !== undefined) headers[key.toLowerCase()] = value;
			}

			const signals = [AbortSignal.timeout(timeoutMs)];
			if (call.signal !== undefined) signals.push(call.signal);
			let response: Response;
			try {
				response = await agent(url, {
					method: call.method,
					headers,
					...(call.body === undefined ? {} : { body: JSON.stringify(call.body) }),
					signal: AbortSignal.any(signals),
					cache: 'no-store'
				});
			} catch (cause) {
				throw new ApiError(0, 'no_answer', `no answer from ${base}`, { cause });
			}

			const meta: ReplyMeta = {
				status: response.status,
				etag: response.headers.get('etag'),
				replayed: response.headers.get('idempotent-replayed') === 'true',
				requestId: response.headers.get('x-request-id')
			};
			const text = await readBounded(response, maxBodyBytes, meta);
			let body: unknown = null;
			if (text !== '') {
				try {
					body = JSON.parse(text);
				} catch (cause) {
					throw new ApiError(response.status, 'invalid_response', `a non-JSON body answered HTTP ${response.status}`, {
						requestId: meta.requestId,
						cause
					});
				}
			}
			if (response.ok) {
				return { ...(body as object), response: meta } as Reply<B>;
			}
			const error = errorOf(body);
			throw new ApiError(response.status, error?.code ?? 'invalid_response', error?.message ?? `request failed (HTTP ${response.status})`, {
				requestId: meta.requestId ?? error?.request_id ?? null,
				details: error?.details ?? null,
				etag: meta.etag,
				retryAfterMs: retryAfterMs(response.headers.get('retry-after'))
			});
		};
	}

	/* ------------------------------------------------------- the whole server */

	/** `GET /livez` — the process answers at all. Unauthenticated. */
	livez(options: ReadOptions = {}): Promise<Reply<ReplyJson<'liveness'>>> {
		return this.#send({ method: 'GET', path: '/livez', ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `GET /readyz` — every dependency's check, honest about `degraded`/`not_ready`. */
	readyz(options: ReadOptions = {}): Promise<Reply<ReplyJson<'readiness'>>> {
		return this.#send({ method: 'GET', path: '/readyz', ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `GET /version` — release, commit and the API versions served. Unauthenticated; the shape
	 *  only ever grows fields. */
	version(options: ReadOptions = {}): Promise<Reply<ReplyJson<'serverVersion'>>> {
		return this.#send({ method: 'GET', path: '/version', ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `GET /me` — who the credential names, the projects it may see and whether it's an
	 *  administrator; `unauthorized` when it names nobody. */
	me(options: ReadOptions = {}): Promise<Reply<ReplyJson<'me'>>> {
		return this.#send({ method: 'GET', path: '/me', ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `POST /api/v1/rich-text/validate` — a `colophon/rich-text` v1 document checked exactly as a
	 *  mutation would check it, stored nowhere. */
	validateRichText(doc: components['schemas']['RichTextDocument'], options: ReadOptions = {}): Promise<Reply<ReplyJson<'validateRichText'>>> {
		return this.#send({ method: 'POST', path: '/api/v1/rich-text/validate', body: doc, ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/* -------------------------------------------------------------- projects */

	/** `GET /api/v1/projects` — the projects the credential may see, a page at a time. */
	listProjects(opts: QueryOf<'listProjects'> & ReadOptions = {}): Promise<Reply<ReplyJson<'listProjects'>>> {
		const { signal, ...query } = opts;
		return this.#send({ method: 'GET', path: '/api/v1/projects', query, ...(signal !== undefined ? { signal } : {}) });
	}

	/** `GET /api/v1/projects/{project}` — one project, with the caller's permissions on it. */
	getProject(project: string, options: ReadOptions = {}): Promise<Reply<ReplyJson<'getProject'>>> {
		return this.#send({ method: 'GET', path: `/api/v1/projects/${segment(project)}`, ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/**
	 * The content surface of one project's environment — `{P}`, the
	 * `/api/v1/projects/{project}/environments/{environment}` prefix every collection, changeset
	 * and release route shares.
	 */
	environment(project: string, environment = 'production'): Environment {
		return new Environment(this.#send, project, environment);
	}

	/**
	 * The public delivery surface — `/content/v1`, published content only, no draft ever reaches
	 * it. `delivery(project).atGeneration(g)` pins every read to one release (invariant K1).
	 */
	delivery(project: string, environment = 'production'): Delivery {
		return new Delivery(this.#send, project, environment);
	}
}

/* ------------------------------------------------------------- environment */

/** The `{P}` scope — one project's environment — bound once so no call repeats it. */
export class Environment {
	readonly #send: Send;
	readonly #base: string;

	constructor(send: Send, project: string, environment: string) {
		this.#send = send;
		this.#base = `/api/v1/projects/${segment(project)}/environments/${segment(environment)}`;
	}

	/* ------------------------------------------------------------- identity */

	/** `GET {P}/permissions` — what the caller's role allows here, as data. */
	ownPermissions(options: ReadOptions = {}): Promise<Reply<ReplyJson<'ownPermissions'>>> {
		return this.#send({ method: 'GET', path: `${this.#base}/permissions`, ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/* -------------------------------------------------------------- entries */

	#entries(collection: string): string {
		return `${this.#base}/collections/${segment(collection)}/entries`;
	}

	/** `GET {P}/collections/{collection}/entries` — drafts, newest-revision first: `limit`,
	 *  `cursor`, title search `q`, `created_by` (`me` or an id) and `locale`. */
	listEntries(collection: string, opts: QueryOf<'listEntries'> & ReadOptions = {}): Promise<Reply<ReplyJson<'listEntries'>>> {
		const { signal, ...query } = opts;
		return this.#send({ method: 'GET', path: this.#entries(collection), query, ...(signal !== undefined ? { signal } : {}) });
	}

	/** `GET {P}/collections/{collection}/entries/{entry}` — one draft, with its `ETag` in
	 *  `response.etag` for the next save's `ifMatch`. */
	getEntry(collection: string, entry: string, options: ReadOptions = {}): Promise<Reply<ReplyJson<'getEntry'>>> {
		return this.#send({ method: 'GET', path: `${this.#entries(collection)}/${segment(entry)}`, ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `POST {P}/collections/{collection}/entries` — a new draft; `locale` names the entry's
	 *  locale (absent leaves it unlocalized). */
	createEntry(collection: string, document: RequestJson<'createEntry'>, opts: WriteOptions & { locale?: string }): Promise<Reply<ReplyJson<'createEntry'>>> {
		const { signal, idempotencyKey, locale } = opts;
		return this.#send({
			method: 'POST',
			path: this.#entries(collection),
			query: { locale },
			headers: { 'idempotency-key': idempotencyKey },
			body: document,
			...(signal !== undefined ? { signal } : {})
		});
	}

	/** `PUT {P}/collections/{collection}/entries/{entry}` — save on top of the revision `ifMatch`
	 *  names; `locale` sets, an empty `locale` clears, absent leaves it. */
	saveEntry(collection: string, entry: string, document: RequestJson<'saveEntry'>, opts: GuardedOptions & { locale?: string }): Promise<Reply<ReplyJson<'saveEntry'>>> {
		const { signal, idempotencyKey, ifMatch, locale } = opts;
		return this.#send({
			method: 'PUT',
			path: `${this.#entries(collection)}/${segment(entry)}`,
			query: { locale },
			headers: { 'if-match': ifMatch, 'idempotency-key': idempotencyKey },
			body: document,
			...(signal !== undefined ? { signal } : {})
		});
	}

	/** `DELETE {P}/collections/{collection}/entries/{entry}` — a tombstone the database refuses
	 *  while anything depends on the entry (409 `still_referenced`, `referenced_by` in `details`). */
	deleteEntry(collection: string, entry: string, opts: GuardedOptions): Promise<Reply<ReplyJson<'deleteEntry'>>> {
		const { signal, idempotencyKey, ifMatch } = opts;
		return this.#send({
			method: 'DELETE',
			path: `${this.#entries(collection)}/${segment(entry)}`,
			headers: { 'if-match': ifMatch, 'idempotency-key': idempotencyKey },
			...(signal !== undefined ? { signal } : {})
		});
	}

	/** `GET {P}/collections/{collection}/entries/{entry}/revisions` — every revision kept. */
	entryHistory(collection: string, entry: string, options: ReadOptions = {}): Promise<Reply<ReplyJson<'entryHistory'>>> {
		return this.#send({ method: 'GET', path: `${this.#entries(collection)}/${segment(entry)}/revisions`, ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `GET {P}/collections/{collection}/entries/{entry}/revisions/{revision}` — one revision's
	 *  document, for comparison against the current draft. */
	getRevision(collection: string, entry: string, revision: string, options: ReadOptions = {}): Promise<Reply<ReplyJson<'getRevision'>>> {
		return this.#send({ method: 'GET', path: `${this.#entries(collection)}/${segment(entry)}/revisions/${segment(revision)}`, ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `POST {P}/collections/{collection}/entries/query` — the bounded query document (D-103)
	 *  over current drafts; `cursor` continues a page. The answer echoes the canonical document
	 *  and its `query_hash` in `applied`. */
	queryEntries(collection: string, document: RequestJson<'queryEntries'>, opts: { cursor?: string } & ReadOptions = {}): Promise<Reply<ReplyJson<'queryEntries'>>> {
		const { signal, ...query } = opts;
		return this.#send({ method: 'POST', path: `${this.#entries(collection)}/query`, query, body: document, ...(signal !== undefined ? { signal } : {}) });
	}

	/* --------------------------------------------------------- content types */

	#type(slug: string): string {
		return `${this.#base}/content-types/${segment(slug)}`;
	}

	/** `GET {P}/content-types` — the collections and single types, each at its current version. */
	listContentTypes(options: ReadOptions = {}): Promise<Reply<ReplyJson<'listContentTypes'>>> {
		return this.#send({ method: 'GET', path: `${this.#base}/content-types`, ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `GET {P}/content-types/{slug}` — one type's definition and `definition_hash`; the `ETag`
	 *  names its version for the next change. */
	getContentType(slug: string, options: ReadOptions = {}): Promise<Reply<ReplyJson<'getContentType'>>> {
		return this.#send({ method: 'GET', path: this.#type(slug), ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `POST {P}/content-types` — a new collection or single type. */
	createContentType(body: RequestJson<'createContentType'>, opts: WriteOptions): Promise<Reply<ReplyJson<'createContentType'>>> {
		const { signal, idempotencyKey } = opts;
		return this.#send({ method: 'POST', path: `${this.#base}/content-types`, headers: { 'idempotency-key': idempotencyKey }, body, ...(signal !== undefined ? { signal } : {}) });
	}

	/** `PUT {P}/content-types/{slug}` — a new version on top of `ifMatch`; changes are checked
	 *  against stored content, so a refused one names the entries in the way. */
	updateContentType(slug: string, definition: components['schemas']['TypeDefinition'], opts: GuardedOptions): Promise<Reply<ReplyJson<'updateContentType'>>> {
		const { signal, idempotencyKey, ifMatch } = opts;
		return this.#send({ method: 'PUT', path: this.#type(slug), headers: { 'if-match': ifMatch, 'idempotency-key': idempotencyKey }, body: definition, ...(signal !== undefined ? { signal } : {}) });
	}

	/** `DELETE {P}/content-types/{slug}` — only an empty collection may go; the refusal names
	 *  what still lives there. */
	deleteContentType(slug: string, opts: GuardedOptions): Promise<Reply<ReplyJson<'deleteContentType'>>> {
		const { signal, idempotencyKey, ifMatch } = opts;
		return this.#send({ method: 'DELETE', path: this.#type(slug), headers: { 'if-match': ifMatch, 'idempotency-key': idempotencyKey }, ...(signal !== undefined ? { signal } : {}) });
	}

	/* ------------------------------------------------------------ changesets */

	#changeset(id: string): string {
		return `${this.#base}/changesets/${segment(id)}`;
	}

	/** `GET {P}/changesets` — open and decided changesets. */
	listChangesets(options: ReadOptions = {}): Promise<Reply<ReplyJson<'listChangesets'>>> {
		return this.#send({ method: 'GET', path: `${this.#base}/changesets`, ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `GET {P}/changesets/{changeset}` — one changeset; its `ETag` names the hash reviews and
	 *  submits pin (`ifMatch`). */
	getChangeset(changeset: string, options: ReadOptions = {}): Promise<Reply<ReplyJson<'getChangeset'>>> {
		return this.#send({ method: 'GET', path: this.#changeset(changeset), ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `POST {P}/changesets` — a changeset pinning exact revisions. */
	createChangeset(body: RequestJson<'createChangeset'>, opts: WriteOptions): Promise<Reply<ReplyJson<'createChangeset'>>> {
		const { signal, idempotencyKey } = opts;
		return this.#send({ method: 'POST', path: `${this.#base}/changesets`, headers: { 'idempotency-key': idempotencyKey }, body, ...(signal !== undefined ? { signal } : {}) });
	}

	/** `POST {P}/changesets/{changeset}/submit` — ask for review on the hash `ifMatch` names. */
	submitChangeset(changeset: string, opts: MatchOptions): Promise<Reply<ReplyJson<'submitChangeset'>>> {
		const { signal, ifMatch } = opts;
		return this.#send({ method: 'POST', path: `${this.#changeset(changeset)}/submit`, headers: { 'if-match': ifMatch }, ...(signal !== undefined ? { signal } : {}) });
	}

	/** `POST {P}/changesets/{changeset}/reviews` — approve or request changes, bound to the hash
	 *  `ifMatch` names (and to a `stage` where the environment names review stages). */
	reviewChangeset(changeset: string, review: RequestJson<'reviewChangeset'>, opts: MatchOptions): Promise<Reply<ReplyJson<'reviewChangeset'>>> {
		const { signal, ifMatch } = opts;
		return this.#send({ method: 'POST', path: `${this.#changeset(changeset)}/reviews`, headers: { 'if-match': ifMatch }, body: review, ...(signal !== undefined ? { signal } : {}) });
	}

	/** `POST {P}/changesets/{changeset}/publish` — commit a release on `base_generation`; the
	 *  reply is the accepted publish, not proof it's served — `getReceipt` reads the evidence. */
	publishChangeset(changeset: string, release: RequestJson<'publishChangeset'>, opts: GuardedOptions): Promise<Reply<ReplyJson<'publishChangeset'>>> {
		const { signal, idempotencyKey, ifMatch } = opts;
		return this.#send({ method: 'POST', path: `${this.#changeset(changeset)}/publish`, headers: { 'if-match': ifMatch, 'idempotency-key': idempotencyKey }, body: release, ...(signal !== undefined ? { signal } : {}) });
	}

	/* -------------------------------------------------------------- releases */

	/** `GET {P}/releases` — the generations, newest first. */
	listReleases(options: ReadOptions = {}): Promise<Reply<ReplyJson<'listReleases'>>> {
		return this.#send({ method: 'GET', path: `${this.#base}/releases`, ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `GET {P}/releases/{generation}` — the publish receipt: deliveries and the probes that saw
	 *  them, the evidence a release is really out. */
	getReceipt(generation: number, options: ReadOptions = {}): Promise<Reply<ReplyJson<'getReceipt'>>> {
		return this.#send({ method: 'GET', path: `${this.#base}/releases/${generation}`, ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/* -------------------------------------------------------------- comments */

	#comments(collection: string, entry: string): string {
		return `${this.#entries(collection)}/${segment(entry)}/comments`;
	}

	/** `GET {P}/collections/{collection}/entries/{entry}/comments` — the discussion: `limit`,
	 *  `cursor`, `revision`, `state`. */
	listComments(collection: string, entry: string, opts: QueryOf<'listComments'> & ReadOptions = {}): Promise<Reply<ReplyJson<'listComments'>>> {
		const { signal, ...query } = opts;
		return this.#send({ method: 'GET', path: this.#comments(collection, entry), query, ...(signal !== undefined ? { signal } : {}) });
	}

	/** `GET {P}/collections/{collection}/entries/{entry}/comments/{comment}` — one comment; its
	 *  `ETag` is the resolve/reopen `ifMatch`. */
	getComment(collection: string, entry: string, comment: string, options: ReadOptions = {}): Promise<Reply<ReplyJson<'getComment'>>> {
		return this.#send({ method: 'GET', path: `${this.#comments(collection, entry)}/${segment(comment)}`, ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `POST {P}/collections/{collection}/entries/{entry}/comments` — a comment anchored on one
	 *  immutable revision. */
	createComment(collection: string, entry: string, body: RequestJson<'createComment'>, opts: WriteOptions): Promise<Reply<ReplyJson<'createComment'>>> {
		const { signal, idempotencyKey } = opts;
		return this.#send({ method: 'POST', path: this.#comments(collection, entry), headers: { 'idempotency-key': idempotencyKey }, body, ...(signal !== undefined ? { signal } : {}) });
	}

	/** `POST {P}/collections/{collection}/entries/{entry}/comments/{comment}/resolve`. */
	resolveComment(collection: string, entry: string, comment: string, opts: GuardedOptions): Promise<Reply<ReplyJson<'resolveComment'>>> {
		const { signal, idempotencyKey, ifMatch } = opts;
		return this.#send({ method: 'POST', path: `${this.#comments(collection, entry)}/${segment(comment)}/resolve`, headers: { 'if-match': ifMatch, 'idempotency-key': idempotencyKey }, ...(signal !== undefined ? { signal } : {}) });
	}

	/** `POST {P}/collections/{collection}/entries/{entry}/comments/{comment}/reopen`. */
	reopenComment(collection: string, entry: string, comment: string, opts: GuardedOptions): Promise<Reply<ReplyJson<'reopenComment'>>> {
		const { signal, idempotencyKey, ifMatch } = opts;
		return this.#send({ method: 'POST', path: `${this.#comments(collection, entry)}/${segment(comment)}/reopen`, headers: { 'if-match': ifMatch, 'idempotency-key': idempotencyKey }, ...(signal !== undefined ? { signal } : {}) });
	}

	/* ----------------------------------------------------------------- tasks */

	#task(id: string): string {
		return `${this.#base}/tasks/${segment(id)}`;
	}

	/** `GET {P}/tasks` — the work list: `status`, `collection`, `entry`, `assignee`, `limit`,
	 *  `cursor`. */
	listTasks(opts: QueryOf<'listTasks'> & ReadOptions = {}): Promise<Reply<ReplyJson<'listTasks'>>> {
		const { signal, ...query } = opts;
		return this.#send({ method: 'GET', path: `${this.#base}/tasks`, query, ...(signal !== undefined ? { signal } : {}) });
	}

	/** `GET {P}/tasks/{task}` — one task; its `ETag` is the update's `ifMatch`. */
	getTask(task: string, options: ReadOptions = {}): Promise<Reply<ReplyJson<'getTask'>>> {
		return this.#send({ method: 'GET', path: this.#task(task), ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `POST {P}/tasks` — ask a person for something on an entry or revision. */
	createTask(body: RequestJson<'createTask'>, opts: WriteOptions): Promise<Reply<ReplyJson<'createTask'>>> {
		const { signal, idempotencyKey } = opts;
		return this.#send({ method: 'POST', path: `${this.#base}/tasks`, headers: { 'idempotency-key': idempotencyKey }, body, ...(signal !== undefined ? { signal } : {}) });
	}

	/** `PATCH {P}/tasks/{task}` — retitle, reassign or close it (`open`, `done`, `cancelled`). */
	updateTask(task: string, patch: RequestJson<'updateTask'>, opts: GuardedOptions): Promise<Reply<ReplyJson<'updateTask'>>> {
		const { signal, idempotencyKey, ifMatch } = opts;
		return this.#send({ method: 'PATCH', path: this.#task(task), headers: { 'if-match': ifMatch, 'idempotency-key': idempotencyKey }, body: patch, ...(signal !== undefined ? { signal } : {}) });
	}

	/* ------------------------------------------------------------- migrations */

	#migration(id: string): string {
		return `${this.#base}/migrations/${segment(id)}`;
	}

	/** `GET {P}/migrations` — content migrations by `collection`/`state`. Reads only: planning,
	 *  approving and applying a migration stays a person's CLI act (D-106). */
	listMigrations(opts: QueryOf<'listMigrations'> & ReadOptions = {}): Promise<Reply<ReplyJson<'listMigrations'>>> {
		const { signal, ...query } = opts;
		return this.#send({ method: 'GET', path: `${this.#base}/migrations`, query, ...(signal !== undefined ? { signal } : {}) });
	}

	/** `GET {P}/migrations/{migration}` — one plan and its state. */
	getMigration(migration: string, options: ReadOptions = {}): Promise<Reply<ReplyJson<'getMigration'>>> {
		return this.#send({ method: 'GET', path: this.#migration(migration), ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `GET {P}/migrations/{migration}/items` — the per-entry outcomes (`outcome`, `after`,
	 *  `limit`). */
	listMigrationItems(migration: string, opts: QueryOf<'listMigrationItems'> & ReadOptions = {}): Promise<Reply<ReplyJson<'listMigrationItems'>>> {
		const { signal, ...query } = opts;
		return this.#send({ method: 'GET', path: `${this.#migration(migration)}/items`, query, ...(signal !== undefined ? { signal } : {}) });
	}

	/* ---------------------------------------------------------------- assets */

	/** `GET {P}/assets` — the media library. */
	listAssets(options: ReadOptions = {}): Promise<Reply<ReplyJson<'listAssets'>>> {
		return this.#send({ method: 'GET', path: `${this.#base}/assets`, ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `GET {P}/assets/{asset}` — one asset and its renditions. Serving the bytes is
	 *  `/media/v1/…`, deliberately not wrapped: a stream, not JSON. */
	getAsset(asset: string, options: ReadOptions = {}): Promise<Reply<ReplyJson<'getAsset'>>> {
		return this.#send({ method: 'GET', path: `${this.#base}/assets/${segment(asset)}`, ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}
}

/* --------------------------------------------------------------- delivery */

/**
 * `/content/v1/{project}/{environment}` — the published projection, readable without signing in.
 * Every read may name a `generation`; `atGeneration(g)` returns a scope that always names it, so
 * the reads one page makes can never mix releases (invariant K1).
 */
export class Delivery {
	readonly #send: Send;
	readonly #project: string;
	readonly #environment: string;
	readonly #generation: number | undefined;

	constructor(send: Send, project: string, environment: string, generation?: number) {
		this.#send = send;
		this.#project = project;
		this.#environment = environment;
		this.#generation = generation;
	}

	/** The same reads, pinned to `generation` for every call that follows (K1). A call may not
	 *  name a different generation on a pinned scope — that would be the mixing the pin exists to
	 *  prevent. */
	atGeneration(generation: number): Delivery {
		return new Delivery(this.#send, this.#project, this.#environment, generation);
	}

	#collection(collection: string): string {
		return `/content/v1/${segment(this.#project)}/${segment(this.#environment)}/${segment(collection)}`;
	}

	#gen(asked: number | undefined): number | undefined {
		if (this.#generation !== undefined && asked !== undefined && asked !== this.#generation) {
			throw new TypeError(`this delivery scope is pinned to generation ${this.#generation}`);
		}
		return this.#generation ?? asked;
	}

	/** `GET /content/v1/{project}/{environment}/{collection}` — published entries: `limit`,
	 *  `cursor`, and `generation` to read a past release. */
	list(collection: string, opts: { limit?: number; cursor?: string; generation?: number } & ReadOptions = {}): Promise<Reply<ReplyJson<'deliveryList'>>> {
		const { signal, generation, ...rest } = opts;
		return this.#send({ method: 'GET', path: this.#collection(collection), query: { ...rest, generation: this.#gen(generation) }, ...(signal !== undefined ? { signal } : {}) });
	}

	/** `GET /content/v1/{project}/{environment}/{collection}/{entry}` — one published entry. */
	get(collection: string, entry: string, opts: { generation?: number } & ReadOptions = {}): Promise<Reply<ReplyJson<'deliveryGet'>>> {
		const { signal, generation } = opts;
		return this.#send({ method: 'GET', path: `${this.#collection(collection)}/${segment(entry)}`, query: { generation: this.#gen(generation) }, ...(signal !== undefined ? { signal } : {}) });
	}

	/** `POST /content/v1/{project}/{environment}/{collection}/query` — the same bounded document
	 *  as `queryEntries`, over the published projection; `cursor` continues a page. */
	query(collection: string, document: RequestJson<'deliveryQuery'>, opts: { cursor?: string; generation?: number } & ReadOptions = {}): Promise<Reply<ReplyJson<'deliveryQuery'>>> {
		const { signal, generation, ...rest } = opts;
		return this.#send({ method: 'POST', path: `${this.#collection(collection)}/query`, query: { ...rest, generation: this.#gen(generation) }, body: document, ...(signal !== undefined ? { signal } : {}) });
	}
}
