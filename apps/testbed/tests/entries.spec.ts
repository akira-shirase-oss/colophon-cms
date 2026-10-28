// Drafts through the testbed, in a real browser: create, edit and save with If-Match, replay a
// save with its Idempotency-Key, two tabs editing one article where the later save is refused
// instead of overwriting the earlier one, and deleting (D-080): refused with its reason while a
// changeset holds the article, then deleted, replayed with its key, and gone.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

/** Creates a project through the API as the signed-in page; its id. */
async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch('/server/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'Entries testbed' })
		});
		return ((await response.json()) as { data: { id: string } }).data.id;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

async function openEntries(page: Page, projectId: string) {
	await page.goto('/entries');
	await page.getByLabel('Project').selectOption(projectId);
	await expect(page.getByTestId('entries')).toBeVisible();
}

test('create an article, save a new revision, and replay the save with its key', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await openEntries(page, projectId);

	await page.getByLabel('New title').fill('Testbed article');
	await page.getByLabel('New text').fill('First paragraph.');
	await page.getByRole('button', { name: 'Create article' }).click();
	await expect(page.getByTestId('message')).toHaveText('created Testbed article');
	await expect(page.getByTestId('entries')).toContainText('Testbed article');
	await expect(page.getByTestId('history').locator('li')).toHaveCount(1);
	const first = await page.getByTestId('revision').textContent();

	await page.getByLabel('Title', { exact: true }).fill('Testbed article, revised');
	await page.getByRole('button', { name: 'Save', exact: true }).click();
	await expect(page.getByTestId('message')).toContainText('saved revision');
	await expect(page.getByTestId('history').locator('li')).toHaveCount(2);
	const second = await page.getByTestId('revision').textContent();
	expect(second).not.toBe(first);

	// The same request with the same key, as a client would after a lost response.
	await page.getByRole('button', { name: 'Retry last save' }).click();
	await expect(page.getByTestId('message')).toHaveText(`replayed revision ${second}`);
	await expect(page.getByTestId('history').locator('li')).toHaveCount(2);
});

test('two tabs edit one article: the later save is refused, and nothing is lost', async ({ browser }) => {
	const context = await browser.newContext();
	const first = await context.newPage();
	await signIn(first, 'dev-owner');
	const projectId = await newProject(first);
	await openEntries(first, projectId);
	await first.getByLabel('New title').fill('Shared article');
	await first.getByRole('button', { name: 'Create article' }).click();
	await expect(first.getByTestId('message')).toHaveText('created Shared article');

	// The same person in a second tab opens the same revision.
	const second = await context.newPage();
	await openEntries(second, projectId);
	await second.getByRole('button', { name: 'Shared article' }).click();
	await expect(second.getByTestId('editor')).toBeVisible();

	await first.getByLabel('Title', { exact: true }).fill('Edited in tab one');
	await first.getByRole('button', { name: 'Save', exact: true }).click();
	await expect(first.getByTestId('message')).toContainText('saved revision');

	await second.getByLabel('Title', { exact: true }).fill('Edited in tab two');
	await second.getByRole('button', { name: 'Save', exact: true }).click();
	await expect(second.getByTestId('message')).toHaveText('409 conflict (stale_revision)');

	await second.getByRole('button', { name: 'Reload' }).click();
	await expect(second.getByLabel('Title', { exact: true })).toHaveValue('Edited in tab one');
	await expect(second.getByTestId('history').locator('li')).toHaveCount(2);
	await context.close();
});

test('titles are searched with wildcards taken literally, and any revision reads back with its content', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await openEntries(page, projectId);
	for (const [title, text] of [
		['Field notes', 'From the field.'],
		['Kitchen sink', 'Everything.']
	] as const) {
		await page.getByLabel('New title').fill(title);
		await page.getByLabel('New text').fill(text);
		await page.getByRole('button', { name: 'Create article' }).click();
		await expect(page.getByTestId('message')).toHaveText(`created ${title}`);
	}
	await expect(page.getByTestId('entries').locator('li[data-state="draft"]')).toHaveCount(2);

	const search = async (text: string) => {
		await page.getByLabel('Search titles').fill(text);
		await page.getByRole('button', { name: 'Search', exact: true }).click();
	};
	await search('NOTES');
	await expect(page.getByTestId('search-results').locator('li')).toHaveText(['Field notes']);
	// As a wildcard, `_` would match the space in "Kitchen sink".
	await search('n_sink');
	await expect(page.getByTestId('search-status')).toHaveText('200');
	await expect(page.getByTestId('search-results').locator('li')).toHaveCount(0);
	await search('x'.repeat(101));
	await expect(page.getByTestId('search-status')).toHaveText('400');

	// "Kitchen sink" is open after creating it: save a second revision, then read the first.
	await page.getByLabel('Title', { exact: true }).fill('Kitchen sink, revised');
	await page.getByRole('button', { name: 'Save', exact: true }).click();
	await expect(page.getByTestId('history').locator('li')).toHaveCount(2);
	await page.getByTestId('history').locator('li').last().getByRole('button', { name: 'Read' }).click();
	await expect(page.getByTestId('revision-content')).toHaveAttribute('data-status', '200');
	await expect(page.getByTestId('revision-content')).toContainText('Kitchen sink');
	await expect(page.getByTestId('revision-content')).toContainText('Everything.');
	await expect(page.getByTestId('revision-content')).not.toContainText('revised');
});

/** Creates a changeset holding `entry`, through the API as the signed-in page; its status. */
async function newChangeset(page: Page, projectId: string, entry: string, title: string): Promise<number> {
	return page.evaluate(
		async ({ projectId, entry, title }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const response = await fetch(`/server/api/v1/projects/${projectId}/environments/production/changesets`, {
				method: 'POST',
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, 'Idempotency-Key': crypto.randomUUID() },
				body: JSON.stringify({ title, entries: [entry] })
			});
			return response.status;
		},
		{ projectId, entry, title }
	);
}

test('delete an article: refused with its reason while a changeset holds it, then gone, and the deletion replays', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await openEntries(page, projectId);

	// An article a changeset holds: deleting it is refused, naming the changeset.
	await page.getByLabel('New title').fill('Held article');
	await page.getByRole('button', { name: 'Create article' }).click();
	await expect(page.getByTestId('message')).toHaveText('created Held article');
	const held = await page.getByTestId('entries').locator('li', { hasText: 'Held article' }).getAttribute('data-id');
	expect(held).not.toBeNull();
	expect(await newChangeset(page, projectId, held ?? '', 'Testbed changeset')).toBe(201);
	await page.getByRole('button', { name: 'Delete', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('409 conflict (entry_in_changeset)');
	await expect(page.getByTestId('in-the-way')).toHaveText(['in changeset Testbed changeset']);
	await expect(page.getByTestId('entries')).toContainText('Held article');

	// An article nothing needs: deleted, gone from the list, and a retry with the same key replays.
	await page.getByLabel('New title').fill('Short-lived article');
	await page.getByRole('button', { name: 'Create article' }).click();
	await expect(page.getByTestId('message')).toHaveText('created Short-lived article');
	await page.getByRole('button', { name: 'Delete', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('deleted Short-lived article');
	await expect(page.getByTestId('editor')).toHaveCount(0);
	await expect(page.getByTestId('entries')).not.toContainText('Short-lived article');
	await expect(page.getByTestId('entries')).toContainText('Held article');
	await page.getByRole('button', { name: 'Retry last delete' }).click();
	await expect(page.getByTestId('message')).toHaveText('replayed deletion of Short-lived article');
});
