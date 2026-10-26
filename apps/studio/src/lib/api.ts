// The Studio's only way to talk to colophon-server, which serves it on the same origin
// (D-037): the session cookie travels by itself, and writes echo the CSRF token.
//
// Every failure is a typed `ApiError` saying what kind it is, because the editor reacts to each
// differently: a conflict opens the conflict screen, a refusal shows the reason, and a network
// failure or a busy server means "keep the edit and try again".

import type { RichTextValue } from '@colophon/editor';

export interface Project {
	id: string;
	slug: string;
	name: string;
	role: string;
}

export interface Me {
	id: string;
	issuer: string;
	display_name: string | null;
	projects: Project[];
}

export interface Article {
	title: string;
	body: RichTextValue;
}

export interface Revision {
	id: string;
	base_revision_id: string | null;
	author_id: string;
	created_at: string;
	content_hash: string;
	title: string;
}

export interface Draft {
	id: string;
	collection: string;
	created_at: string;
	updated_at: string;
	revision: Revision;
	content: Article;
}

export interface Summary {
	id: string;
	updated_at: string;
	revision: Revision;
}

/** A draft and the ETag that names its revision, for the next save's `If-Match`. */
export interface Loaded {
	draft: Draft;
	etag: string;
	/** True when the server answered from its idempotency record (a retried request). */
	replayed: boolean;
}

export type Failure =
	/** Not signed in, or the session ended. */
	| 'signed-out'
	/** The draft moved on since this revision; `current` names the new one. */
	| 'conflict'
	/** The server refused the content or the request; `message` says why. */
	| 'refused'
	/** No such project or article, or not one this person may see. */
	| 'not-found'
	/** No answer, or the server is busy or failing: the same request can be sent again. */
	| 'retry';

export class ApiError extends Error {
	constructor(
		readonly failure: Failure,
		message: string,
		readonly status: number | null = null,
		readonly reason: string | null = null,
		/** For a conflict: the current revision's ETag. */
		readonly current: string | null = null
	) {
		super(message);
	}
}

/** The session's CSRF token, which every cookie-authenticated write echoes. */
export function csrfToken(): string {
	const match = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='));
	return match?.slice('colophon_csrf='.length) ?? '';
}

/** Where "Sign in" goes; the server comes back to `returnTo` afterwards. */
export function signInUrl(returnTo: string): string {
	return `/auth/login?return_to=${encodeURIComponent(returnTo)}`;
}

/** Every request gives up after this long and counts as "no answer": a save is then retried with
 *  the same key, so nothing waits forever (invariant ST4). */
export const REQUEST_TIMEOUT_MS = 15_000;

interface Envelope {
	error?: { code?: string; message?: string; details?: { reason?: string } };
}

async function send(method: string, path: string, body?: unknown, headers: Record<string, string> = {}): Promise<Response> {
	const write = method !== 'GET';
	let response: Response;
	try {
		response = await fetch(path, {
			method,
			cache: 'no-store',
			signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
			headers: {
				Accept: 'application/json',
				...(write ? { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken() } : {}),
				...headers
			},
			...(body === undefined ? {} : { body: JSON.stringify(body) })
		});
	} catch {
		throw new ApiError('retry', 'no answer from the server');
	}
	if (response.ok) return response;
	const envelope = ((await response.json().catch(() => ({}))) as Envelope).error;
	const message = envelope?.message ?? `HTTP ${response.status}`;
	const reason = envelope?.details?.reason ?? null;
	switch (true) {
		case response.status === 401:
			throw new ApiError('signed-out', message, 401);
		case response.status === 404:
			throw new ApiError('not-found', message, 404);
		case response.status === 409 && reason === 'stale_revision':
			throw new ApiError('conflict', message, 409, reason, response.headers.get('etag'));
		case response.status >= 500 || response.status === 429:
			throw new ApiError('retry', message, response.status, reason);
		default:
			throw new ApiError('refused', message, response.status, reason);
	}
}

async function loaded(response: Response): Promise<Loaded> {
	const { data } = (await response.json()) as { data: Draft };
	return {
		draft: data,
		etag: response.headers.get('etag') ?? '',
		replayed: response.headers.get('idempotent-replayed') === 'true'
	};
}

const entries = (project: string) => `/api/v1/projects/${project}/environments/production/collections/articles/entries`;

/** Who is signed in, or `null` if nobody is. */
export async function me(): Promise<Me | null> {
	try {
		return ((await (await send('GET', '/me')).json()) as { data: Me }).data;
	} catch (error) {
		if (error instanceof ApiError && error.failure === 'signed-out') return null;
		throw error;
	}
}

export async function listArticles(project: string): Promise<Summary[]> {
	return ((await (await send('GET', `${entries(project)}?limit=100`)).json()) as { data: Summary[] }).data;
}

export async function getArticle(project: string, entry: string): Promise<Loaded> {
	return loaded(await send('GET', `${entries(project)}/${entry}`));
}

export async function createArticle(project: string, article: Article, key: string): Promise<Loaded> {
	return loaded(await send('POST', entries(project), article, { 'Idempotency-Key': key }));
}

/** Saves `article` on top of the revision `etag` names. Sending the same key again is safe. */
export async function saveArticle(project: string, entry: string, article: Article, etag: string, key: string): Promise<Loaded> {
	return loaded(await send('PUT', `${entries(project)}/${entry}`, article, { 'If-Match': etag, 'Idempotency-Key': key }));
}

export async function listRevisions(project: string, entry: string): Promise<Revision[]> {
	return ((await (await send('GET', `${entries(project)}/${entry}/revisions`)).json()) as { data: Revision[] }).data;
}

/** A fresh idempotency key: 32 hex characters. */
export function newKey(): string {
	return crypto.randomUUID().replaceAll('-', '');
}
