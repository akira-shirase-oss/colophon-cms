// Undo after Unpublish is one request to the server (D-184), which puts back exactly what readers
// had, or nothing. These drive it the way people do: someone else publishes in between (a second
// browser), the answer is lost on the way back, the offer runs out, a double click, typing that
// isn't saved yet. Each one counts the requests that went out and reads what readers get.

import { expect, type Page, test } from '@playwright/test';
import { api, env, key, member, newEntry, newProject, publishThroughApi, signIn, toast } from './helpers.ts';

function body(...paragraphs: string[]) {
	return {
		schema: 'colophon/rich-text',
		version: 1,
		doc: { type: 'doc', content: paragraphs.map((text) => ({ type: 'paragraph', content: [{ type: 'text', text }] })) }
	};
}

/** What readers get for one entry, without signing in: its status and, when live, its title. */
async function readers(page: Page, project: string, entry: string): Promise<{ status: number; title?: string }> {
	return page.evaluate(async (path) => {
		const response = await fetch(path, { credentials: 'omit', cache: 'no-store' });
		if (!response.ok) return { status: response.status };
		return { status: response.status, title: ((await response.json()) as { data: { title: string } }).data.title };
	}, `/content/v1/${project}/production/articles/${entry}`);
}

/** The environment's newest release. */
async function newest(page: Page, project: string): Promise<number> {
	return Number(await page.evaluate(async (path) => (await fetch(path)).headers.get('colophon-generation'), `${env(project)}/releases`));
}

/** Every write the page sends from now on, as `METHOD path`. */
function writes(page: Page): string[] {
	const seen: string[] = [];
	page.on('request', (request) => {
		if (request.method() !== 'GET' && request.url().includes('/api/v1/')) seen.push(`${request.method()} ${new URL(request.url()).pathname}`);
	});
	return seen;
}

/** Opens the entry and takes it down from ⋯ More; the Unpublished toast. */
async function unpublish(page: Page, project: string, entry: string, generation: number) {
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', /live|changed/);
	await page.getByTestId('more-menu').click();
	await page.getByTestId('unpublish-button').click();
	await page.getByRole('alertdialog').getByRole('button', { name: 'Unpublish' }).click();
	const unpublished = toast(page, `Unpublished in Release ${generation + 1}`);
	await expect(unpublished).toBeVisible();
	return unpublished;
}

test('Undo after someone else published in between is refused, says why, and puts nothing back', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, '', 'articles', { title: 'Lifeboat open day', body: body('Saturday from ten.') });
	const generation = await publishThroughApi(page, project, 'Open day', [entry]);
	const unpublished = await unpublish(page, project, entry, generation);

	// A colleague, in their own browser, publishes something else meanwhile.
	const colleague = await member(page, browser, project, 'publisher');
	const theirs = await newEntry(colleague, project, '', 'articles', { title: 'Harbour fees', body: body('Mooring is 14 a night.') });
	expect(await publishThroughApi(colleague, project, 'Fees', [theirs])).toBe(generation + 2);

	const sent = writes(page);
	await unpublished.getByRole('button', { name: 'Undo' }).click();
	const refused = toast(page, 'Not undone');
	await expect(refused).toBeVisible();
	await expect(refused).toContainText('Another release went out in the meantime');
	expect(sent.filter((write) => write.endsWith('/undo'))).toHaveLength(1);
	// No silent overwrite: their release stands, this entry stays down, nothing new committed.
	expect(await newest(page, project)).toBe(generation + 2);
	expect((await readers(page, project, entry)).status).toBe(410);
	expect((await readers(page, project, theirs)).status).toBe(200);
	await colleague.context().close();
});

