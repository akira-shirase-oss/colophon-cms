// No request waits past its deadline (D-245): a write whose answer never comes, or whose headers
// come and then whose body stalls (a flaky link to the edge), fails as "no answer" within
// REQUEST_TIMEOUT_MS, and the publish flow stops with a "try again" whose retry reuses the same
// Idempotency-Key, so it replays rather than doubles. The fetch here behaves as the platform's
// does: a request still pending, and a body still streaming, fail when the signal aborts.

import { expect, test } from '@playwright/test';
import { ApiError, REQUEST_TIMEOUT_MS, send } from '../src/lib/api.ts';
import { newKeys, publishInOneClick } from '../src/lib/publishing.ts';

type Call = { method: string; url: string; key: string | undefined };

const aborted = () => new DOMException('The operation was aborted.', 'AbortError');

/** A fetch that never answers: it fails only when its signal aborts. */
function never(calls: Call[]): typeof fetch {
	return ((url: string, init: RequestInit = {}) => {
		calls.push({ method: init.method ?? 'GET', url, key: (init.headers as Record<string, string>)['Idempotency-Key'] });
		return new Promise<Response>((_, reject) => init.signal?.addEventListener('abort', () => reject(aborted())));
	}) as typeof fetch;
}

/** A fetch whose headers come at once and whose body never finishes: the stream errors on abort. */
function stalledBody(calls: Call[]): typeof fetch {
	return (async (url: string, init: RequestInit = {}) => {
		calls.push({ method: init.method ?? 'GET', url, key: (init.headers as Record<string, string>)['Idempotency-Key'] });
		const body = new ReadableStream<Uint8Array>({
			start(controller) {
				controller.enqueue(new TextEncoder().encode('{"data":'));
				init.signal?.addEventListener('abort', () => controller.error(aborted()));
			}
		});
		return new Response(body, { status: 201, headers: { 'Content-Type': 'application/json' } });
	}) as typeof fetch;
}

/** Headers at once, then a body that ignores the abort and never ends: the deadline must still hold. */
function deafBody(calls: Call[]): typeof fetch {
	return (async (url: string, init: RequestInit = {}) => {
		calls.push({ method: init.method ?? 'GET', url, key: (init.headers as Record<string, string>)['Idempotency-Key'] });
		const body = new ReadableStream<Uint8Array>({ start: (controller) => controller.enqueue(new TextEncoder().encode('{')) });
		return new Response(body, { status: 200, headers: { 'Content-Type': 'application/json' } });
	}) as typeof fetch;
}

const realSetTimeout = globalThis.setTimeout;
test.beforeAll(() => {
	// Writes echo the CSRF cookie; there is none outside a browser.
	(globalThis as { document?: unknown }).document = { cookie: '' };
	// The deadline itself, made short so the tests take milliseconds: every other timer runs as set.
	globalThis.setTimeout = ((handler: () => void, ms?: number, ...rest: unknown[]) =>
		realSetTimeout(handler, ms === REQUEST_TIMEOUT_MS ? 50 : ms, ...rest)) as typeof setTimeout;
});
test.afterAll(() => {
	globalThis.setTimeout = realSetTimeout;
});

test('the deadline is fifteen seconds', () => {
	expect(REQUEST_TIMEOUT_MS).toBe(15_000);
});

test('a write whose answer never comes is "no answer" at the deadline', async () => {
	const calls: Call[] = [];
	globalThis.fetch = never(calls);
	const started = Date.now();
	const failure = await send('POST', '/api/v1/projects/p1/x', { a: 1 }).catch((error: unknown) => error);
	expect(failure).toBeInstanceOf(ApiError);
	expect(failure).toMatchObject({ failure: 'retry', status: null });
	expect(Date.now() - started).toBeLessThan(2_000);
});

test('headers that come with a body that stalls are "no answer" too, not a wait', async () => {
	const calls: Call[] = [];
	globalThis.fetch = stalledBody(calls);
	const started = Date.now();
	const failure = await send('POST', '/api/v1/projects/p1/x', { a: 1 }).catch((error: unknown) => error);
	expect(failure).toBeInstanceOf(ApiError);
	expect(failure).toMatchObject({ failure: 'retry', status: null });
	expect(Date.now() - started).toBeLessThan(2_000);
});

test('a body that never ends, even one deaf to the abort, still ends at the deadline', async () => {
	const calls: Call[] = [];
	globalThis.fetch = deafBody(calls);
	const started = Date.now();
	const failure = await send('POST', '/api/v1/projects/p1/x', { a: 1 }).catch((error: unknown) => error);
	expect(failure).toMatchObject({ failure: 'retry', status: null });
	expect(Date.now() - started).toBeLessThan(2_000);
});

test('a publish that gets no answer stops with "try again", and trying again sends the same key', async () => {
	const calls: Call[] = [];
	globalThis.fetch = stalledBody(calls);
	const keys = newKeys();
	const steps: string[] = [];
	const options = { project: 'p1', title: 'Hello', entries: ['e1'], oneClick: false, seenGeneration: 1, keys, step: (words: string) => steps.push(words) };
	const first = await publishInOneClick(options);
	expect(first).toMatchObject({ kind: 'stopped', noAnswer: true });
	expect(first.kind === 'stopped' && first.message).toContain('try again');
	const second = await publishInOneClick(options);
	expect(second).toMatchObject({ kind: 'stopped', noAnswer: true });
	const creates = calls.filter((call) => call.method === 'POST' && call.url.endsWith('/changesets'));
	expect(creates).toHaveLength(2);
	expect(creates[0]?.key).toBe(keys.request);
	expect(creates[1]?.key).toBe(keys.request);
});
