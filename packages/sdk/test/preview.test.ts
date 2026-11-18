// The preview helpers' contract, against a stub `fetch` and a stub `window`: the handler never
// redirects off the site, its cookie carries exactly the documented attributes and lifetime,
// every answer is `no-store`/`noindex`, the reads carry the bearer token, failures keep their
// `details.reason`, and the browser helper talks to exactly one Studio origin.

import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	ApiError,
	createPreviewHandler,
	exitPreviewHandler,
	listenForDraftUpdates,
	previewClient,
	previewHeaders,
	previewTokenFromRequest,
	safePreviewPath
} from '../src/index.ts';

interface Seen {
	url: string;
	method: string;
	headers: Record<string, string>;
	body: string | undefined;
	init: RequestInit;
}

function stubFetch(handler: (call: Seen) => Response | Promise<Response>) {
	const calls: Seen[] = [];
	const impl = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
		const seen: Seen = {
			url: String(input),
			method: init?.method ?? 'GET',
			headers: (init?.headers ?? {}) as Record<string, string>,
			body: typeof init?.body === 'string' ? init.body : undefined,
			init: init ?? {}
		};
		calls.push(seen);
		return handler(seen);
	};
	return { calls, fetch: impl as typeof fetch };
}

function json(body: unknown, status = 200, headers: Record<string, string> = {}): Response {
	return new Response(JSON.stringify(body), { status, headers: { 'content-type': 'application/json', ...headers } });
}

const REQUEST_ID = '0190a4b0-0000-7000-8000-000000000002';

