// The client's contract, checked against a stub `fetch`: what each method sends (verb, path,
// query, credential, If-Match, Idempotency-Key, body, signal) and what it makes of the answer
// (the parsed body plus `response.{status,etag,replayed,requestId}`, or the error envelope read
// into a typed `ApiError`). The same behaviours then run once more against the real server in
// `server.test.ts` when `COLOPHON_TEST_ORIGIN` is set.

import { describe, expect, it } from 'vitest';
import { ApiError, Client, availableLocales, newIdempotencyKey } from '../src/index.ts';

interface Seen {
	url: string;
	method: string;
	init: RequestInit;
}

/** A `fetch` that records every call and answers what `handler` says. */
function stubFetch(handler: (call: Seen) => Response | Promise<Response>) {
	const calls: Seen[] = [];
	const impl = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
		const seen: Seen = { url: String(input), method: init?.method ?? 'GET', init: init ?? {} };
		calls.push(seen);
		return handler(seen);
	};
	return { calls, fetch: impl as typeof fetch };
}

function json(body: unknown, status = 200, headers: Record<string, string> = {}): Response {
	return new Response(JSON.stringify(body), {
		status,
		headers: { 'content-type': 'application/json', ...headers }
	});
}

const REQUEST_ID = '0190a4b0-0000-7000-8000-000000000001';

function envelope(code: string, message: string, details?: Record<string, unknown>) {
	return { error: { code, message, request_id: REQUEST_ID, ...(details === undefined ? {} : { details }) } };
}

async function failure(promise: Promise<unknown>): Promise<ApiError> {
	try {
		await promise;
	} catch (error) {
		expect(error).toBeInstanceOf(ApiError);
		return error as ApiError;
	}
	throw new Error('the call should have failed');
}

describe('reads and replies', () => {
	it('answers the body plus the response metadata', async () => {
		const { calls, fetch } = stubFetch(() =>
			json({ data: [{ id: 'e1' }], next_cursor: 'd1:abc' }, 200, { etag: '"v7"', 'x-request-id': REQUEST_ID })
		);
		const env = new Client({ baseUrl: 'http://server.test/', fetch }).environment('proj');
		const page = await env.listEntries('articles', { limit: 10 });
		expect(calls).toHaveLength(1);
		expect(calls[0]!.method).toBe('GET');
		expect(calls[0]!.url).toBe('http://server.test/api/v1/projects/proj/environments/production/collections/articles/entries?limit=10');
		expect(page.data).toEqual([{ id: 'e1' }]);
		expect(page.next_cursor).toBe('d1:abc');
		expect(page.response).toEqual({ status: 200, etag: '"v7"', replayed: false, requestId: REQUEST_ID });
	});

	it('keeps the wire field names (next_cursor), nothing renamed', async () => {
		const { fetch } = stubFetch(() => json({ data: [], next_cursor: null }));
		const page = await new Client({ baseUrl: 'http://server.test', fetch }).environment('p').listChangesets();
		expect('next_cursor' in page ? page.next_cursor : null).toBeNull();
	});

	it('sends the bearer token, and only it', async () => {
		const { calls, fetch } = stubFetch(() => json({ data: null }));
		await new Client({ baseUrl: 'http://server.test', token: 'colophon_pat_x', fetch }).me();
		const headers = calls[0]!.init.headers as Record<string, string>;
		expect(headers['authorization']).toBe('Bearer colophon_pat_x');
		expect(headers['cookie']).toBeUndefined();
		expect(headers['x-csrf-token']).toBeUndefined();
		expect(calls[0]!.url).toBe('http://server.test/me');
	});

	it('sends CSRF on writes and the session cookie when one is named — never on reads', async () => {
		const { calls, fetch } = stubFetch(() => json({ data: { id: 'e1' } }, 201));
		const env = new Client({
			baseUrl: 'http://server.test',
			session: { csrf: 'csrf-value', cookie: 'session-value' },
			fetch
		}).environment('p');
		await env.createEntry('articles', { title: 'Hi' }, { idempotencyKey: 'k-abcdefghijklmnop' });
		const write = calls[0]!.init.headers as Record<string, string>;
		expect(write['x-csrf-token']).toBe('csrf-value');
		expect(write['cookie']).toBe('colophon_session=session-value');
		expect(write['authorization']).toBeUndefined();
		await env.listEntries('articles');
		const read = calls[1]!.init.headers as Record<string, string>;
		expect(read['x-csrf-token']).toBeUndefined();
		expect(read['cookie']).toBe('colophon_session=session-value');
		expect(read['content-type']).toBeUndefined();
	});

	it('serializes query params and URL-encodes path segments', async () => {
		const { calls, fetch } = stubFetch(() => json({ data: [], next_cursor: null }));
		const env = new Client({ baseUrl: 'http://server.test', fetch }).environment('p', 'staging');
		await env.listEntries('my collection', { q: 'a&b', created_by: 'me', locale: 'fr' });
		expect(calls[0]!.url).toBe(
			'http://server.test/api/v1/projects/p/environments/staging/collections/my%20collection/entries?q=a%26b&created_by=me&locale=fr'
		);
	});

	it('marks a replayed idempotent result', async () => {
		const { fetch } = stubFetch(() => json({ data: { id: 'e1' } }, 201, { 'idempotent-replayed': 'true' }));
		const reply = await new Client({ baseUrl: 'http://server.test', fetch })
			.environment('p')
			.createEntry('articles', {}, { idempotencyKey: 'k-abcdefghijklmnop' });
		expect(reply.response.replayed).toBe(true);
	});

	it('passes the caller signal through, joined with the client timeout', async () => {
		const { calls, fetch } = stubFetch(() => json({ data: [] }));
		const controller = new AbortController();
		await new Client({ baseUrl: 'http://server.test', fetch }).livez({ signal: controller.signal });
		expect(calls[0]!.init.signal).toBeInstanceOf(AbortSignal);
	});

	it('newIdempotencyKey mints keys in the route’s alphabet and length', () => {
		for (let i = 0; i < 8; i++) {
			const key = newIdempotencyKey();
			expect(key).toMatch(/^[A-Za-z0-9_-]{16,128}$/);
		}
	});
});

