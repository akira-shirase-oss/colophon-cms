// F9: a project's reads wait (briefly) for its live stream, so the stream's first open has nothing to re-read.
// These run in Node against the modules themselves: the gate, the one fetch layer, and `watchProject` with a
// stand-in EventSource and query client. The browser version, with a real server, is tests/server/one-read.spec.ts.

import { expect, test } from '@playwright/test';
import { getProject } from '../src/lib/api.ts';
import { watchProject } from '../src/lib/live.ts';
import { projectOf, readsMayStart, streamClosed, streamOpened } from '../src/lib/streamGate.ts';

const tick = (ms = 0) => new Promise((resolve) => setTimeout(resolve, ms));

test('a project read waits for its stream, and the open reports nothing early', async () => {
	let done = false;
	const read = readsMayStart('p-wait').then(() => (done = true));
	await tick(50);
	expect(done, 'the read waits while the stream is not open').toBe(false);
	expect(streamOpened('p-wait')).toEqual({ early: false });
	await read;
	expect(done).toBe(true);
	// Once open, reads go at once.
	const started = Date.now();
	await readsMayStart('p-wait');
	expect(Date.now() - started).toBeLessThan(20);
	streamClosed('p-wait');
});

test('a stream that does not open in time lets reads go, and its open says some went early', async () => {
	const started = Date.now();
	await readsMayStart('p-slow', 60);
	expect(Date.now() - started).toBeGreaterThanOrEqual(55);
	expect(streamOpened('p-slow'), 'the first open must re-read').toEqual({ early: true });
	expect(streamOpened('p-slow'), 'only the first open reports it').toEqual({ early: false });
	streamClosed('p-slow');
});

test('projects are separate, and leaving a project means the next visit waits for a new stream', async () => {
	streamOpened('p-a');
	await readsMayStart('p-a');
	let other = false;
	const waiting = readsMayStart('p-b').then(() => (other = true));
	await tick(30);
	expect(other, 'another project still waits for its own stream').toBe(false);
	streamOpened('p-b');
	await waiting;
	streamClosed('p-a');
	let again = false;
	const next = readsMayStart('p-a').then(() => (again = true));
	await tick(30);
	expect(again, 'after leaving, a new visit waits again').toBe(false);
	streamOpened('p-a');
	await next;
	streamClosed('p-a');
	streamClosed('p-b');
});

test('which requests are project reads that wait', () => {
	expect(projectOf('/api/v1/projects/abc/environments/production/releases')).toBe('abc');
	expect(projectOf('/api/v1/projects/abc')).toBe(null);
	expect(projectOf('/api/v1/projects/abc/environments/production/events')).toBe(null);
	expect(projectOf('/api/v1/me')).toBe(null);
	expect(projectOf('/content/v1/abc/production/articles')).toBe(null);
});

test('the fetch layer holds a project read until the stream opens, and never holds a write', async () => {
	const sent: string[] = [];
	globalThis.fetch = (async (url: string, init: RequestInit = {}) => {
		sent.push(`${init.method ?? 'GET'} ${url}`);
		return new Response(JSON.stringify({ data: { id: 'p-fetch', slug: 's', name: 'S', role: 'owner', permissions: [] } }), { status: 200 });
	}) as typeof fetch;
	// `getProject` reads `/api/v1/projects/{id}`, which is not under a project's environment: it never waits.
	await getProject('p-fetch');
	expect(sent).toHaveLength(1);
	// A read under the project waits for the stream.
	const { send } = await import('../src/lib/api.ts');
	const pending = send('GET', '/api/v1/projects/p-fetch/environments/production/releases');
	await tick(30);
	expect(sent, 'not sent before the stream opens').toHaveLength(1);
	streamOpened('p-fetch');
	await pending;
	expect(sent).toHaveLength(2);
	streamClosed('p-fetch');
});

/** A stand-in for EventSource: records the instance so the test can fire its events. */
class FakeSource {
	static last: FakeSource | null = null;
	static OPEN = 1;
	readyState = 0;
	private handlers = new Map<string, ((event: unknown) => void)[]>();
	constructor(public url: string) {
		FakeSource.last = this;
	}
	addEventListener(type: string, handler: (event: unknown) => void) {
		this.handlers.set(type, [...(this.handlers.get(type) ?? []), handler]);
	}
	fire(type: string, event: unknown = {}) {
		if (type === 'open') this.readyState = 1;
		for (const handler of this.handlers.get(type) ?? []) handler(event);
	}
	close() {
		this.readyState = 2;
	}
}

function fakeClient() {
	const invalidated: unknown[] = [];
	return {
		invalidated,
		cancelQueries: async () => {},
		invalidateQueries: async (filters: unknown) => void invalidated.push(filters)
	};
}

function stage() {
	(globalThis as unknown as { EventSource: unknown }).EventSource = FakeSource;
	(globalThis as unknown as { document: unknown }).document = { visibilityState: 'visible', addEventListener() {}, removeEventListener() {} };
}

test('the first open re-reads nothing when every read waited for it', async () => {
	stage();
	const client = fakeClient();
	const stop = watchProject('p-live', 'production', client as never);
	const read = readsMayStart('p-live');
	FakeSource.last!.fire('open');
	await read;
	await tick();
	expect(client.invalidated, 'no catch-up: nothing was read before the stream listened').toEqual([]);
	// A reopen (the stream's lifetime ended, or the network dropped) still resyncs.
	FakeSource.last!.fire('open');
	await tick();
	expect(client.invalidated).toHaveLength(1);
	stop();
});

test('the first open re-reads when a read had to go ahead of it', async () => {
	stage();
	const client = fakeClient();
	await readsMayStart('p-late', 20); // the stream was slow: this read went early
	const stop = watchProject('p-late', 'production', client as never);
	FakeSource.last!.fire('open');
	await tick();
	expect(client.invalidated, 'the catch-up runs, as before').toHaveLength(1);
	stop();
});
