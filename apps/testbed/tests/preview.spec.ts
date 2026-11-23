// Draft preview on the editor's own site, through the real HTTP API, in a real browser. The
// testbed's `/preview/**` pages play the customer's site with the SDK's helpers
// (docs/guides/preview.md): an article is published, its draft edited, the preview target set to
// the site's handler, and a preview issued. Opening its URL shows the draft under a "Preview
// mode" banner with private, unindexed headers; the spent code gets the small 404 page; exit
// returns to the published page. A caching proxy in front of the site (a "cache everything" CDN)
// stores nothing from a preview and keeps serving the public the published page; a Studio-like
// frame hears the site's ready message and makes it reload after a saved draft.

import { expect, test, type Page } from '@playwright/test';
import { FakeCdn } from './fake-cdn.ts';
import { signIn } from './sign-in.ts';

const CDN_PORT = Number(process.env['COLOPHON_TESTBED_CDN_PORT'] ?? 58089);

interface Answer {
	status: number;
	body: unknown;
	etag: string | null;
}

/** One API call from the signed-in testbed page, through its `/server` proxy, as other specs do. */
function api(page: Page, method: string, path: string, body?: unknown, headers: Record<string, string> = {}): Promise<Answer> {
	return page.evaluate(
		async ({ method, path, body, headers }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('__Host-colophon_csrf='))?.slice('__Host-colophon_csrf='.length) ?? '';
			const response = await fetch(`/server${path}`, {
				method,
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, ...headers },
				...(body === undefined ? {} : { body: JSON.stringify(body) })
			});
			const text = await response.text();
			return { status: response.status, body: text === '' ? null : (JSON.parse(text) as unknown), etag: response.headers.get('etag') };
		},
		{ method, path, body, headers }
	);
}

function data<T>(answer: Answer, what: string): T {
	if (answer.status >= 300) throw new Error(`${what}: HTTP ${answer.status} ${JSON.stringify(answer.body)}`);
	return (answer.body as { data: T }).data;
}

const doc = (text: string) => ({
	schema: 'colophon/rich-text',
	version: 1,
	doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] }
});

interface Site {
	project: string;
	entry: string;
	/** The site's page for the article. */
	post: string;
	base: string;
}

/** Saves a new draft over the entry's current revision; answers the new revision's id. */
async function saveDraft(page: Page, site: Site, title: string, body: string): Promise<string> {
	const path = `${site.base}/collections/articles/entries/${site.entry}`;
	const current = await api(page, 'GET', path);
	data(current, 'read draft');
	const saved = data<{ revision?: { id?: string } }>(
		await api(page, 'PUT', path, { title, body: doc(body) }, { 'If-Match': current.etag ?? '', 'Idempotency-Key': crypto.randomUUID() }),
		'save draft'
	);
	return saved.revision?.id ?? 'unknown';
}

/**
 * A project with one published article ("Published title") whose draft says "Draft title", and
 * a preview target naming this site's handler at `siteOrigin` — the testbed itself, or the CDN
 * in front of it.
 */