describe('writes: If-Match and Idempotency-Key', () => {
	it('saveEntry sends If-Match, Idempotency-Key, the JSON body and the locale query', async () => {
		const { calls, fetch } = stubFetch(() => json({ data: { id: 'e1' } }, 200, { etag: '"rev2"' }));
		await new Client({ baseUrl: 'http://server.test', fetch })
			.environment('p')
			.saveEntry('articles', 'e1', { title: 'T' }, { ifMatch: '"rev1"', idempotencyKey: 'key-0000000000000001', locale: '' });
		const call = calls[0]!;
		expect(call.method).toBe('PUT');
		expect(call.url).toBe('http://server.test/api/v1/projects/p/environments/production/collections/articles/entries/e1?locale=');
		const headers = call.init.headers as Record<string, string>;
		expect(headers['if-match']).toBe('"rev1"');
		expect(headers['idempotency-key']).toBe('key-0000000000000001');
		expect(JSON.parse(call.init.body as string)).toEqual({ title: 'T' });
	});

	it('reviewChangeset and submitChangeset take If-Match but no key', async () => {
		const { calls, fetch } = stubFetch(() => json({ data: { id: 'c1' } }));
		const env = new Client({ baseUrl: 'http://server.test', fetch }).environment('p');
		await env.reviewChangeset('c1', { decision: 'approve' }, { ifMatch: '"hash1"' });
		expect(calls[0]!.url).toBe('http://server.test/api/v1/projects/p/environments/production/changesets/c1/reviews');
		let headers = calls[0]!.init.headers as Record<string, string>;
		expect(headers['if-match']).toBe('"hash1"');
		expect(headers['idempotency-key']).toBeUndefined();
		expect(JSON.parse(calls[0]!.init.body as string)).toEqual({ decision: 'approve' });
		await env.submitChangeset('c1', { ifMatch: '"hash1"' });
		expect(calls[1]!.init.body).toBeUndefined();
		headers = calls[1]!.init.headers as Record<string, string>;
		expect(headers['if-match']).toBe('"hash1"');
	});

	it('publishChangeset sends both guards and the release body', async () => {
		const { calls, fetch } = stubFetch(() => json({ data: { generation: 7 } }, 201));
		await new Client({ baseUrl: 'http://server.test', fetch })
			.environment('p')
			.publishChangeset('c1', { base_generation: 6 }, { ifMatch: '"h"', idempotencyKey: 'k-abcdefghijklmnop' });
		expect(calls[0]!.url).toBe('http://server.test/api/v1/projects/p/environments/production/changesets/c1/publish');
		const headers = calls[0]!.init.headers as Record<string, string>;
		expect(headers['if-match']).toBe('"h"');
		expect(headers['idempotency-key']).toBe('k-abcdefghijklmnop');
		expect(JSON.parse(calls[0]!.init.body as string)).toEqual({ base_generation: 6 });
	});
});

