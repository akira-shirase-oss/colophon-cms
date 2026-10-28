// Honest states (D-059, rule 3; ST4): a role that can't read one collection still gets a working
// Overview and its checklist, sees only the collections it can read, and is told plainly when it
// opens one it can't; a failed read shows the problem with a way to try again, never an endless
// "Loading…".
//
// Regression: the Overview waited for every collection's list, and the server refuses the list of
// a collection the role can't read (403), so the page said "Loading…" for good and the checklist
// never appeared; the command palette's search failed as a whole.

import { expect, test } from '@playwright/test';
import { api, env, internalTerms, key, member, newEntry, newProject, signIn, violations } from './helpers.ts';

const PAGES = {
	name: 'Pages',
	title_field: 'title',
	fields: [{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false }]
};

test('a role that can’t read one collection gets the Overview, its checklist and a working search', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'pages', definition: PAGES }, { 'Idempotency-Key': key() })).status).toBe(201);
	const role = await api(page, 'POST', `/api/v1/projects/${project}/roles`, {
		key: 'articles-writer',
		name: 'Articles writer',
		permissions: [
			{ action: 'content.read', subject: 'articles' },
			{ action: 'content.create', subject: 'articles' },
			{ action: 'content.update', subject: 'articles' }
		]
	});
	expect(role.status).toBe(201);
	await newEntry(page, project, 'Findable article');
	await newEntry(page, project, 'A private page', 'pages', { title: 'A private page' });

	const writer = await member(page, browser, project, 'articles-writer');
	await expect(writer.getByTestId('first-run')).toBeVisible();
	await expect(writer.getByTestId('first-run-item').first()).toBeVisible();
	await expect(writer.getByTestId('nav-collections')).toContainText('Articles');
	await expect(writer.getByTestId('nav-collections')).not.toContainText('Pages');
	await expect(writer.getByTestId('content-types')).not.toContainText('Pages');
	await expect(writer.getByText('Findable article')).toBeVisible();
	await expect(writer.getByText('Loading…')).toHaveCount(0);
	await expect(writer.getByTestId('problem')).toHaveCount(0);
	expect(await violations(writer)).toEqual([]);
	expect(await internalTerms(writer)).toEqual([]);

	// Search finds what the role can read.
	await writer.keyboard.press('ControlOrMeta+k');
	await writer.getByPlaceholder('Search entries or jump to…').fill('Findable');
	await expect(writer.getByRole('option', { name: /Findable article/ })).toBeVisible();
	await writer.keyboard.press('Escape');

	// Opened directly, a collection the role can't read says so in plain words.
	await writer.goto(`/studio/projects/${project}/pages`);
	await expect(writer.getByTestId('problem')).toContainText("Your role can't see this");
	await expect(writer.getByTestId('problem')).toContainText("can't see Pages");
	expect(await internalTerms(writer)).toEqual([]);
	await writer.context().close();
});

test('a failed read shows the problem and a way to try again, not “Loading…” for good', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	let failing = true;
	await page.route('**/environments/production/changesets', (route) =>
		failing && route.request().method() === 'GET'
			? route.fulfill({ status: 500, json: { error: { code: 'internal', message: 'internal error' } } })
			: route.fallback()
	);
	await page.goto(`/studio/projects/${project}`);
	// Reads that can succeed on a second try are retried first (three times), then reported.
	const problem = page.getByTestId('problem').filter({ hasText: 'publish requests' });
	await expect(problem).toContainText("Couldn't load publish requests", { timeout: 20_000 });
	await expect(problem).toContainText('Try again in a moment');
	await expect(page.getByText('Loading…')).toHaveCount(0);
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);

	failing = false;
	await problem.getByRole('button', { name: 'Try again' }).click();
	await expect(page.getByText('Nothing is waiting for review.')).toBeVisible();
	await expect(page.getByTestId('awaiting-count')).toHaveText('0');
	await expect(page.getByTestId('first-run')).toBeVisible();
});

test('Settings shows a failed read of content types, and a way to try again', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	let failing = true;
	await page.route('**/environments/production/content-types', (route) =>
		failing && route.request().method() === 'GET'
			? route.fulfill({ status: 503, json: { error: { code: 'unavailable', message: 'the server is busy; try again shortly' } } })
			: route.fallback()
	);
	await page.goto(`/studio/projects/${project}/settings`);
	const problem = page.getByTestId('problem').filter({ hasText: 'content types' });
	await expect(problem).toContainText("Couldn't load content types", { timeout: 20_000 });
	expect(await violations(page)).toEqual([]);
	failing = false;
	await problem.getByRole('button', { name: 'Try again' }).click();
	await expect(page.getByTestId('content-types')).toContainText('Articles');
});
