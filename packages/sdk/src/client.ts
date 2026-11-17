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
import { judge, readReport, type Compatibility } from './compat.js';
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

/* ---------------------------------------------------------------- locales */
//
// Hand-written until `docs/api/openapi.json` carries the environment's language list and the
// delivery API's locale fields; `api.types.ts` is generated from that document and never edited
// by hand. When it does, these become aliases of the generated schemas (`ReplyJson<…>`), and the
// intersections below stay harmless — the same fields, declared twice.

/** One language in an environment's list (`GET {P}/locales`). */
export interface LocaleSetting {
	/** BCP-47-shaped: 2–3 letters, then `-` subtags of 1–8 letters or digits (`en`, `pt-BR`,
	 *  `zh-Hant`), at most 35 characters. Unique in the list, ignoring case. */
	tag: string;
	/** What a person sees: 1–80 characters. */
	label: string;
	/** Exactly one language in a non-empty list is the default — what an untagged entry counts as. */
	default: boolean;
	/** The tags a read in this language tries next, in order: at most four, each listed, never
	 *  the tag itself or a repeat. */
	fallback: string[];
}

/** An environment's languages, in display order. An empty list at version 0 means one language
 *  with free tags; `version` goes up by one on every successful `putLocales`. */
export interface LocaleList {
	version: number;
	locales: LocaleSetting[];
}

/** What a `?locale=` read resolved: the tag asked for, the tag actually served (`null` for an
 *  untagged entry in an environment with no default language), and whether that was a fallback. */
export interface Localization {
	requested: string;
	resolved: string | null;
	fallback: boolean;
}

/** A published sibling of the entry read — the same entry in another language. `slug` is absent
 *  when the collection has none. */
export interface Alternate {
	id: string;
	locale: string | null;
	slug?: string;
}

/** A delivery row: the published entry, its locale (the default tag for an untagged entry when
 *  the environment lists languages, else `null`), and — on a `?locale=` read — what resolved. */
export type DeliveryEntry = Omit<
	components['schemas']['PublishedEntryView'],
	'locale' | 'localization' | 'requested_id' | 'alternates'
> & {
	locale?: string | null;
	localization?: Localization;
};

/** One entry read: a row plus its published siblings at that generation (`[]` when it has
 *  none). On a `?locale=` read the row may be a sibling: `id` is the sibling's, `requested_id`
 *  the id that was asked for. */
export type DeliveryEntryRead = DeliveryEntry & {
	requested_id?: string;
	alternates?: Alternate[];
};

/** `Delivery.list`'s answer. */
export type DeliveryListReply = Omit<ReplyJson<'deliveryList'>, 'data'> & { data: DeliveryEntry[] };
/** `Delivery.get`'s answer. */
export type DeliveryGetReply = Omit<ReplyJson<'deliveryGet'>, 'data'> & { data: DeliveryEntryRead };
/** `Delivery.query`'s answer. */
export type DeliveryQueryReply = Omit<ReplyJson<'deliveryQuery'>, 'data'> & { data: DeliveryEntry[] };

/**
 * How a localized read falls back when an entry has nothing published in the asked tag:
 * `'policy'` (the server's default) follows the environment's chain for that tag; `'none'`
 * answers exact matches only; a list names this read's own chain, at most four tags.
 */
export type Fallback = 'policy' | 'none' | readonly string[];

export interface LocaleOptions {
	fallback?: Fallback;
}

/** The most tags a `?fallback=` list may name — the server refuses more (`invalid_fallback`). */
export const MAX_FALLBACKS = 4;

/** A tag as it can ride a query string unchanged in meaning: non-empty, no comma (the list
 *  separator), no whitespace. Its BCP-47 shape is the server's to judge. */
function tagArg(tag: string, what: string): string {
	if (tag === '' || /[\s,]/.test(tag)) {
		throw new TypeError(`${what} must be a language tag such as "pt-BR", got ${JSON.stringify(tag)}`);
	}
	return tag;
}

/** `?fallback=`'s value: absent for `'policy'` (the server's default — one URL per answer, so a
 *  CDN key never splits on it), `none`, or the list joined with commas. */
function fallbackArg(fallback: Fallback | undefined): string | undefined {
	if (fallback === undefined || fallback === 'policy') return undefined;
	if (fallback === 'none') return 'none';
	if (typeof fallback === 'string') {
		throw new TypeError(`fallback must be 'policy', 'none' or a list of tags, got ${JSON.stringify(fallback)}`);
	}
	if (fallback.length > MAX_FALLBACKS) {
		throw new TypeError(`a fallback chain names at most ${MAX_FALLBACKS} tags, got ${fallback.length}`);
	}
	if (fallback.length === 0) return 'none';
	return fallback.map((tag) => tagArg(tag, 'a fallback')).join(',');
}

/** A delivery scope's locale: the tag every read names and the `?fallback=` it sends, if any
 *  (`Delivery.inLocale` builds it). */