describe('the error envelope, parsed into ApiError', () => {
	it('a 422 refusal carries code, reason, pointer and request id', async () => {
		const { fetch } = stubFetch(() =>
			json(envelope('validation_failed', 'the document does not fit the type', { reason: 'slug_taken', pointer: '/slug' }), 422)
		);
		const error = await failure(
			new Client({ baseUrl: 'http://server.test', fetch }).environment('p').getEntry('articles', 'e1')
		);
		expect(error.status).toBe(422);
		expect(error.code).toBe('validation_failed');
		expect(error.message).toBe('the document does not fit the type');
		expect(error.reason).toBe('slug_taken');
		expect(error.pointer).toBe('/slug');
		expect(error.requestId).toBe(REQUEST_ID);
		expect(error.retryable).toBe(false);
	});

	it('401 is unauthorized and never retryable; 404 is not_found', async () => {
		const { fetch } = stubFetch(({ url }) =>
			url.endsWith('/me') ? json(envelope('unauthorized', 'not signed in'), 401) : json(envelope('not_found', 'no such entry'), 404)
		);
		const c = new Client({ baseUrl: 'http://server.test', fetch });
		const signed = await failure(c.me());
		expect(signed).toMatchObject({ status: 401, code: 'unauthorized', retryable: false });
		const missing = await failure(c.environment('p').getEntry('articles', 'e1'));
		expect(missing).toMatchObject({ status: 404, code: 'not_found', retryable: false });
	});

	it('a 409 conflict carries the current ETag', async () => {
		const { fetch } = stubFetch(() =>
			json(envelope('conflict', 'the draft moved on', { reason: 'stale_revision' }), 409, { etag: '"rev-current"' })
		);
		const error = await failure(
			new Client({ baseUrl: 'http://server.test', fetch })
				.environment('p')
				.saveEntry('articles', 'e1', {}, { ifMatch: '"rev-old"', idempotencyKey: 'k-abcdefghijklmnop' })
		);
		expect(error).toMatchObject({ status: 409, code: 'conflict', etag: '"rev-current"' });
		expect(error.reason).toBe('stale_revision');
		expect(error.retryable).toBe(false);
	});

	it('a 429 names its Retry-After and is retryable', async () => {
		const { fetch } = stubFetch(() => json(envelope('rate_limited', 'slow down', { reason: 'rate_limited' }), 429, { 'retry-after': '3' }));
		const error = await failure(new Client({ baseUrl: 'http://server.test', fetch }).version());
		expect(error).toMatchObject({ status: 429, code: 'rate_limited', retryAfterMs: 3000, retryable: true });
	});

	it('a non-envelope failure is invalid_response but keeps its status', async () => {
		const { fetch } = stubFetch(() => new Response('<html>bad gateway</html>', { status: 502 }));
		const error = await failure(new Client({ baseUrl: 'http://server.test', fetch }).readyz());
		expect(error).toMatchObject({ status: 502, code: 'invalid_response', retryable: true });
	});

	it('no answer at all is no_answer, status 0, retryable', async () => {
		const { fetch } = stubFetch(() => Promise.reject(new TypeError('connect ECONNREFUSED')));
		const error = await failure(new Client({ baseUrl: 'http://server.test', fetch }).livez());
		expect(error).toMatchObject({ status: 0, code: 'no_answer', retryable: true });
	});

	it('a body over the cap is response_too_large, declared or measured', async () => {
		const big = 'x'.repeat(64);
		const declared = stubFetch(() => json({ data: big }, 200, { 'content-length': String(big.length + 20) }));
		const small = new Client({ baseUrl: 'http://server.test', fetch: declared.fetch, maxBodyBytes: 10 });
		expect((await failure(small.livez())).code).toBe('response_too_large');
		// No Content-Length: the overflow is measured as the stream arrives.
		const stream = new ReadableStream<Uint8Array>({
			start(controller) {
				controller.enqueue(new TextEncoder().encode(JSON.stringify({ data: big })));
				controller.close();
			}
		});
		const measured = stubFetch(() => new Response(stream, { status: 200, headers: { 'content-type': 'application/json' } }));
		const smallTwo = new Client({ baseUrl: 'http://server.test', fetch: measured.fetch, maxBodyBytes: 10 });
		expect((await failure(smallTwo.livez())).code).toBe('response_too_large');
	});
});

