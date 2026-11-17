// Preview on the editor's own site (D-178), against the real server: the owner sets the
// preview address in Settings; the editor's Preview opens the site beside the fields; the site —
// a page served by route fulfilment whose server side is this test, which trades the one-time
// code at the real exchange endpoint and reads the draft with the token, as a site's preview
// route does — says it is previewing; a save in the Studio reaches it; closing revokes the grant,
// and the token is refused from then on. The mocked journeys (every state, keyboard, phone, axe)
// are tests/site-preview.spec.ts; this one proves the contract end to end.

import { expect, type Page, test } from '@playwright/test';
import { api, env, internalTerms, key, newEntry, newProject, signIn } from './helpers.ts';

const SITE = 'https://site.preview.test';

/** The stub site's page: the draft it read, and the SDK helper's two messages. */
function page(title: string, previewing: boolean, studio: string): string {
	return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>${title} · Example site</title></head>
<body><main><h1 id="title">${title}</h1><p id="state">${previewing ? 'Preview mode' : 'Published version'}</p></main>
<script>
	window.addEventListener('message', async (event) => {
		if (event.origin !== ${JSON.stringify(studio)} || !event.data || event.data.type !== 'colophon:draft-saved') return;
		const answer = await fetch('/api/draft');
		const row = await answer.json();
		document.getElementById('title').textContent = row.title;
		document.getElementById('state').textContent = 'Updated to ' + event.data.revision;
	});
	parent.postMessage({ type: 'colophon:preview-ready', previewing: ${previewing} }, ${JSON.stringify(studio)});
</script></body></html>`;
}

/** The site's server side: its preview route and its draft read, using the real preview API. */
async function site(studio: Page, project: string, entry: string, base: string): Promise<{ tokens: string[]; reads: Response[] }> {
	const tokens: string[] = [];
	const reads: Response[] = [];
	const origin = new URL(base).origin;
	const read = async (token: string) => {
		const response = await fetch(`${base}/preview/v1/${project}/production/articles/${entry}`, { headers: { Authorization: `Bearer ${token}` } });
		reads.push(response.clone());
		return response;
	};
	await studio.context().route(`${SITE}/**`, async (route) => {
		const url = new URL(route.request().url());
		if (url.pathname === '/api/preview') {
			const exchanged = await fetch(`${base}/preview/v1/${project}/production/exchange`, {
				method: 'POST',
				headers: { 'Content-Type': 'application/json' },
				body: JSON.stringify({ code: url.searchParams.get('code') })
			});
			if (!exchanged.ok) {
				await route.fulfill({ status: 200, contentType: 'text/html', body: page('Not previewing', false, origin) });
				return;
			}
			const token = ((await exchanged.json()) as { data: { token: string } }).data.token;
			tokens.push(token);
			const row = ((await (await read(token)).json()) as { data: { title: string } }).data;
			await route.fulfill({ status: 200, contentType: 'text/html', body: page(row.title, true, origin) });
			return;
		}
		if (url.pathname === '/api/draft') {
			const token = tokens.at(-1) ?? '';
			const row = ((await (await read(token)).json()) as { data: unknown }).data;
			await route.fulfill({ status: 200, json: row });
			return;
		}
		await route.fulfill({ status: 404, body: 'not found' });
	});
	return { tokens, reads };
}

test('set the address, preview a draft on the site, a save reaches it, closing revokes it', async ({ page: studio, baseURL }) => {
	const base = baseURL ?? '';
	await signIn(studio);
	const project = await newProject(studio, 'Preview on the site');
	const entry = await newEntry(studio, project, 'A draft to preview');

	// Settings → Website connections → Preview on your site.
	await studio.goto(`/studio/projects/${project}/settings/connections`);
	const address = studio.getByLabel('Preview address');
	await address.fill(`${SITE}/api/preview`);
	await address.blur();
	await expect(studio.getByTestId('preview-address-problem')).toContainText('Add {code}');
	await address.fill(`${SITE}/api/preview?code={code}&id={id}`);
	await studio.getByTestId('preview-address-save').click();
	await expect(studio.getByTestId('preview-target-state')).toHaveText('On');
	const target = await api(studio, 'GET', `${env(project)}/preview-target`);
	expect(target.body?.['data']).toMatchObject({ url_template: `${SITE}/api/preview?code={code}&id={id}`, origin: SITE });

	// The server refuses what the Studio would refuse too (its 422, said plainly).
	const refused = await api(studio, 'PUT', `${env(project)}/preview-target`, { url_template: 'http://www.example.com/p?code={code}' });
	expect(refused.status).toBe(422);
	expect(refused.body?.['error'].details.reason).toBe('invalid_preview_url');

	// The editor: Preview → Side by side, live once the site says so.
	const seen = await site(studio, project, entry, base);
	await studio.goto(`/studio/projects/${project}/articles/${entry}`);
	await studio.getByTestId('preview-menu').click();
	await studio.getByRole('menuitem', { name: /Side by side/ }).click();
	await expect(studio.getByTestId('preview-pane')).toHaveAttribute('data-state', 'live');
	const frame = studio.frameLocator('[data-testid="preview-frame"]');
	await expect(frame.locator('#title')).toHaveText('A draft to preview');
	expect(seen.tokens).toHaveLength(1);
	expect(seen.tokens[0]).toMatch(/^cpv_/);
	const first = seen.reads[0];
	expect(first?.headers.get('cache-control')).toBe('private, no-store');
	expect(first?.headers.get('x-robots-tag')).toContain('noindex');
	expect(first?.headers.get('colophon-preview')).toBe('entry');
	expect(await internalTerms(studio)).toEqual([]);

	// A save the server acknowledged reaches the site, which reads the new draft with its token.
	await studio.getByLabel('Title', { exact: true }).fill('A draft, edited');
	await expect(studio.getByTestId('save-status')).toHaveAttribute('data-status', 'saved', { timeout: 10_000 });
	await expect(studio.getByTestId('preview-sync')).toHaveText('Updated');
	await expect(frame.locator('#title')).toHaveText('A draft, edited');

	// The grant is the editor's own, live; closing revokes it, and the site's token stops working.
	const live = await api(studio, 'GET', `${env(project)}/previews?entry=${entry}`);
	expect(live.body?.['data']).toHaveLength(1);
	await studio.getByTestId('preview-close').click();
	await expect(studio.getByTestId('preview-pane')).toHaveCount(0);
	await expect
		.poll(async () => (await fetch(`${base}/preview/v1/${project}/production/articles/${entry}`, { headers: { Authorization: `Bearer ${seen.tokens[0]}` } })).status)
		.toBe(401);
	const after = await fetch(`${base}/preview/v1/${project}/production/articles/${entry}`, { headers: { Authorization: `Bearer ${seen.tokens[0]}` } });
	expect(((await after.json()) as { error: { details: { reason: string } } }).error.details.reason).toBe('preview_revoked');
	expect((await api(studio, 'GET', `${env(project)}/previews?entry=${entry}`)).body?.['data']).toHaveLength(0);
});

test('a publish request: Preview on your site opens exactly it, with a one-time code for its versions', async ({ page: studio, baseURL }) => {
	const base = baseURL ?? '';
	await signIn(studio);
	const project = await newProject(studio, 'Preview a publish request');
	const entry = await newEntry(studio, project, 'In the spring issue');
	expect((await api(studio, 'PUT', `${env(project)}/preview-target`, { url_template: `${SITE}/api/preview?code={code}` })).status).toBe(200);
	const made = await api(studio, 'POST', `${env(project)}/changesets`, { title: 'Spring issue', entries: [entry], unpublish: [] }, { 'Idempotency-Key': key() });
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	const changeset = made.body?.['data'].id as string;
	// The site: a plain page; its preview route's exchange is done below, as its server would.
	await studio
		.context()
		.route(`${SITE}/**`, (route) =>
			route.fulfill({ status: 200, contentType: 'text/html', body: '<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Example site</title></head><body><main><h1>Example site</h1></main></body></html>' })
		);

	await studio.goto(`/studio/projects/${project}/changesets/${changeset}`);
	const button = studio.getByTestId('changeset-preview-button');
	await expect(button).toBeEnabled();
	await expect(studio.getByTestId('changeset-preview')).toContainText('Shows exactly what this request will publish, for 15 minutes.');
	const [tab] = await Promise.all([studio.waitForEvent('popup'), button.click()]);
	await tab.waitForURL(`${SITE}/**`);
	const code = new URL(tab.url()).searchParams.get('code') ?? '';
	expect(code).toMatch(/^cpc_/);
	await expect(studio.getByTestId('changeset-preview-problem')).toHaveCount(0);

	// The code is for exactly this request: its exchange names it, and reads come from its versions.
	const exchanged = await fetch(`${base}/preview/v1/${project}/production/exchange`, {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ code })
	});
	expect(exchanged.status).toBe(200);
	const grant = (await exchanged.json()) as { data: { token: string; scope: Record<string, unknown> } };
	expect(grant.data.scope).toMatchObject({ kind: 'changeset', changeset });
	const read = await fetch(`${base}/preview/v1/${project}/production/articles/${entry}`, { headers: { Authorization: `Bearer ${grant.data.token}` } });
	expect(read.status).toBe(200);
	expect(((await read.json()) as { data: { title: string; preview: { source: string } } }).data).toMatchObject({ title: 'In the spring issue', preview: { source: 'changeset' } });
});