function envelope(status: number, code: string, reason: string, headers: Record<string, string> = {}): Response {
	return json({ error: { code, message: `refused: ${reason}`, request_id: REQUEST_ID, details: { reason } } }, status, headers);
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

const NOW = Date.parse('2026-11-17T12:00:00Z');
const IN_15_MIN = '2026-11-17T12:15:00Z';
const SCOPE = { kind: 'entry', entry: '0190a4b0-0000-7000-8000-0000000000e1', collection: 'articles' };
const SITE = 'https://site.example';
const CMS = 'https://cms.example';

function exchanged(expiresAt = IN_15_MIN) {
	return stubFetch(() => json({ data: { token: 'cpv_tok3n', expires_at: expiresAt, scope: SCOPE } }));
}

function handlerWith(fetch: typeof globalThis.fetch, extra: Partial<Parameters<typeof createPreviewHandler>[0]> = {}) {
	return createPreviewHandler({ baseUrl: CMS, project: 'blog', fetch, now: () => NOW, ...extra });
}

function previewRequest(query: string, init: RequestInit = {}): Request {
	return new Request(`${SITE}/api/preview?${query}`, init);
}

function expectGuarded(response: Response) {
	expect(response.headers.get('cache-control')).toBe('no-store');
	expect(response.headers.get('x-robots-tag')).toBe('noindex');
	expect(response.headers.get('referrer-policy')).toBe('no-referrer');
}

/* ------------------------------------------------------------ open redirects */

describe('safePreviewPath: a same-site relative path, or nothing', () => {
	it.each([
		['/blog/hello', '/blog/hello'],
		['/blog/hello?x=1#part', '/blog/hello?x=1#part'],
		['/', '/'],
		['/café', '/caf%C3%A9'],
		['/a/../b', '/b'],
		['/a/./b', '/a/b']
	])('keeps %j as %j', (path, safe) => {
		expect(safePreviewPath(path)).toBe(safe);
	});

	it.each([
		['protocol-relative', '//evil.example/x'],
		['absolute URL', 'https://evil.example/x'],
		['scheme', 'javascript:alert(1)'],
		['no leading slash', 'evil.example'],
		['backslash first', '\\evil.example'],
		['slash-backslash', '/\\evil.example'],
		['backslash later', '/a\\b'],
		['dot segment to //', '/..//evil.example'],
		['dot segment, single', '/.//evil.example'],
		['encoded dots to //', '/%2e%2e//evil.example'],
		['tab', '/\t/evil.example'],
		['newline', '/a\nSet-Cookie: x=1'],
		['carriage return', '/a\rb'],
		['NUL', '/a\u0000b'],
		['DEL', '/a\u007fb'],
		['empty', ''],
		['too long', `/${'a'.repeat(2048)}`]
	])('refuses %s', (_why, path) => {
		expect(safePreviewPath(path)).toBeNull();
	});

	it('takes up to 2,048 characters', () => {
		expect(safePreviewPath(`/${'a'.repeat(2047)}`)).toHaveLength(2048);
	});

	it('refuses a missing path', () => {
		expect(safePreviewPath(null)).toBeNull();
		expect(safePreviewPath(undefined)).toBeNull();
	});

	it('asks isAllowedPath about the resolved path, so dot segments cannot sneak past a prefix', () => {
		const seen: string[] = [];
		const blogOnly = (path: string) => {
			seen.push(path);
			return path.startsWith('/blog/');
		};
		expect(safePreviewPath('/blog/post', blogOnly)).toBe('/blog/post');
		expect(safePreviewPath('/blog/../admin', blogOnly)).toBeNull();
		expect(seen).toEqual(['/blog/post', '/admin']);
	});
});

/* ----------------------------------------------------------- the handler */

describe('createPreviewHandler', () => {
	it('exchanges the code server-side and answers 303 with the documented cookie', async () => {
		const { calls, fetch } = exchanged();
		const response = await handlerWith(fetch)(previewRequest('code=cpc_abc&path=%2Fblog%2Fhello'));

		expect(calls).toHaveLength(1);
		expect(calls[0]!.method).toBe('POST');
		expect(calls[0]!.url).toBe(`${CMS}/preview/v1/blog/production/exchange`);
		expect(JSON.parse(calls[0]!.body ?? '')).toEqual({ code: 'cpc_abc' });
		expect(calls[0]!.headers['authorization']).toBeUndefined();

		expect(response.status).toBe(303);
		expect(response.headers.get('location')).toBe('/blog/hello');
		expect(response.headers.get('set-cookie')).toBe(
			'colophon_preview=cpv_tok3n; HttpOnly; Secure; SameSite=None; Partitioned; Path=/; Max-Age=900'
		);
		expectGuarded(response);
	});

	it.each([
		['missing', 'code=cpc_abc'],
		['protocol-relative', 'code=cpc_abc&path=%2F%2Fevil.example'],
		['absolute', 'code=cpc_abc&path=https%3A%2F%2Fevil.example%2F'],
		['backslash', 'code=cpc_abc&path=%2F%5Cevil.example'],
		['dot segments', 'code=cpc_abc&path=%2F..%2F%2Fevil.example'],
		['control character', 'code=cpc_abc&path=%2Fa%0D%0ASet-Cookie%3A%20x%3D1'],
		['too long', `code=cpc_abc&path=%2F${'a'.repeat(2048)}`]
	])('sends a %s path to / (no open redirect)', async (_why, query) => {
		const { fetch } = exchanged();
		const response = await handlerWith(fetch)(previewRequest(query));
		expect(response.status).toBe(303);
		expect(response.headers.get('location')).toBe('/');
		expect(response.headers.get('set-cookie')).toContain('colophon_preview=cpv_tok3n;');
	});

	it('sends a path isAllowedPath refuses to /', async () => {
		const { fetch } = exchanged();
		const handle = handlerWith(fetch, { isAllowedPath: (path) => path.startsWith('/blog/') });
		expect((await handle(previewRequest('code=cpc_a&path=/blog/x'))).headers.get('location')).toBe('/blog/x');
		expect((await handle(previewRequest('code=cpc_a&path=/admin'))).headers.get('location')).toBe('/');
	});

	it('percent-encodes the path it redirects to', async () => {
		const { fetch } = exchanged();
		const response = await handlerWith(fetch)(previewRequest(`code=cpc_a&path=${encodeURIComponent('/café au lait')}`));
		expect(response.headers.get('location')).toBe('/caf%C3%A9%20au%20lait');
	});

	it('lets the cookie live exactly as long as the token, within 0 ..= 8 h', async () => {
		const maxAge = async (expiresAt: string) => {
			const { fetch } = exchanged(expiresAt);
			const response = await handlerWith(fetch)(previewRequest('code=cpc_a'));
			return /Max-Age=(\d+)$/.exec(response.headers.get('set-cookie') ?? '')?.[1];
		};
		expect(await maxAge('2026-11-17T12:01:00.900Z')).toBe('60');
		expect(await maxAge('2026-11-17T11:59:00Z')).toBe('0');
		expect(await maxAge('2026-11-18T12:00:00Z')).toBe(String(8 * 60 * 60));
	});

	it('names its own cookie when asked, and refuses a name that is not a cookie name', async () => {
		const { fetch } = exchanged();
		const response = await handlerWith(fetch, { cookieName: '__Host-preview' })(previewRequest('code=cpc_a'));
		expect(response.headers.get('set-cookie')).toMatch(/^__Host-preview=cpv_tok3n; /);
		expect(() => handlerWith(fetch, { cookieName: 'bad name;' })).toThrow(TypeError);
	});

	it('answers an unknown, used or expired code with a small 404 page and no cookie', async () => {
		const { fetch } = stubFetch(() => envelope(404, 'not_found', 'preview_not_found'));
		const response = await handlerWith(fetch)(previewRequest('code=cpc_used&path=/blog/x'));
		expect(response.status).toBe(404);
		expect(response.headers.get('set-cookie')).toBeNull();
		expect(response.headers.get('location')).toBeNull();
		expect(response.headers.get('content-type')).toBe('text/html; charset=utf-8');
		expectGuarded(response);
		const html = await response.text();
		expect(html).toContain('expired or was already used');
		expect(html).toContain('<meta name="robots" content="noindex">');
	});

	it('answers a missing or oversized code with the 404 page without asking Colophon', async () => {
		const { calls, fetch } = exchanged();
		expect((await handlerWith(fetch)(previewRequest('path=/x'))).status).toBe(404);
		expect((await handlerWith(fetch)(previewRequest(`code=${'c'.repeat(513)}`))).status).toBe(404);
		expect(calls).toHaveLength(0);
	});

	it('answers 502 when Colophon cannot be reached, 503 when it is at its limit — never a cookie', async () => {
		const down = handlerWith(stubFetch(() => Promise.reject(new TypeError('connect ECONNREFUSED'))).fetch);
		const broken = await down(previewRequest('code=cpc_a'));
		expect(broken.status).toBe(502);
		expect(broken.headers.get('set-cookie')).toBeNull();
		expectGuarded(broken);

		const busy = handlerWith(stubFetch(() => envelope(429, 'rate_limited', 'preview_limit', { 'retry-after': '7' })).fetch);
		const full = await busy(previewRequest('code=cpc_a'));
		expect(full.status).toBe(503);
		expect(full.headers.get('retry-after')).toBe('7');
		expect(full.headers.get('set-cookie')).toBeNull();
	});

	it('refuses anything but GET, and never spends a code on a prefetch', async () => {
		const { calls, fetch } = exchanged();
		const handle = handlerWith(fetch);
		const post = await handle(previewRequest('code=cpc_a', { method: 'POST' }));
		expect(post.status).toBe(405);
		expect(post.headers.get('allow')).toBe('GET');
		const prefetch = await handle(previewRequest('code=cpc_a', { headers: { 'sec-purpose': 'prefetch' } }));
		expect(prefetch.status).toBe(503);
		expect(calls).toHaveLength(0);
	});
});

/* ------------------------------------------------- cookie, headers and exit */

describe('previewTokenFromRequest', () => {
	const withCookie = (cookie: string) => new Request(`${SITE}/blog/x`, { headers: { cookie } });

	it('finds the token among other cookies', () => {
		expect(previewTokenFromRequest(withCookie('theme=dark; colophon_preview=cpv_abc; other=1'))).toBe('cpv_abc');
		expect(previewTokenFromRequest(withCookie('colophon_preview_old=nope; colophon_preview=cpv_abc'))).toBe('cpv_abc');
	});

	it('answers null without the cookie, or for a value that is not a token', () => {
		expect(previewTokenFromRequest(new Request(`${SITE}/`))).toBeNull();
		expect(previewTokenFromRequest(withCookie('theme=dark'))).toBeNull();
		expect(previewTokenFromRequest(withCookie('colophon_preview='))).toBeNull();
		expect(previewTokenFromRequest(withCookie('colophon_preview="quoted"'))).toBeNull();
	});

	it('reads the cookie name it is given', () => {
		expect(previewTokenFromRequest(withCookie('mine=cpv_x; colophon_preview=cpv_y'), 'mine')).toBe('cpv_x');
	});
});

describe('previewHeaders', () => {
	it('is private, unstored, unindexed and framable only by the Studio', () => {
		expect(previewHeaders({ studioOrigin: 'https://cms.example' })).toEqual({
			'Cache-Control': 'private, no-store',
			'X-Robots-Tag': 'noindex',
			'Content-Security-Policy': 'frame-ancestors https://cms.example'
		});
		expect(previewHeaders({ studioOrigin: 'http://localhost:58000/' })['Content-Security-Policy']).toBe('frame-ancestors http://localhost:58000');
	});

	it.each(['*', 'https://*.cms.example', 'https://cms.example/studio/', 'https://cms.example?x', 'cms.example', "https://cms.example 'unsafe-inline'", 'ftp://cms.example', 'https://user:pw@cms.example'])(
		'refuses %j as a Studio origin',
		(studioOrigin) => {
			expect(() => previewHeaders({ studioOrigin })).toThrow(TypeError);
		}
	);
});

describe('exitPreviewHandler', () => {
	it('clears the cookie with the attributes that set it and answers 303', () => {
		const response = exitPreviewHandler()(new Request(`${SITE}/api/exit`));
		expect(response.status).toBe(303);
		expect(response.headers.get('location')).toBe('/');
		expect(response.headers.get('set-cookie')).toBe('colophon_preview=; HttpOnly; Secure; SameSite=None; Partitioned; Path=/; Max-Age=0');
		expectGuarded(response);
	});

	it('goes back to a checked ?path=, else to redirectTo', () => {
		const exit = exitPreviewHandler({ cookieName: 'mine', redirectTo: '/blog', isAllowedPath: (path) => path.startsWith('/blog') });
		const back = exit(new Request(`${SITE}/api/exit?path=%2Fblog%2Fhello`));
		expect(back.headers.get('location')).toBe('/blog/hello');
		expect(back.headers.get('set-cookie')).toMatch(/^mine=; /);
		expect(exit(new Request(`${SITE}/api/exit?path=%2F%2Fevil.example`)).headers.get('location')).toBe('/blog');
		expect(exit(new Request(`${SITE}/api/exit?path=%2Fadmin`)).headers.get('location')).toBe('/blog');
	});

	it('refuses a redirectTo that is not a same-site path', () => {
		expect(() => exitPreviewHandler({ redirectTo: 'https://evil.example/' })).toThrow(TypeError);
	});
});

/* --------------------------------------------------------- client and reads */

describe('previewClient', () => {
	it('exchanges a code into { token, expiresAt, scope }', async () => {
		const { fetch } = exchanged();
		const result = await previewClient({ baseUrl: `${CMS}/`, project: 'blog', environment: 'staging', fetch }).exchange('cpc_a');
		expect(result).toEqual({ token: 'cpv_tok3n', expiresAt: IN_15_MIN, scope: SCOPE });
	});

	it('refuses an exchange answer that is not in the documented shape', async () => {
		const bad = [{ data: { token: 'cpv; x', expires_at: IN_15_MIN, scope: SCOPE } }, { data: { token: 'cpv_a', expires_at: 'soon', scope: SCOPE } }, { data: { token: 'cpv_a', expires_at: IN_15_MIN, scope: { kind: 'site' } } }, {}];
		for (const body of bad) {
			const { fetch } = stubFetch(() => json(body));
			const error = await failure(previewClient({ baseUrl: CMS, project: 'blog', fetch }).exchange('cpc_a'));
			expect(error.code).toBe('invalid_response');
		}
	});

	it('reads with Authorization: Bearer and nothing else', async () => {
		const row = { id: 'e1', title: 'Draft', content: { title: 'Draft' }, preview: { source: 'draft' } };
		const { calls, fetch } = stubFetch((call) => {
			if (call.url.endsWith('/session')) return json({ data: { scope: SCOPE, expires_at: IN_15_MIN } });
			if (call.method === 'POST') return json({ data: [row], next_cursor: null, applied: { query_hash: 'h' } });
			if (call.url.includes('/articles/')) return json({ data: row }, 200, { 'colophon-preview': 'entry', 'colophon-generation': '12' });
			return json({ data: [row], next_cursor: 'c2' });
		});
		const reader = previewClient({ baseUrl: CMS, project: 'my blog', fetch }).reader('cpv_tok3n');

		const page = await reader.list('articles', { limit: 5, cursor: 'c1' });
		expect(page.data[0]!.preview.source).toBe('draft');
		expect(page.next_cursor).toBe('c2');

		const one = await reader.get('articles', 'e/1');
		expect(one.data.title).toBe('Draft');
		expect(one.response).toEqual({ status: 200, requestId: null, preview: 'entry', generation: 12 });

		const queried = await reader.query('articles', { filter: { title: { eq: 'Draft' } } }, { cursor: 'q1' });
		expect(queried.applied).toEqual({ query_hash: 'h' });

		expect(await reader.session()).toEqual({ scope: SCOPE, expiresAt: IN_15_MIN });

		expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
			`GET ${CMS}/preview/v1/my%20blog/production/articles?limit=5&cursor=c1`,
			`GET ${CMS}/preview/v1/my%20blog/production/articles/e%2F1`,
			`POST ${CMS}/preview/v1/my%20blog/production/articles/query?cursor=q1`,
			`GET ${CMS}/preview/v1/my%20blog/production/session`
		]);
		for (const call of calls) {
			expect(call.headers['authorization']).toBe('Bearer cpv_tok3n');
			expect(call.headers['cookie']).toBeUndefined();
			expect(call.init.redirect).toBe('manual');
			expect(call.init.cache).toBe('no-store');
		}
		expect(JSON.parse(calls[2]!.body ?? '')).toEqual({ filter: { title: { eq: 'Draft' } } });
	});

	it('refuses a token that could break a header', () => {
		const client = previewClient({ baseUrl: CMS, project: 'blog', fetch: exchanged().fetch });
		expect(() => client.reader('cpv_a\r\nX-Evil: 1')).toThrow(TypeError);
		expect(() => client.reader('')).toThrow(TypeError);
	});

	it.each([
		[401, 'unauthorized', 'preview_expired'],
		[401, 'unauthorized', 'preview_revoked'],
		[401, 'unauthorized', 'preview_invalid'],
		[409, 'conflict', 'preview_stale'],
		[410, 'gone', 'preview_published'],
		[422, 'validation_failed', 'preview_reads_latest']
	])('keeps a %i %s refusal reason (%s)', async (status, code, reason) => {
		const { fetch } = stubFetch(() => envelope(status, code, reason, { 'x-request-id': REQUEST_ID }));
		const error = await failure(previewClient({ baseUrl: CMS, project: 'blog', fetch }).reader('cpv_a').get('articles', 'e1'));
		expect(error.status).toBe(status);
		expect(error.code).toBe(code);
		expect(error.reason).toBe(reason);
		expect(error.requestId).toBe(REQUEST_ID);
		expect(error.retryable).toBe(false);
	});

	it('reads a full preview capacity as retryable, with its wait', async () => {
		const { fetch } = stubFetch(() => envelope(503, 'unavailable', 'preview_capacity', { 'retry-after': '2' }));
		const error = await failure(previewClient({ baseUrl: CMS, project: 'blog', fetch }).reader('cpv_a').list('articles'));
		expect(error.status).toBe(503);
		expect(error.retryable).toBe(true);
		expect(error.retryAfterMs).toBe(2000);
	});

	it('reads no answer, a non-JSON body and a redirect as client-side codes', async () => {
		const reader = (fetch: typeof globalThis.fetch) => previewClient({ baseUrl: CMS, project: 'blog', fetch }).reader('cpv_a');
		const none = await failure(reader(stubFetch(() => Promise.reject(new TypeError('fetch failed'))).fetch).session());
		expect([none.status, none.code]).toEqual([0, 'no_answer']);
		const html = await failure(reader(stubFetch(() => new Response('<html>', { status: 502 })).fetch).session());
		expect([html.status, html.code]).toEqual([502, 'invalid_response']);
		const moved = await failure(reader(stubFetch(() => new Response(null, { status: 302, headers: { location: 'https://evil.example/' } })).fetch).session());
		expect([moved.status, moved.code]).toEqual([302, 'invalid_response']);
		const cutOff = new ReadableStream<Uint8Array>({
			start(controller) {
				controller.enqueue(new TextEncoder().encode('{"data":'));
				controller.error(new DOMException('The operation timed out.', 'TimeoutError'));
			}
		});
		const cut = await failure(reader(stubFetch(() => new Response(cutOff, { status: 200 })).fetch).session());
		expect([cut.status, cut.code, cut.retryable]).toEqual([0, 'no_answer', true]);
	});

	it('stops reading a body past maxBodyBytes', async () => {
		const { fetch } = stubFetch(() => json({ data: { title: 'x'.repeat(100) } }));
		const error = await failure(previewClient({ baseUrl: CMS, project: 'blog', fetch, maxBodyBytes: 50 }).reader('cpv_a').get('articles', 'e1'));
		expect(error.code).toBe('response_too_large');
	});

	it('refuses a baseUrl that is not http(s)', () => {
		expect(() => previewClient({ baseUrl: 'ftp://cms.example', project: 'blog' })).toThrow(TypeError);
	});
});