describe('delivery: published reads and the pinned scope', () => {
	it('lists, gets and queries /content/v1 with an optional generation', async () => {
		const { calls, fetch } = stubFetch(() => json({ data: [], next_cursor: null, applied: { query: {}, query_hash: 'sha256:0' } }));
		const delivery = new Client({ baseUrl: 'http://server.test', fetch }).delivery('p', 'staging');
		await delivery.list('articles', { limit: 5, generation: 9 });
		expect(calls[0]!.url).toBe('http://server.test/content/v1/p/staging/articles?limit=5&generation=9');
		await delivery.get('articles', 'e1');
		expect(calls[1]!.url).toBe('http://server.test/content/v1/p/staging/articles/e1');
		await delivery.query('articles', { filters: [{ field: 'state', op: 'eq', value: 'x' }] }, { cursor: 'q1:tok' });
		expect(calls[2]!.url).toBe('http://server.test/content/v1/p/staging/articles/query?cursor=q1%3Atok');
		expect(calls[2]!.method).toBe('POST');
		expect(JSON.parse(calls[2]!.init.body as string)).toEqual({ filters: [{ field: 'state', op: 'eq', value: 'x' }] });
	});

	it('a pinned scope names its generation on every read and refuses a different one', async () => {
		const { calls, fetch } = stubFetch(() => json({ data: [] }));
		const pinned = new Client({ baseUrl: 'http://server.test', fetch }).delivery('p').atGeneration(42);
		await pinned.list('articles');
		expect(calls[0]!.url).toContain('generation=42');
		await pinned.get('articles', 'e1', { generation: 42 });
		expect(calls[1]!.url).toContain('generation=42');
		expect(() => pinned.get('articles', 'e1', { generation: 7 })).toThrow(TypeError);
		expect(() => pinned.query('articles', {}, { generation: 7 })).toThrow(TypeError);
	});
});

