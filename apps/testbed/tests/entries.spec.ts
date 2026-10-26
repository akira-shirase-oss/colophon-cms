// Drafts through the testbed, in a real browser: create, edit and save with If-Match, replay a
// save with its Idempotency-Key, and two tabs editing one article where the later save is
// refused instead of overwriting the earlier one.

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
	await signIn(page, 'dev-editor');
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
	await signIn(first, 'dev-editor');
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