export interface LocaleScope {
	readonly locale: string;
	readonly fallback: string | undefined;
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
	/** The server-version check (`client.compatibility()`), run once, lazily, before the first
	 *  API call: `'warn'` (the default) logs the plain-words reason through `console.warn` and
	 *  never delays or fails a call; `'throw'` waits for the check and fails every call with
	 *  `incompatible_server` while the server doesn't fit; `'off'` never asks. A check that gets
	 *  no answer decides nothing and is tried again on the next call. */
	compat?: 'off' | 'warn' | 'throw';
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
	/** The transport without the compatibility gate: health, version, the check itself. */
	readonly #raw: Send;
	readonly #base: string;
	/** The settled check, once one got an answer. */
	#checked: Compatibility | null = null;
	/** The check in flight; `null` again after one that got no answer. */
	#checking: Promise<Compatibility | null> | null = null;

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
		const compat = options.compat ?? 'warn';
		if (compat !== 'off' && compat !== 'warn' && compat !== 'throw') {
			throw new TypeError(`compat must be 'off', 'warn' or 'throw', not ${JSON.stringify(compat)}`);
		}
		this.#base = base;

		const raw = async <B>(call: Call): Promise<Reply<B>> => {
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
		this.#raw = raw;
		this.#send = async <B>(call: Call): Promise<Reply<B>> => {
			if (compat === 'throw') {
				const result = this.#checked ?? (await this.#check());
				if (result !== null && !result.compatible) {
					throw new ApiError(0, 'incompatible_server', result.reason ?? 'the server does not fit this SDK');
				}
			} else if (compat === 'warn' && this.#checked === null) {
				// Never in the way of the call: the answer arrives beside it.
				void this.#check(true);
			}
			return raw<B>(call);
		};
	}

	/** One check at a time; an unanswered one is forgotten so the next call asks again. With
	 *  `warn`, a misfit is said once, when the answer settles. */
	#check(warn = false): Promise<Compatibility | null> {
		this.#checking ??= this.compatibility().then(
			(result) => {
				this.#checked = result;
				if (warn && !result.compatible) console.warn(`colophon: ${result.reason}`);
				return result;
			},
			() => {
				this.#checking = null;
				return null;
			}
		);
		return this.#checking;
	}

	/**
	 * `GET /version`, judged by the CLI's rule (D-062): the server must serve API `v1`, share this
	 * SDK's minor line before 1.0, and be at least the oldest server this SDK supports. Answers
	 * `{ compatible, sdk, server, api }`, plus `misfit` (a stable code) and `reason` (plain words:
	 * which side should move, and to what) when it doesn't fit. A server without `/version`
	 * predates every supported one. Fails as `ApiError` only when the server can't be asked
	 * (`no_answer`) or answers something that isn't a version report (`invalid_response`).
	 */
	async compatibility(options: ReadOptions = {}): Promise<Compatibility> {
		let body: unknown;
		try {
			body = await this.#raw({ method: 'GET', path: '/version', ...(options.signal !== undefined ? { signal: options.signal } : {}) });
		} catch (error) {
			if (error instanceof ApiError && error.status === 404) return judge(this.#base, null);
			throw error;
		}
		const report = readReport(body);
		if (report === null) {
			throw new ApiError(200, 'invalid_response', `${this.#base}/version answered something other than a version report`);
		}
		return judge(this.#base, report);
	}

	/* ------------------------------------------------------- the whole server */

	/** `GET /livez` — the process answers at all. Unauthenticated. */
	livez(options: ReadOptions = {}): Promise<Reply<ReplyJson<'liveness'>>> {
		return this.#raw({ method: 'GET', path: '/livez', ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `GET /readyz` — every dependency's check, honest about `degraded`/`not_ready`. */
	readyz(options: ReadOptions = {}): Promise<Reply<ReplyJson<'readiness'>>> {
		return this.#raw({ method: 'GET', path: '/readyz', ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `GET /version` — release, commit and the API versions served. Unauthenticated; the shape
	 *  only ever grows fields. */
	version(options: ReadOptions = {}): Promise<Reply<ReplyJson<'serverVersion'>>> {
		return this.#raw({ method: 'GET', path: '/version', ...(options.signal !== undefined ? { signal: options.signal } : {}) });
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
	 * it. `delivery(project).atGeneration(g)` pins every read to one release (invariant K1);
	 * `.inLocale(tag, { fallback })` reads in one language.
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

	/* ------------------------------------------------------------- languages */

	/** `GET {P}/locales` — the environment's languages in display order, each with its default
	 *  flag and fallback chain. `response.etag` (`"locales-{version}"`) is `putLocales`' `ifMatch`.
	 *  Any member of the project may read it. */
	getLocales(options: ReadOptions = {}): Promise<Reply<{ data: LocaleList }>> {
		return this.#send({ method: 'GET', path: `${this.#base}/locales`, ...(options.signal !== undefined ? { signal: options.signal } : {}) });
	}

	/** `PUT {P}/locales` — the whole list, in order, on top of the version `ifMatch` names
	 *  (`schema.change`). Refusals: 409 `stale_locales` (changed since — `error.etag` is the
	 *  current one), 409 `locale_in_use` (a removed tag live entries still carry: `details.entries`,
	 *  `details.total`), 422 `validation_failed` with its reason and pointer (`invalid_locale`,
	 *  `duplicate_locale`, `locale_default_required`, `fallback_not_listed`, `invalid_fallback`,
	 *  `too_many_locales`, `invalid_locale_label`). A replay of the same key and body gets the
	 *  recorded answer. */
	putLocales(locales: readonly LocaleSetting[], opts: GuardedOptions): Promise<Reply<{ data: LocaleList }>> {
		const { signal, idempotencyKey, ifMatch } = opts;
		return this.#send({
			method: 'PUT',
			path: `${this.#base}/locales`,
			headers: { 'if-match': ifMatch, 'idempotency-key': idempotencyKey },
			body: { locales },
			...(signal !== undefined ? { signal } : {})
		});
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
 * the reads one page makes can never mix releases (invariant K1). `inLocale(tag)` returns a scope
 * whose reads all ask for one language, falling back as the environment's policy (or the read's
 * own chain) says; the two compose in either order.
 */
export class Delivery {
	readonly #send: Send;
	readonly #project: string;
	readonly #environment: string;
	readonly #generation: number | undefined;
	readonly #locale: LocaleScope | undefined;

	constructor(send: Send, project: string, environment: string, generation?: number, locale?: LocaleScope) {
		this.#send = send;
		this.#project = project;
		this.#environment = environment;
		this.#generation = generation;
		this.#locale = locale;
	}

	/** The same reads, pinned to `generation` for every call that follows (K1). A call may not
	 *  name a different generation on a pinned scope — that would be the mixing the pin exists to
	 *  prevent. A locale the scope already has is kept. */
	atGeneration(generation: number): Delivery {
		return new Delivery(this.#send, this.#project, this.#environment, generation, this.#locale);
	}

	/**
	 * The same reads in one language: every `list`, `get` and `query` sends `?locale=tag` (and
	 * `?fallback=` unless it's the policy). Rows then carry `localization` — the tag asked for,
	 * the tag served and whether that was a fallback; an entry `get` answers the entry's best
	 * sibling (`requested_id` names the one asked for), or 404 `not_found` with
	 * `details.reason` `not_in_locale` when nothing in the chain is published —
	 * `availableLocales(error)` lists what is. A generation the scope is pinned to is kept.
	 */
	inLocale(tag: string, options: LocaleOptions = {}): Delivery {
		const locale: LocaleScope = { locale: tagArg(tag, 'the locale'), fallback: fallbackArg(options.fallback) };
		return new Delivery(this.#send, this.#project, this.#environment, this.#generation, locale);
	}

	#collection(collection: string): string {
		return `/content/v1/${segment(this.#project)}/${segment(this.#environment)}/${segment(collection)}`;
	}

	/** The scope's query: the pinned (or asked) generation, then the locale and its fallback. */
	#scoped(asked: number | undefined): Record<string, QueryValue | undefined> {
		if (this.#generation !== undefined && asked !== undefined && asked !== this.#generation) {
			throw new TypeError(`this delivery scope is pinned to generation ${this.#generation}`);
		}
		return { generation: this.#generation ?? asked, locale: this.#locale?.locale, fallback: this.#locale?.fallback };
	}

	/** `GET /content/v1/{project}/{environment}/{collection}` — published entries: `limit`,
	 *  `cursor`, and `generation` to read a past release. In a locale, one row per translation
	 *  group — its best published member for the tag and chain. */
	list(collection: string, opts: { limit?: number; cursor?: string; generation?: number } & ReadOptions = {}): Promise<Reply<DeliveryListReply>> {
		const { signal, generation, ...rest } = opts;
		return this.#send({ method: 'GET', path: this.#collection(collection), query: { ...rest, ...this.#scoped(generation) }, ...(signal !== undefined ? { signal } : {}) });
	}

	/** `GET /content/v1/{project}/{environment}/{collection}/{entry}` — one published entry, with
	 *  its published siblings in `alternates`. */
	get(collection: string, entry: string, opts: { generation?: number } & ReadOptions = {}): Promise<Reply<DeliveryGetReply>> {
		const { signal, generation } = opts;
		return this.#send({ method: 'GET', path: `${this.#collection(collection)}/${segment(entry)}`, query: this.#scoped(generation), ...(signal !== undefined ? { signal } : {}) });
	}

	/** `POST /content/v1/{project}/{environment}/{collection}/query` — the same bounded document
	 *  as `queryEntries`, over the published projection; `cursor` continues a page. */
	query(collection: string, document: RequestJson<'deliveryQuery'>, opts: { cursor?: string; generation?: number } & ReadOptions = {}): Promise<Reply<DeliveryQueryReply>> {
		const { signal, generation, ...rest } = opts;
		return this.#send({ method: 'POST', path: `${this.#collection(collection)}/query`, query: { ...rest, ...this.#scoped(generation) }, body: document, ...(signal !== undefined ? { signal } : {}) });
	}
}