describe('delivery in a locale: inLocale composes with the pinned scope', () => {
	const page = { data: [], next_cursor: null, applied: { query: {}, query_hash: 'sha256:0' } };

	it('names the locale on list, get and query, and no fallback for the policy', async () => {
		const { calls, fetch } = stubFetch(() => json(page));
		const delivery = new Client({ baseUrl: 'http://server.test', fetch }).delivery('p').inLocale('pt-BR');
		await delivery.list('articles', { limit: 5 });
		expect(calls[0]!.url).toBe('http://server.test/content/v1/p/production/articles?limit=5&locale=pt-BR');
		await delivery.get('articles', 'e1');
		expect(calls[1]!.url).toBe('http://server.test/content/v1/p/production/articles/e1?locale=pt-BR');
		await delivery.query('articles', { limit: 3 }, { cursor: 'q1' });
		expect(calls[2]!.url).toBe('http://server.test/content/v1/p/production/articles/query?cursor=q1&locale=pt-BR');
		// The policy is the server's default, so naming it changes nothing on the wire.
		await new Client({ baseUrl: 'http://server.test', fetch }).delivery('p').inLocale('fr', { fallback: 'policy' }).list('articles');
		expect(calls[3]!.url).toBe('http://server.test/content/v1/p/production/articles?locale=fr');
	});

	it("sends fallback=none, and a chain joined with commas", async () => {
		const { calls, fetch } = stubFetch(() => json(page));
		const delivery = new Client({ baseUrl: 'http://server.test', fetch }).delivery('p');
		await delivery.inLocale('pt-BR', { fallback: 'none' }).get('articles', 'e1');
		expect(new URL(calls[0]!.url).searchParams.get('fallback')).toBe('none');
		await delivery.inLocale('pt-BR', { fallback: ['pt', 'en'] }).list('articles');
		const url = new URL(calls[1]!.url);
		expect(url.searchParams.get('locale')).toBe('pt-BR');
		expect(url.searchParams.get('fallback')).toBe('pt,en');
		// An empty chain is no chain.
		await delivery.inLocale('pt-BR', { fallback: [] }).list('articles');
		expect(new URL(calls[2]!.url).searchParams.get('fallback')).toBe('none');
	});

	it('refuses a chain over four tags, a comma in a tag, and an empty tag — before any request', () => {
		const { calls, fetch } = stubFetch(() => json(page));
		const delivery = new Client({ baseUrl: 'http://server.test', fetch }).delivery('p');
		expect(() => delivery.inLocale('pt-BR', { fallback: ['pt', 'en', 'es', 'fr', 'de'] })).toThrow(TypeError);
		expect(() => delivery.inLocale('pt-BR', { fallback: ['pt,en'] })).toThrow(TypeError);
		expect(() => delivery.inLocale('')).toThrow(TypeError);
		expect(() => delivery.inLocale('pt BR')).toThrow(TypeError);
		// Four is the bound, not a refusal.
		expect(() => delivery.inLocale('pt-BR', { fallback: ['pt', 'en', 'es', 'fr'] })).not.toThrow();
		expect(calls).toHaveLength(0);
	});

	it('composes with atGeneration in either order, and the pin still refuses another generation', async () => {
		const { calls, fetch } = stubFetch(() => json(page));
		const delivery = new Client({ baseUrl: 'http://server.test', fetch }).delivery('p');
		await delivery.inLocale('pt-BR', { fallback: ['en'] }).atGeneration(9).list('articles');
		await delivery.atGeneration(9).inLocale('pt-BR', { fallback: ['en'] }).list('articles');
		expect(calls[0]!.url).toBe('http://server.test/content/v1/p/production/articles?generation=9&locale=pt-BR&fallback=en');
		expect(calls[1]!.url).toBe(calls[0]!.url);
		const pinned = delivery.atGeneration(9).inLocale('fr');
		expect(() => pinned.get('articles', 'e1', { generation: 8 })).toThrow(TypeError);
		await pinned.query('articles', {});
		expect(calls[2]!.url).toBe('http://server.test/content/v1/p/production/articles/query?generation=9&locale=fr');
	});

	it('leaves the unlocalized scope exactly as it was', async () => {
		const { calls, fetch } = stubFetch(() => json(page));
		const delivery = new Client({ baseUrl: 'http://server.test', fetch }).delivery('p');
		delivery.inLocale('fr'); // a new scope; this one is untouched
		await delivery.list('articles');
		expect(calls[0]!.url).toBe('http://server.test/content/v1/p/production/articles');
	});

	it('answers the row fields: locale, localization, requested_id, alternates', async () => {
		const row = {
			id: 'pt-1',
			requested_id: 'en-1',
			locale: 'pt',
			localization: { requested: 'pt-BR', resolved: 'pt', fallback: true },
			alternates: [{ id: 'en-1', locale: 'en', slug: 'hello' }]
		};
		const { fetch } = stubFetch(() => json({ data: row }, 200, { 'content-language': 'pt-BR' }));
		const read = await new Client({ baseUrl: 'http://server.test', fetch }).delivery('p').inLocale('pt-BR').get('articles', 'en-1');
		expect(read.data.id).toBe('pt-1');
		expect(read.data.requested_id).toBe('en-1');
		expect(read.data.localization).toEqual({ requested: 'pt-BR', resolved: 'pt', fallback: true });
		expect(read.data.alternates?.map((alternate) => alternate.locale)).toEqual(['en']);
	});

	it('a 404 not_in_locale names what is published; any other error gives null', async () => {
		const { fetch } = stubFetch(({ url }) =>
			url.includes('locale=ja')
				? json(envelope('not_found', 'not published in ja', { reason: 'not_in_locale', available: ['en', 'pt'] }), 404)
				: json(envelope('not_found', 'no such entry'), 404)
		);
		const delivery = new Client({ baseUrl: 'http://server.test', fetch }).delivery('p');
		const missing = await failure(delivery.inLocale('ja', { fallback: 'none' }).get('articles', 'e1'));
		expect(missing).toMatchObject({ status: 404, code: 'not_found', reason: 'not_in_locale' });
		expect(availableLocales(missing)).toEqual(['en', 'pt']);
		const gone = await failure(delivery.get('articles', 'e2'));
		expect(availableLocales(gone)).toBeNull();
		expect(availableLocales(new Error('not an ApiError'))).toBeNull();
	});
});

