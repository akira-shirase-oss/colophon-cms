// Rate limits (D-076) in the Studio's one fetch layer: a 429 `rate_limited` says how long to wait
// (`Retry-After`); a read waits that long and goes once more; a write isn't sent again here (the
// autosave does it, with the same Idempotency-Key, after the same wait); and the words are plain.

import { expect, test } from '@playwright/test';
import { ApiError, getProject, retryAfterMs, saveEntry } from '../src/lib/api.ts';
import { describe } from '../src/lib/errors.ts';
import { busyFor, INTERNAL_TERMS } from '../src/lib/words.ts';

type Call = { method: string; url: string; headers: Record<string, string> };

/** Answers each request with the next of `answers`, recording what was sent. */
function serve(answers: (() => Response)[]): Call[] {
	const calls: Call[] = [];
	globalThis.fetch = (async (url: string, init: RequestInit = {}) => {
		calls.push({ method: init.method ?? 'GET', url, headers: { ...(init.headers as Record<string, string>) } });
		const next = answers.shift();
		if (next === undefined) throw new Error('no more answers');
		return next();
	}) as typeof fetch;
	return calls;
}

const limited = (seconds: string) => () =>
	new Response(JSON.stringify({ error: { code: 'rate_limited', message: 'too many requests', details: { reason: 'rate_limited' } } }), {
		status: 429,
		headers: { 'Content-Type': 'application/json', 'Retry-After': seconds }
	});
const project = () => new Response(JSON.stringify({ data: { id: 'p1', slug: 'site', name: 'Site', role: 'owner', permissions: [] } }), { status: 200 });

test.beforeAll(() => {
	// Writes echo the CSRF cookie; there is none outside a browser.
	(globalThis as { document?: unknown }).document = { cookie: '' };
});

test('Retry-After is read as seconds or a date, a second when it says neither, never over a minute', () => {
	expect(retryAfterMs('2')).toBe(2000);
	expect(retryAfterMs('0')).toBe(0);
	expect(retryAfterMs(null)).toBe(1000);
	expect(retryAfterMs('soon')).toBe(1000);
	expect(retryAfterMs('3600')).toBe(60_000);
	const now = Date.parse('2026-10-28T12:00:00Z');
	expect(retryAfterMs('Wed, 28 Oct 2026 12:00:05 GMT', now)).toBe(5000);
	expect(retryAfterMs('Wed, 28 Oct 2026 11:59:00 GMT', now)).toBe(0);
});

test('a read turned away waits as asked and goes once more', async () => {
	const calls = serve([limited('0'), project]);
	expect((await getProject('p1')).name).toBe('Site');
	expect(calls.map((call) => call.method)).toEqual(['GET', 'GET']);
});

test('a read turned away twice fails with the wait and plain words, after one retry only', async () => {
	const calls = serve([limited('0'), limited('2')]);
	const failure = await getProject('p1').catch((error: unknown) => error);
	expect(calls).toHaveLength(2);
	expect(failure).toBeInstanceOf(ApiError);
	expect(failure).toMatchObject({ failure: 'retry', status: 429, reason: 'rate_limited', retryAfter: 2000 });
	expect((failure as ApiError).message).toBe('Colophon is busy for a moment; try again in 2 seconds.');
	expect(describe(failure)).toMatchObject({ title: 'Colophon is busy', text: 'Colophon is busy for a moment; try again in 2 seconds.' });
});

test('a write turned away is not sent again here; its key goes with it unchanged', async () => {
	const calls = serve([limited('1')]);
	const failure = await saveEntry('p1', 'articles', 'e1', { title: 'Hello' }, '"r1"', 'key-1').catch((error: unknown) => error);
	expect(calls).toHaveLength(1);
	expect(calls[0]?.headers['Idempotency-Key']).toBe('key-1');
	expect(failure).toMatchObject({ failure: 'retry', status: 429, reason: 'rate_limited', retryAfter: 1000 });
});

test('another 429 (too many setup code tries) keeps its own words and isn’t retried', async () => {
	const calls = serve([
		() =>
			new Response(JSON.stringify({ error: { code: 'too_many_attempts', message: 'too many attempts; wait 60 seconds', details: { reason: 'too_many_attempts' } } }), {
				status: 429,
				headers: { 'Retry-After': '60' }
			})
	]);
	const failure = await getProject('p1').catch((error: unknown) => error);
	expect(calls).toHaveLength(1);
	expect(failure).toMatchObject({ status: 429, reason: 'too_many_attempts', message: 'too many attempts; wait 60 seconds' });
});

test('the words are plain', () => {
	expect(busyFor(1, true)).toBe('Colophon is busy for a moment; trying again in 1 second.');
	expect(busyFor(2.2, true)).toBe('Colophon is busy for a moment; trying again in 3 seconds.');
	expect(busyFor(0, false)).toBe('Colophon is busy for a moment; try again in 1 second.');
	for (const words of [busyFor(1, true), busyFor(5, false)]) expect(INTERNAL_TERMS.test(words), words).toBe(false);
});