async function publishedWithDraft(page: Page, siteOrigin: string): Promise<Site> {
	const slug = `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
	const project = data<{ id: string }>(await api(page, 'POST', '/api/v1/projects', { slug, name: 'Preview testbed' }), 'project').id;
	const base = `/api/v1/projects/${project}/environments/production`;
	const write = () => ({ 'Idempotency-Key': crypto.randomUUID() });

	const entry = data<{ id: string }>(await api(page, 'POST', `${base}/collections/articles/entries`, { title: 'Published title', body: doc('Published body') }, write()), 'entry').id;
	const set = data<{ id: string; hash: string }>(await api(page, 'POST', `${base}/changesets`, { title: 'Preview testbed', entries: [entry] }, write()), 'changeset');
	const ifMatch = { 'If-Match': `"${set.hash}"` };
	data(await api(page, 'POST', `${base}/changesets/${set.id}/submit`, undefined, ifMatch), 'submit');
	data(await api(page, 'POST', `${base}/changesets/${set.id}/reviews`, { decision: 'approve' }, ifMatch), 'review');
	data(await api(page, 'POST', `${base}/changesets/${set.id}/publish`, { base_generation: 0 }, { ...ifMatch, ...write() }), 'publish');

	const site: Site = { project, entry, post: `/preview/${project}/articles/${entry}`, base };
	await saveDraft(page, site, 'Draft title', 'Draft body');

	const template = `${siteOrigin}/preview/${project}/api?code={code}&path=/preview/${project}/{collection}/{id}`;
	const target = data<{ url_template: string | null; origin: string | null }>(await api(page, 'PUT', `${base}/preview-target`, { url_template: template }), 'preview target');
	expect(target.url_template).toBe(template);
	expect(target.origin).toBe(siteOrigin);
	return site;
}

interface Grant {
	id: string;
	code: string;
	url: string;
	origin: string;
	expires_at: string;
	scope: { kind: string; entry?: string };
}

async function issue(page: Page, site: Site): Promise<Grant> {
	const grant = data<Grant>(await api(page, 'POST', `${site.base}/previews`, { entry: site.entry }), 'issue preview');
	expect(grant.code).toMatch(/^cpc_/);
	expect(grant.scope).toMatchObject({ kind: 'entry', entry: site.entry });
	return grant;
}

test('a preview shows the draft on the site, privately; its code works once; exit returns to the published page', async ({ page, baseURL }) => {
	const origin = new URL(baseURL ?? '').origin;
	await signIn(page, 'dev-owner');
	const site = await publishedWithDraft(page, origin);
	const grant = await issue(page, site);
	expect(grant.origin).toBe(origin);
	expect(grant.url.startsWith(`${origin}/preview/${site.project}/api?code=`)).toBe(true);

	// The Studio opens the URL: the handler trades the code, sets its cookie, redirects to the post.
	const shown = await page.goto(grant.url);
	expect(new URL(page.url()).pathname).toBe(site.post);
	await expect(page.getByTestId('title')).toHaveText('Draft title');
	await expect(page.getByTestId('body')).toHaveText('Draft body');
	await expect(page.getByTestId('preview-banner')).toContainText('Preview mode');
	await expect(page.getByTestId('preview-scope')).toHaveText('entry');
	await expect(page.getByTestId('preview-source')).toHaveText('draft');
	const endsAt = Date.parse((await page.getByTestId('preview-ends').getAttribute('datetime')) ?? '');
	expect(endsAt).toBeGreaterThan(Date.now());
	expect(endsAt).toBeLessThanOrEqual(Date.now() + 16 * 60_000);

	// The page: never stored, never indexed, framable only by the Studio.
	const studio = await page.locator('[data-studio-origin]').getAttribute('data-studio-origin');
	const headers = (await shown?.allHeaders()) ?? {};
	expect(headers['cache-control']).toBe('private, no-store');
	expect(headers['x-robots-tag']).toBe('noindex');
	expect(headers['content-security-policy']).toBe(`frame-ancestors ${studio}`);

	// The handler's 303: the documented cookie, living as long as the token; no-store, no-referrer.
	const handler = await shown?.request().redirectedFrom()?.response();
	expect(handler?.status()).toBe(303);
	const handled = (await handler?.allHeaders()) ?? {};
	expect(handled['set-cookie']).toMatch(/^colophon_preview=cpv_[^;\s]+; HttpOnly; Secure; SameSite=None; Partitioned; Path=\/; Max-Age=\d+$/);
	const maxAge = Number(/Max-Age=(\d+)$/.exec(handled['set-cookie'] ?? '')?.[1]);
	expect(maxAge).toBeGreaterThan(14 * 60);
	expect(maxAge).toBeLessThanOrEqual(15 * 60);
	expect(handled['cache-control']).toBe('no-store');
	expect(handled['referrer-policy']).toBe('no-referrer');
	expect(handled['x-robots-tag']).toBe('noindex');
	const cookie = (await page.context().cookies()).find((held) => held.name === 'colophon_preview');
	expect(cookie).toMatchObject({ httpOnly: true, secure: true, sameSite: 'None', path: '/' });
	// Partitioned (CHIPS): kept under the top-level site it was set in.
	expect(cookie?.partitionKey).toBe(`${new URL(origin).protocol}//${new URL(origin).hostname}`);

	// The code is spent: the same URL answers the small 404 page, as privately.
	const spent = await page.goto(grant.url);
	expect(spent?.status()).toBe(404);
	await expect(page.locator('h1')).toHaveText('Preview link expired');
	const spentHeaders = (await spent?.allHeaders()) ?? {};
	expect(spentHeaders['cache-control']).toBe('no-store');
	expect(spentHeaders['x-robots-tag']).toBe('noindex');
	expect(spentHeaders['referrer-policy']).toBe('no-referrer');
	expect(spentHeaders['set-cookie']).toBeUndefined();

	// The cookie still holds the session; exit clears it and lands on the published page.
	await page.goto(site.post);
	await expect(page.getByTestId('title')).toHaveText('Draft title');
	await page.getByTestId('exit-preview').click();
	await expect(page.getByTestId('title')).toHaveText('Published title');
	await expect(page.getByTestId('body')).toHaveText('Published body');
	await expect(page.getByTestId('preview-banner')).toHaveCount(0);
	expect(new URL(page.url()).pathname).toBe(site.post);
	expect((await page.context().cookies()).some((held) => held.name === 'colophon_preview')).toBe(false);
	const published = (await (await page.reload())?.allHeaders()) ?? {};
	expect(published['cache-control']).toBe('public, max-age=0, s-maxage=60');
	expect(published['content-security-policy']).toBeUndefined();
});