test('one press is one request: Publish, Unpublish and Undo each send exactly one, even double-clicked', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, '', 'articles', { title: 'Regatta programme', body: body('Races start at eleven.') });
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	const sent = writes(page);
	await page.getByTestId('publish-button').dblclick();
	await expect(toast(page, 'Published in Release 1')).toBeVisible();
	expect(sent.filter((write) => write.endsWith('/publish'))).toHaveLength(1);
	expect(sent.filter((write) => write.endsWith('/changesets'))).toHaveLength(1);

	await page.getByTestId('more-menu').click();
	await page.getByTestId('unpublish-button').click();
	await page.getByRole('alertdialog').getByRole('button', { name: 'Unpublish' }).dblclick();
	const unpublished = toast(page, 'Unpublished in Release 2');
	await expect(unpublished).toBeVisible();
	expect(sent.filter((write) => write.endsWith('/publish'))).toHaveLength(2);

	await unpublished.getByRole('button', { name: 'Undo' }).dblclick({ force: true });
	await expect(toast(page, 'Back on your site in Release 3')).toBeVisible();
	expect(sent.filter((write) => write.endsWith('/undo'))).toHaveLength(1);
	expect(await newest(page, project)).toBe(3);
	expect((await readers(page, project, entry)).status).toBe(200);
});

test('a lost answer: Undo asks again with the same key and gets the one release', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, '', 'articles', { title: 'Ferry strike notice', body: body('No sailings on Friday.') });
	const generation = await publishThroughApi(page, project, 'Strike', [entry]);
	const unpublished = await unpublish(page, project, entry, generation);

	// The first undo reaches the server and commits, but its answer never comes back.
	const keys: string[] = [];
	let dropped = false;
	await page.route('**/releases/*/undo', async (route) => {
		keys.push(route.request().headers()['idempotency-key'] ?? '');
		if (!dropped) {
			dropped = true;
			await route.fetch();
			await route.abort('connectionreset');
			return;
		}
		await route.continue();
	});
	await unpublished.getByRole('button', { name: 'Undo' }).click();
	await expect(toast(page, `Back on your site in Release ${generation + 2}`)).toBeVisible();
	expect(keys.length).toBeGreaterThanOrEqual(2);
	expect(new Set(keys).size, 'every attempt is the same request').toBe(1);
	expect(await newest(page, project), 'one release, however many attempts').toBe(generation + 2);
	expect((await readers(page, project, entry)).status).toBe(200);
});

test('Undo with typing not saved yet puts back what readers had and keeps the typing', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, '', 'articles', { title: 'Tide tables for March', body: body('High water at 06:12.') });
	const generation = await publishThroughApi(page, project, 'Tides', [entry]);
	const unpublished = await unpublish(page, project, entry, generation);
	// Saves are slow, so the typing below is still unsaved when Undo is pressed.
	await page.route('**/collections/articles/entries/*', async (route) => {
		if (route.request().method() === 'PUT') await new Promise((wake) => setTimeout(wake, 1500));
		await route.continue();
	});
	const title = page.getByLabel('Title', { exact: true });
	await title.fill('Tide tables for March, corrected');
	await unpublished.getByRole('button', { name: 'Undo' }).click();
	await expect(toast(page, `Back on your site in Release ${generation + 2}`)).toBeVisible();
	expect(await readers(page, project, entry)).toEqual({ status: 200, title: 'Tide tables for March' });
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved', { timeout: 15_000 });
	await expect(title).toHaveValue('Tide tables for March, corrected');
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'changed');
	const draft = await api(page, 'GET', `${env(project)}/collections/articles/entries/${entry}`);
	expect(draft.body?.['data'].content.title).toBe('Tide tables for March, corrected');
});

test('the Undo offer lasts ten seconds, and the server refuses an undo of someone else’s takedown', async ({ page, browser }) => {
	test.setTimeout(60_000);
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, '', 'articles', { title: 'Lighthouse tours', body: body('On the hour.') });
	const generation = await publishThroughApi(page, project, 'Tours', [entry]);
	const unpublished = await unpublish(page, project, entry, generation);
	await expect(unpublished).toBeVisible();
	await expect(unpublished).toBeHidden({ timeout: 12_000 });
	// Gone from the screen; a colleague asking the server directly is refused by name.
	const colleague = await member(page, browser, project, 'publisher');
	const asked = await api(colleague, 'POST', `${env(project)}/releases/${generation + 1}/undo`, undefined, { 'Idempotency-Key': key() });
	expect(asked.status).toBe(403);
	expect(asked.body?.['error'].details.reason).toBe('not_your_release');
	expect((await readers(page, project, entry)).status).toBe(410);
	await colleague.context().close();
});