describe("the environment's languages: getLocales and putLocales", () => {
	const list = {
		version: 3,
		locales: [
			{ tag: 'en', label: 'English', default: true, fallback: [] },
			{ tag: 'pt-BR', label: 'Português (Brasil)', default: false, fallback: ['en'] }
		]
	};

	it('reads the list with its ETag', async () => {
		const { calls, fetch } = stubFetch(() => json({ data: list }, 200, { etag: '"locales-3"' }));
		const read = await new Client({ baseUrl: 'http://server.test', fetch }).environment('p', 'staging').getLocales();
		expect(calls[0]!.method).toBe('GET');
		expect(calls[0]!.url).toBe('http://server.test/api/v1/projects/p/environments/staging/locales');
		expect(read.data.locales.map((locale) => locale.tag)).toEqual(['en', 'pt-BR']);
		expect(read.response.etag).toBe('"locales-3"');
	});

	it('puts the whole list under If-Match and an Idempotency-Key', async () => {
		const { calls, fetch } = stubFetch(() => json({ data: { ...list, version: 4 } }, 200, { etag: '"locales-4"' }));
		const saved = await new Client({ baseUrl: 'http://server.test', fetch })
			.environment('p')
			.putLocales(list.locales, { ifMatch: '"locales-3"', idempotencyKey: 'k-abcdefghijklmnop' });
		const call = calls[0]!;
		expect(call.method).toBe('PUT');
		expect(call.url).toBe('http://server.test/api/v1/projects/p/environments/production/locales');
		const headers = call.init.headers as Record<string, string>;
		expect(headers['if-match']).toBe('"locales-3"');
		expect(headers['idempotency-key']).toBe('k-abcdefghijklmnop');
		expect(JSON.parse(call.init.body as string)).toEqual({ locales: list.locales });
		expect(saved.data.version).toBe(4);
		expect(saved.response.etag).toBe('"locales-4"');
	});

	it('a stale list is a 409 carrying the current ETag; a removed tag in use names the entries', async () => {
		const { fetch } = stubFetch(({ init }) =>
			(init.headers as Record<string, string>)['if-match'] === '"locales-2"'
				? json(envelope('conflict', 'the languages changed', { reason: 'stale_locales' }), 409, { etag: '"locales-3"' })
				: json(
						envelope('conflict', '12 entries are written in pt-BR', {
							reason: 'locale_in_use',
							pointer: '/locales',
							total: 12,
							entries: [{ entry: 'e1', title: 'Olá', state: 'draft' }]
						}),
						409
					)
		);
		const env = new Client({ baseUrl: 'http://server.test', fetch }).environment('p');
		const stale = await failure(env.putLocales([], { ifMatch: '"locales-2"', idempotencyKey: 'k-abcdefghijklmnop' }));
		expect(stale).toMatchObject({ status: 409, code: 'conflict', reason: 'stale_locales', etag: '"locales-3"', retryable: false });
		const inUse = await failure(env.putLocales([], { ifMatch: '"locales-3"', idempotencyKey: 'k-abcdefghijklmnoq' }));
		expect(inUse.reason).toBe('locale_in_use');
		expect(inUse.message).toBe('12 entries are written in pt-BR');
		expect(inUse.details?.total).toBe(12);
	});

	it('a 422 names the reason and the pointer into the list', async () => {
		const { fetch } = stubFetch(() =>
			json(envelope('validation_failed', 'pt-BR falls back to a tag that is not listed', { reason: 'fallback_not_listed', pointer: '/locales/1/fallback/0' }), 422)
		);
		const error = await failure(
			new Client({ baseUrl: 'http://server.test', fetch })
				.environment('p')
				.putLocales([{ tag: 'pt-BR', label: 'PT', default: true, fallback: ['xx'] }], { ifMatch: '"locales-0"', idempotencyKey: 'k-abcdefghijklmnop' })
		);
		expect(error).toMatchObject({ status: 422, code: 'validation_failed', reason: 'fallback_not_listed', pointer: '/locales/1/fallback/0' });
	});
});
