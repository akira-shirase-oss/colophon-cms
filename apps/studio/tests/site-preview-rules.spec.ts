// Preview on the editor's own site (D-181): the Studio's rules, without a browser. The
// management calls (paths, methods, CSRF on writes, bodies), the address template's checks (the
// server's, mirrored), the pane's state machine, the exact origin check on the site's messages,
// and the words — plain, with no internal terms outside Technical details.

import { expect, test } from '@playwright/test';
import { ApiError } from '../src/lib/api.ts';
import {
	changesetPreviewProblem,
	draftSaved,
	endsIn,
	expired,
	getPreviewTarget,
	grantOf,
	hostOf,
	issueChangesetPreview,
	issuePreview,
	listPreviews,
	type PaneState,
	type PreviewGrant,
	previewCode,
	previewProblem,
	readyFrom,
	renewDue,
	renewPreview,
	revokePreview,
	scaleFor,
	setPreviewTarget,
	step,
	templateOrigin,
	templateProblem
} from '../src/lib/preview.ts';
import { INTERNAL_TERMS } from '../src/lib/words.ts';

type Call = { method: string; url: string; headers: Record<string, string>; body: unknown };

/** Answers each request with the next of `answers`, recording what was sent. */
function serve(answers: (() => Response)[]): Call[] {
	const calls: Call[] = [];
	globalThis.fetch = (async (url: string, init: RequestInit = {}) => {
		calls.push({
			method: init.method ?? 'GET',
			url,
			headers: { ...(init.headers as Record<string, string>) },
			body: init.body === undefined ? undefined : JSON.parse(String(init.body))
		});
		const next = answers.shift();
		if (next === undefined) throw new Error('no more answers');
		return next();
	}) as typeof fetch;
	return calls;
}
const json = (status: number, body: unknown) => () => new Response(status === 204 ? null : JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
const refusal = (status: number, reason: string, message = 'refused') => json(status, { error: { code: reason, message, details: { reason } } });

const P = '/api/v1/projects/p1/environments/production';
const GRANT: PreviewGrant = {
	id: 'g1',
	code: 'cpc_one',
	code_expires_at: '2026-11-17T12:01:00Z',
	expires_at: '2026-11-17T12:15:00Z',
	url: 'https://www.example.com/api/preview?code=cpc_one&path=/blog/hello',
	origin: 'https://www.example.com',
	scope: { kind: 'entry', entry: 'e1', collection: 'articles' }
};

test.beforeAll(() => {
	(globalThis as { document?: unknown }).document = { cookie: '__Host-colophon_csrf=tok-1' };
});

test.describe('the management calls', () => {
	test('read and set the preview address; writes carry the CSRF token', async () => {
		const target = { url_template: 'https://www.example.com/p?code={code}', origin: 'https://www.example.com', updated_at: '2026-11-17T12:00:00Z' };
		const calls = serve([json(200, { data: target }), json(200, { data: target }), json(200, { data: { url_template: null, origin: null, updated_at: null } })]);
		expect(await getPreviewTarget('p1')).toEqual(target);
		expect(await setPreviewTarget('p1', target.url_template)).toEqual(target);
		expect((await setPreviewTarget('p1', null)).url_template).toBeNull();
		expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([`GET ${P}/preview-target`, `PUT ${P}/preview-target`, `PUT ${P}/preview-target`]);
		expect(calls[0]?.headers['X-CSRF-Token']).toBeUndefined();
		expect(calls[1]?.headers['X-CSRF-Token']).toBe('tok-1');
		expect(calls[1]?.body).toEqual({ url_template: target.url_template });
		expect(calls[2]?.body).toEqual({ url_template: null });
	});

	test('issue, list, renew, a fresh code and revoke go where the contract says', async () => {
		const calls = serve([
			json(201, { data: GRANT }),
			json(200, { data: [] }),
			json(200, { data: { id: 'g1', expires_at: '2026-11-17T12:20:00Z' } }),
			json(201, { data: { code: 'cpc_two', code_expires_at: 'x', url: 'https://www.example.com/api/preview?code=cpc_two' } }),
			json(204, null)
		]);
		expect(await issuePreview('p1', 'e1')).toEqual(GRANT);
		expect(await listPreviews('p1', 'e1')).toEqual([]);
		expect((await renewPreview('p1', 'g1')).expires_at).toBe('2026-11-17T12:20:00Z');
		expect((await previewCode('p1', 'g1')).code).toBe('cpc_two');
		await revokePreview('p1', 'g1');
		expect(calls.map((call) => `${call.method} ${call.url}`)).toEqual([
			`POST ${P}/previews`,
			`GET ${P}/previews?entry=e1`,
			`POST ${P}/previews/g1/renew`,
			`POST ${P}/previews/g1/code`,
			`DELETE ${P}/previews/g1`
		]);
		expect(calls[0]?.body).toEqual({ entry: 'e1' });
		for (const call of calls.filter((each) => each.method !== 'GET')) expect(call.headers['X-CSRF-Token']).toBe('tok-1');
	});

	test('a publish request’s preview names the request, and its refusals have their own words', async () => {
		const calls = serve([json(201, { data: { ...GRANT, scope: { kind: 'changeset', changeset: 'c1' } } }), refusal(409, 'already_published')]);
		expect((await issueChangesetPreview('p1', 'c1')).scope.kind).toBe('changeset');
		expect(calls[0]).toMatchObject({ method: 'POST', url: `${P}/previews`, body: { changeset: 'c1' } });
		expect(calls[0]?.headers['X-CSRF-Token']).toBe('tok-1');
		const live = await issueChangesetPreview('p1', 'c1').catch((error: unknown) => error);
		expect(live).toMatchObject({ status: 409, reason: 'already_published', failure: 'refused' });
		expect(changesetPreviewProblem(live)).toMatchObject({ title: 'This publish request is already live' });
		expect(changesetPreviewProblem(live).technical).toContain('already_published');
		expect(changesetPreviewProblem(new ApiError('not-found', 'not found', 404, null)).title).toBe('This publish request can’t be previewed');
		expect(changesetPreviewProblem(new ApiError('retry', 'too many', 429, 'preview_limit')).title).toBe('Too many previews open');
		for (const error of [live, new ApiError('retry', 'no answer from the server')]) {
			const said = changesetPreviewProblem(error);
			expect(INTERNAL_TERMS.test(`${said.title} ${said.text}`), said.text).toBe(false);
		}
	});

	test('refusals arrive as typed errors with their reasons', async () => {
		serve([refusal(422, 'preview_not_configured'), refusal(429, 'preview_limit'), refusal(404, 'not_found')]);
		await expect(issuePreview('p1', 'e1')).rejects.toMatchObject({ status: 422, reason: 'preview_not_configured', failure: 'refused' });
		await expect(issuePreview('p1', 'e1')).rejects.toMatchObject({ status: 429, reason: 'preview_limit' });
		await expect(renewPreview('p1', 'g1')).rejects.toMatchObject({ status: 404, failure: 'not-found' });
	});
});

test.describe('the preview address', () => {
	test('a good address passes, and its origin is the one previews talk to', () => {
		for (const good of [
			'https://www.example.com/api/preview?code={code}&path=/blog/{slug}',
			'https://example.com/preview/{collection}/{id}?code={code}&lang={locale}',
			'http://localhost:3000/api/preview?code={code}',
			'http://127.0.0.1:4321/preview?code={code}',
			'  https://www.example.com/p?code={code}  '
		]) {
			expect(templateProblem(good), good).toBe('');
		}
		expect(templateOrigin('https://www.example.com/api/preview?code={code}')).toBe('https://www.example.com');
		expect(templateOrigin('http://localhost:3000/p?code={code}')).toBe('http://localhost:3000');
		expect(templateOrigin('https://www.example.com/p')).toBeNull();
	});

	test('each mistake is said in words a developer can act on', () => {
		const cases: [string, RegExp][] = [
			['', /Write the address/],
			['   ', /Write the address/],
			['https://www.example.com/api/preview', /Add \{code\}/],
			['http://www.example.com/p?code={code}', /https:\/\/.*localhost/],
			['ftp://www.example.com/p?code={code}', /https:\/\//],
			['http://localhost.example.com/p?code={code}', /https:\/\//],
			['www.example.com/p?code={code}', /full web address/],
			['https://www.example.com/p?code={code}&x={title}', /\{title\} isn’t a placeholder/],
			['https://www.example.com/p?code={code}&x={code', /Curly brackets/],
			['https://{locale}.example.com/p?code={code}', /not in the site’s own address/],
			['https://user:secret@www.example.com/p?code={code}', /user name and password/],
			[`https://www.example.com/p?code={code}&pad=${'a'.repeat(2048)}`, /at most 2,048 characters/]
		];
		for (const [text, words] of cases) expect(templateProblem(text), text).toMatch(words);
		// Exactly 2,048 characters is still fine.
		const edge = 'https://www.example.com/p?code={code}&pad=';
		expect(templateProblem(edge + 'a'.repeat(2048 - edge.length))).toBe('');
	});
});

test.describe('the pane', () => {
	const closed: PaneState = { kind: 'closed' };
	const failure = (status: number, reason: string, failureKind: ConstructorParameters<typeof ApiError>[0] = 'refused') =>
		new ApiError(failureKind, 'refused', status, reason);

	test('opens, hears the site, and goes live or says the cookie was kept out', () => {
		let state = step(closed, { type: 'open' });
		expect(state).toEqual({ kind: 'opening', grant: null });
		state = step(state, { type: 'issued', grant: GRANT });
		expect(state).toEqual({ kind: 'opening', grant: GRANT });
		expect(step(state, { type: 'ready', previewing: true })).toEqual({ kind: 'live', grant: GRANT });
		expect(step(state, { type: 'ready', previewing: false })).toEqual({ kind: 'cookie-blocked', grant: GRANT });
	});

	test('ten quiet seconds say "can’t tell"; a late answer still counts', () => {
		const opening: PaneState = { kind: 'opening', grant: GRANT };
		const quiet = step(opening, { type: 'quiet' });
		expect(quiet).toEqual({ kind: 'unconfirmed', grant: GRANT });
		expect(step(quiet, { type: 'ready', previewing: true })).toEqual({ kind: 'live', grant: GRANT });
		// Quiet only matters while waiting for a frame: not while issuing, not once live.
		expect(step({ kind: 'opening', grant: null }, { type: 'quiet' })).toEqual({ kind: 'opening', grant: null });
		expect(step({ kind: 'live', grant: GRANT }, { type: 'quiet' })).toEqual({ kind: 'live', grant: GRANT });
	});

	test('issuing refused: not set up, or failed with the error kept', () => {
		const issuing: PaneState = { kind: 'opening', grant: null };
		expect(step(issuing, { type: 'failed', error: failure(422, 'preview_not_configured') })).toEqual({ kind: 'not-set-up' });
		const busy = failure(503, 'unavailable', 'retry');
		expect(step(issuing, { type: 'failed', error: busy })).toEqual({ kind: 'failed', error: busy, grant: null });
		// Trying again from a failure issues afresh.
		expect(step({ kind: 'failed', error: busy, grant: null }, { type: 'open' })).toEqual({ kind: 'opening', grant: null });
	});

	test('reload keeps the grant; a fresh code refused as gone means the preview ended', () => {
		const live: PaneState = { kind: 'live', grant: GRANT };
		const reloading = step(live, { type: 'reload' });
		expect(reloading).toEqual({ kind: 'opening', grant: GRANT });
		expect(step(reloading, { type: 'failed', error: failure(404, 'not_found', 'not-found') })).toEqual({ kind: 'ended' });
		const busy = failure(500, 'internal', 'retry');
		const failed = step(reloading, { type: 'failed', error: busy });
		expect(failed).toEqual({ kind: 'failed', error: busy, grant: GRANT });
		expect(grantOf(failed)).toBe(GRANT);
		expect(step(failed, { type: 'reload' })).toEqual({ kind: 'opening', grant: GRANT });
	});

	test('ended, then started again; close always closes; stray events change nothing', () => {
		const ended = step({ kind: 'live', grant: GRANT }, { type: 'ended' });
		expect(ended).toEqual({ kind: 'ended' });
		expect(grantOf(ended)).toBeNull();
		expect(step(ended, { type: 'ready', previewing: true })).toEqual(ended);
		expect(step(ended, { type: 'open' })).toEqual({ kind: 'opening', grant: null });
		for (const state of [ended, { kind: 'live', grant: GRANT } as PaneState, { kind: 'opening', grant: null } as PaneState]) {
			expect(step(state, { type: 'close' })).toEqual(closed);
		}
		expect(step(closed, { type: 'ready', previewing: true })).toEqual(closed);
		expect(step(closed, { type: 'issued', grant: GRANT })).toEqual(closed);
		expect(step(closed, { type: 'ended' })).toEqual(closed);
	});
});

test.describe('messages', () => {
	const frame = { name: 'the frame' };
	const ready = (previewing: unknown, origin = 'https://www.example.com', source: unknown = frame) => ({
		origin,
		source,
		data: { type: 'colophon:preview-ready', previewing }
	});

	test('only the grant’s exact origin, from the frame, with the right shape, is heard', () => {
		expect(readyFrom(ready(true), GRANT.origin, frame)).toBe(true);
		expect(readyFrom(ready(false), GRANT.origin, frame)).toBe(false);
		// Lookalike origins, another window, and malformed messages are all ignored.
		for (const origin of ['https://www.example.com.evil.test', 'http://www.example.com', 'https://example.com', 'https://www.example.com:8443', 'null', '']) {
			expect(readyFrom(ready(true, origin), GRANT.origin, frame), origin).toBeNull();
		}
		expect(readyFrom(ready(true, GRANT.origin, { name: 'another window' }), GRANT.origin, frame)).toBeNull();
		expect(readyFrom(ready('yes'), GRANT.origin, frame)).toBeNull();
		expect(readyFrom({ origin: GRANT.origin, source: frame, data: 'colophon:preview-ready' }, GRANT.origin, frame)).toBeNull();
		expect(readyFrom({ origin: GRANT.origin, source: frame, data: { type: 'colophon:draft-saved', previewing: true } }, GRANT.origin, frame)).toBeNull();
		expect(readyFrom({ origin: GRANT.origin, source: frame, data: null }, GRANT.origin, frame)).toBeNull();
		// With no grant origin, nothing is heard.
		expect(readyFrom(ready(true, ''), '', frame)).toBeNull();
	});

	test('what a save tells the site', () => {
		expect(draftSaved('e1', 'r2')).toEqual({ type: 'colophon:draft-saved', entry: 'e1', revision: 'r2' });
	});
});

test.describe('timing and words', () => {
	const now = Date.parse('2026-11-17T12:00:00Z');

	test('"Ends in" rounds down and never promises more than is left', () => {
		expect(endsIn('2026-11-17T12:12:59Z', now)).toBe('Ends in 12 min');
		expect(endsIn('2026-11-17T12:01:00Z', now)).toBe('Ends in 1 min');
		expect(endsIn('2026-11-17T12:00:59Z', now)).toBe('Ends in under a minute');
		expect(endsIn('2026-11-17T12:00:00Z', now)).toBe('Ended');
		expect(endsIn('not a date', now)).toBe('Ended');
		expect(expired('2026-11-17T12:00:00Z', now)).toBe(true);
		expect(expired('2026-11-17T12:00:01Z', now)).toBe(false);
	});

	test('renewal is every few minutes, and sooner when the end is near', () => {
		const far = '2026-11-17T12:15:00Z';
		expect(renewDue(far, now - 60_000, now)).toBe(false);
		expect(renewDue(far, now - 4 * 60_000, now)).toBe(true);
		expect(renewDue('2026-11-17T12:02:00Z', now - 10_000, now)).toBe(true);
	});

	test('a preset wider than the pane is shown smaller, never wider', () => {
		expect(scaleFor(null, 500)).toBe(1);
		expect(scaleFor(360, 500)).toBe(1);
		expect(scaleFor(1280, 640)).toBe(0.5);
		expect(scaleFor(768, 500)).toBe(0.65);
		expect(scaleFor(1280, 0)).toBe(1);
		expect(hostOf(GRANT.url)).toBe('www.example.com');
		expect(hostOf('not a url')).toBe('');
	});

	test('why a preview didn’t open, in plain words with the codes kept for Technical details', () => {
		const cases = [
			new ApiError('retry', 'too many previews', 429, 'preview_limit'),
			new ApiError('refused', 'the preview address is invalid', 422, 'invalid_preview_url'),
			new ApiError('not-found', 'not found', 404, null),
			new ApiError('retry', 'internal', 500, null),
			new ApiError('retry', 'no answer from the server'),
			new ApiError('refused', 'missing permission', 403, 'missing_permission'),
			new Error('boom')
		];
		for (const error of cases) {
			const said = previewProblem(error);
			expect(said.title.length, error.message).toBeGreaterThan(0);
			expect(INTERNAL_TERMS.test(`${said.title} ${said.text}`), `${said.title} ${said.text}`).toBe(false);
			expect(said.text, error.message).not.toMatch(/preview_limit|invalid_preview_url|HTTP/);
		}
		expect(previewProblem(cases[0]).title).toBe('Too many previews open');
		expect(previewProblem(cases[0]).technical).toContain('preview_limit');
		expect(previewProblem(cases[1]).text).toMatch(/Settings, under Website connections/);
		expect(previewProblem(cases[4]).text).toMatch(/didn’t answer/);
	});
});