/* ------------------------------------------------------------ the browser */

interface FakeWindow extends EventTarget {
	parent: { postMessage: ReturnType<typeof vi.fn> } | FakeWindow;
	location: { reload: ReturnType<typeof vi.fn> };
}

function fakeWindow(framed: boolean): { win: FakeWindow; parentPost: ReturnType<typeof vi.fn> } {
	const parentPost = vi.fn();
	const target = new EventTarget() as FakeWindow;
	target.location = { reload: vi.fn() };
	target.parent = framed ? { postMessage: parentPost } : target;
	vi.stubGlobal('window', target);
	return { win: target, parentPost };
}

const STUDIO = 'https://cms.example';
const saved = { type: 'colophon:draft-saved', entry: 'e1', revision: 'r2' };

function post(win: EventTarget, data: unknown, origin: string) {
	win.dispatchEvent(new MessageEvent('message', { data, origin }));
}

describe('listenForDraftUpdates', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
	});

	it('tells the framing Studio it is ready, at exactly the Studio origin', () => {
		const { parentPost } = fakeWindow(true);
		listenForDraftUpdates({ studioOrigin: STUDIO, previewing: true });
		expect(parentPost).toHaveBeenCalledWith({ type: 'colophon:preview-ready', previewing: true }, STUDIO);
	});

	it('reports previewing:false when the page holds no session', () => {
		const { parentPost } = fakeWindow(true);
		listenForDraftUpdates({ studioOrigin: `${STUDIO}/`, previewing: false });
		expect(parentPost).toHaveBeenCalledWith({ type: 'colophon:preview-ready', previewing: false }, STUDIO);
	});

	it('posts nothing when not framed', () => {
		const { win } = fakeWindow(false);
		const spy = vi.fn();
		(win as unknown as { postMessage: typeof spy }).postMessage = spy;
		listenForDraftUpdates({ studioOrigin: STUDIO, previewing: true });
		expect(spy).not.toHaveBeenCalled();
	});

	it('acts on a draft-saved message from exactly the Studio origin, and nothing else', () => {
		const { win } = fakeWindow(true);
		const onUpdate = vi.fn();
		listenForDraftUpdates({ studioOrigin: STUDIO, previewing: true, onUpdate });
		post(win, saved, 'https://cms.example.evil.example');
		post(win, saved, 'https://evil.example');
		post(win, saved, 'http://cms.example');
		post(win, saved, 'https://cms.example:8443');
		post(win, saved, 'null');
		post(win, { type: 'colophon:preview-ready', previewing: true }, STUDIO);
		post(win, { type: 'colophon:draft-saved' }, STUDIO);
		post(win, 'colophon:draft-saved', STUDIO);
		expect(onUpdate).not.toHaveBeenCalled();
		post(win, saved, STUDIO);
		expect(onUpdate).toHaveBeenCalledExactlyOnceWith(saved);
	});

	it('reloads by default, and stops after unsubscribe', () => {
		const { win } = fakeWindow(true);
		const stop = listenForDraftUpdates({ studioOrigin: STUDIO, previewing: true });
		post(win, saved, STUDIO);
		expect(win.location.reload).toHaveBeenCalledTimes(1);
		stop();
		post(win, saved, STUDIO);
		expect(win.location.reload).toHaveBeenCalledTimes(1);
	});

	it('refuses a wildcard or non-origin target', () => {
		fakeWindow(true);
		expect(() => listenForDraftUpdates({ studioOrigin: '*', previewing: true })).toThrow(TypeError);
		expect(() => listenForDraftUpdates({ studioOrigin: 'https://cms.example/studio', previewing: true })).toThrow(TypeError);
	});

	it('does nothing outside a browser', () => {
		vi.stubGlobal('window', undefined);
		const stop = listenForDraftUpdates({ studioOrigin: STUDIO, previewing: true });
		expect(stop).toBeTypeOf('function');
		stop();
	});
});