test('a caching proxy in front of the site stores nothing from a preview and keeps the published page public', async ({ page, baseURL }) => {
	const origin = new URL(baseURL ?? '');
	const cdn = await FakeCdn.start(CDN_PORT, `http://127.0.0.1:${origin.port}`);
	try {
		await signIn(page, 'dev-owner');
		// The public reaches the site through the CDN, so the preview target names the CDN.
		const site = await publishedWithDraft(page, cdn.origin);
		const grant = await issue(page, site);
		expect(grant.origin).toBe(cdn.origin);

		// The preview page view, through the CDN: the handler's redirect and the draft page both
		// pass, and neither is kept — the headers say so, not a CDN rule.
		await page.goto(grant.url);
		await expect(page.getByTestId('title')).toHaveText('Draft title');
		const previewed = cdn.log.filter((row) => row.path.startsWith(`/preview/${site.project}/`));
		expect(previewed.map((row) => [row.path.split('?')[0], row.status, row.outcome, row.stored, row.reason])).toEqual([
			[`/preview/${site.project}/api`, 303, 'MISS', false, 'no-store'],
			[site.post, 200, 'MISS', false, 'no-store']
		]);
		expect(cdn.stored().filter((key) => key.includes(`/preview/${site.project}/`))).toEqual([]);

		// An anonymous visitor through the same CDN gets the published page, which the CDN keeps.
		const anonymous = `http://127.0.0.1:${CDN_PORT}${site.post}`;
		const first = await fetch(anonymous);
		const firstHtml = await first.text();
		expect(first.headers.get('x-fake-cdn')).toBe('MISS');
		expect(first.headers.get('x-fake-cdn-stored')).toBe('yes');
		expect(firstHtml).toContain('Published title');
		expect(firstHtml).not.toContain('Draft title');
		expect(firstHtml).not.toContain('Preview mode');
		const second = await fetch(anonymous);
		const secondHtml = await second.text();
		expect(second.headers.get('x-fake-cdn')).toBe('HIT');
		expect(secondHtml).toContain('Published title');
		expect(secondHtml).not.toContain('Draft title');

		// With the guide's CDN rule (bypass on the preview cookie), the editor still sees the
		// draft while the public copy stays cached.
		cdn.bypassCookie = 'colophon_preview';
		await page.reload();
		await expect(page.getByTestId('title')).toHaveText('Draft title');
		expect(cdn.log.filter((row) => row.path === site.post).at(-1)?.outcome).toBe('BYPASS');
		const third = await fetch(anonymous);
		expect(third.headers.get('x-fake-cdn')).toBe('HIT');
		expect(await third.text()).toContain('Published title');
	} finally {
		await cdn.stop();
	}
});

