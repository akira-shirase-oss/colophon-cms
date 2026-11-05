// Live delivery updates (D-139): one upstream server-sent-events connection from this site's
// server to Colophon, fanned out to every browser watching it. Visitors never connect to the
// CMS directly, so ten thousand readers cost the CMS one stream, not ten thousand.
//
// The upstream stream says only that published content changed. What actually moved is read
// from the delivery API's `Colophon-Generation` header — the authoritative number — and when it
// is newer than what this process last saw, every subscriber is told, which reloads the page
// and renders the new release. The reader is a plain fetch loop rather than EventSource: the
// CMS's stream is newline-framed SSE and this keeps reconnection, timeouts and shutdown in our
// hands instead of a runtime global's.

import { type Site } from '@colophon/site-kit';

/** How long to wait before reconnecting after the stream ends or fails. */
const RETRY_MS = 2_000;
/** Concurrent browser streams this site server will fan out to (invariant 4: bounded). */
const MAX_CLIENTS = 500;

let generation = 0;
let started = false;
let stopped = false;
const listeners = new Set<(generation: number) => void>();

/** The release this process last heard about (0 until the first read answers). */
export function currentGeneration(): number {
	return generation;
}

/** Whether this server can carry one more browser stream. */
export function room(): boolean {
	return listeners.size < MAX_CLIENTS;
}

/** Adds a listener; `null` when this server already carries `MAX_CLIENTS` of them. */
export function subscribe(listener: (generation: number) => void): (() => void) | null {
	if (listeners.size >= MAX_CLIENTS) return null;
	listeners.add(listener);
	return () => {
		listeners.delete(listener);
	};
}

/** Starts the single upstream connection for this process; safe to call repeatedly. */
export function start(cms: Site, collection: string): void {
	if (started) return;
	started = true;
	void refresh(cms, collection);
	void connect(cms, collection);
}

/** Stops the upstream connection (tests and graceful shutdown). */
export function stop(): void {
	stopped = true;
}

async function connect(cms: Site, collection: string): Promise<void> {
	while (!stopped) {
		try {
			await listen(cms, collection);
		} catch (error) {
			console.log(JSON.stringify({ message: 'delivery events stream ended; retrying', error: String(error) }));
		}
		await new Promise((resolve) => setTimeout(resolve, RETRY_MS));
	}
}

/** Reads the stream until it ends, asking for a refresh on every `change` frame. */
async function listen(cms: Site, collection: string): Promise<void> {
	const response = await fetch(`${cms.api}/content/v1/${cms.project}/${cms.environment}/events`, {
		headers: { accept: 'text/event-stream' },
		signal: AbortSignal.timeout(3_600_000)
	});
	if (!response.ok || response.body === null) throw new Error(`events: HTTP ${response.status}`);
	const reader = response.body.getReader();
	const decoder = new TextDecoder();
	let buffer = '';
	for (;;) {
		const { done, value } = await reader.read();
		if (done || stopped) return;
		buffer += decoder.decode(value, { stream: true });
		let boundary = buffer.indexOf('\n\n');
		while (boundary >= 0) {
			const frame = buffer.slice(0, boundary);
			buffer = buffer.slice(boundary + 2);
			if (frame.includes('event: change')) void refresh(cms, collection);
			boundary = buffer.indexOf('\n\n');
		}
		// A malformed peer must not grow this buffer without bound.
		if (buffer.length > 64 * 1024) buffer = '';
	}
}

/** Reads the live generation and tells the listeners when it moved forward. */
async function refresh(cms: Site, collection: string): Promise<void> {
	try {
		const response = await fetch(`${cms.api}/content/v1/${cms.project}/${cms.environment}/${collection}`, {
			method: 'HEAD',
			signal: AbortSignal.timeout(5_000)
		});
		const next = Number(response.headers.get('colophon-generation'));
		if (!Number.isSafeInteger(next) || next < 1 || next <= generation) return;
		generation = next;
		for (const listener of listeners) listener(next);
	} catch {
		// A failed read is not a state change; the next hint or reconnect tries again.
	}
}