test('in a Studio frame the site says it is ready, reloads on a saved draft, and reports a missing session', async ({ page, baseURL }) => {
	const origin = new URL(baseURL ?? '').origin;
	await signIn(page, 'dev-owner');
	const site = await publishedWithDraft(page, origin);

	// The Studio origin the site trusts (its CSP and its messages name exactly this).
	await page.goto(site.post);
	const studio = (await page.locator('[data-studio-origin]').getAttribute('data-studio-origin')) ?? '';
	expect(studio).toMatch(/^https?:\/\/[^/]+$/);

	// A stand-in for the Studio's preview pane, served at the Studio's origin: it frames a URL
	// and records every message it hears. Playwright fulfils it, so Chromium cannot place it on
	// the loopback address and would ask before it frames localhost (Local Network Access) — as
	// it would for a public Studio framing a localhost dev site; the grant answers that prompt.
	await page.context().grantPermissions(['local-network-access']);
	let framed = '';
	const pane = (url: string) =>
		`<!doctype html><title>Studio</title><script>window.heard = []; addEventListener('message', (event) => window.heard.push({ origin: event.origin, data: event.data }));</script><iframe src="${url.replaceAll('&', '&amp;').replaceAll('"', '&quot;')}" width="900" height="700"></iframe>`;
	await page.route(`${studio}/__preview-pane`, (route) => route.fulfill({ contentType: 'text/html', body: pane(framed) }));
	const heard = () => page.evaluate(() => (window as unknown as { heard: unknown[] }).heard);
	// The API calls go from a second tab on the testbed, as the pane's origin has no `/server` proxy.
	const tab = await page.context().newPage();
	await tab.goto('/session');

	const grant = await issue(tab, site);
	framed = grant.url;
	await page.goto(`${studio}/__preview-pane`);
	const frame = page.frameLocator('iframe');
	await expect(frame.getByTestId('title')).toHaveText('Draft title');
	await expect.poll(heard).toContainEqual({ origin, data: { type: 'colophon:preview-ready', previewing: true } });

	// An acknowledged save, then the Studio's message to exactly the site's origin: the frame reloads.
	const revision = await saveDraft(tab, site, 'Second draft', 'Saved again');
	await page.evaluate(
		({ origin, message }) => document.querySelector('iframe')?.contentWindow?.postMessage(message, origin),
		{ origin, message: { type: 'colophon:draft-saved', entry: site.entry, revision } }
	);
	await expect(frame.getByTestId('title')).toHaveText('Second draft');
	await expect(frame.getByTestId('body')).toHaveText('Saved again');

	// Any other page that frames a preview — even with a fresh code — is refused by the CSP. The
	// other origin is on localhost too (nothing listens there; Playwright fulfils it), so Local
	// Network Access cannot be what stops the frame: only `frame-ancestors` can.
	const elsewhere = `http://localhost:${Number(new URL(studio).port) === 58095 ? 58094 : 58095}`;
	const stolen = await issue(tab, site);
	await page.route(`${elsewhere}/`, (route) => route.fulfill({ contentType: 'text/html', body: pane(stolen.url) }));
	const refused = page.waitForEvent('console', (message) => message.text().includes(`"frame-ancestors ${studio}"`));
	await page.goto(`${elsewhere}/`);
	await refused;
	await expect(frame.getByTestId('title')).toHaveCount(0);

	// Without the cookie (as when a browser blocks it in the frame) the page shows the published
	// entry and tells the Studio it holds no preview session.
	await page.context().clearCookies({ name: 'colophon_preview' });
	framed = `${origin}${site.post}`;
	await page.goto(`${studio}/__preview-pane`);
	await expect(frame.getByTestId('title')).toHaveText('Published title');
	await expect.poll(heard).toContainEqual({ origin, data: { type: 'colophon:preview-ready', previewing: false } });
	await tab.close();
});
