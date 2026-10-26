// Publishing in the Studio against a real server (D-045): an approved changeset becomes release 1,
// the Releases page and public delivery show it, and a second tab that still thinks release 1 is
// live is told another release came first, then publishes on top of it.

import { type Page, expect, test } from '@playwright/test';
import { newProject, signIn, violations } from './helpers.ts';

/** An approved changeset of one new article, made through the API; the changeset page's path. */
async function approvedChangeset(page: Page, project: string, title: string): Promise<string> {
	const id = await page.evaluate(
		async ([project, title]) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const headers = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
			const key = () => crypto.randomUUID().replaceAll('-', '');
			const base = `/api/v1/projects/${project}/environments/production`;
			const body = {
				title,
				body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Hello.' }] }] } }
			};
			const entry = await fetch(`${base}/collections/articles/entries`, { method: 'POST', headers: { ...headers, 'Idempotency-Key': key() }, body: JSON.stringify(body) });
			const entryId = ((await entry.json()) as { data: { id: string } }).data.id;
			const made = await fetch(`${base}/changesets`, { method: 'POST', headers: { ...headers, 'Idempotency-Key': key() }, body: JSON.stringify({ title, entries: [entryId] }) });
			const set = ((await made.json()) as { data: { id: string; hash: string } }).data;
			const ifMatch = { ...headers, 'If-Match': `"${set.hash}"` };
			await fetch(`${base}/changesets/${set.id}/submit`, { method: 'POST', headers: ifMatch });
			await fetch(`${base}/changesets/${set.id}/reviews`, { method: 'POST', headers: ifMatch, body: JSON.stringify({ decision: 'approve' }) });
			return set.id;
		},
		[project, title]
	);
	return `/studio/projects/${project}/changesets/${id}`;
}

const status = (page: Page) => page.getByTestId('changeset-status');

test('an approved changeset is published as release 1, and a stale tab is told another release came first', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	const first = await approvedChangeset(page, project, 'Opening day');

	await page.goto(first);
	await expect(status(page)).toHaveAttribute('data-state', 'approved');
	await page.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(status(page)).toHaveAttribute('data-state', 'published');
	await expect(page.getByTestId('release-link')).toHaveText('release 1');

	await page.getByTestId('release-link').click();
	await expect(page.getByTestId('current-generation')).toContainText('Live: release 1');
	await expect(page.getByTestId('releases')).toContainText('Opening day');
	expect(await violations(page)).toEqual([]);
	const delivered = await page.evaluate(async (path) => {
		const response = await fetch(path, { credentials: 'omit' });
		return { generation: response.headers.get('colophon-generation'), body: (await response.json()) as { data: { title: string }[] } };
	}, `/content/v1/${project}/production/articles`);
	expect(delivered.generation).toBe('1');
	expect(delivered.body.data.map((entry) => entry.title)).toEqual(['Opening day']);

	// Two approved changesets; a second tab opens one while release 1 is live.
	const second = await approvedChangeset(page, project, 'Second wave');
	const third = await approvedChangeset(page, project, 'Third wave');
	const other = await (await browser.newContext()).newPage();
	await signIn(other);
	await other.goto(third);
	await expect(status(other)).toHaveAttribute('data-state', 'approved');

	await page.goto(second);
	await page.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(page.getByTestId('release-link')).toHaveText('release 2');

	await other.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(other.getByTestId('changeset-notice')).toContainText('Another release was published first');
	await expect(status(other)).toHaveAttribute('data-state', 'approved');
	await other.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(other.getByTestId('release-link')).toHaveText('release 3');
	expect(await violations(other)).toEqual([]);
});

test('an article is taken down through a reviewed changeset, and delivery says it is gone', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(await approvedChangeset(page, project, 'Short-lived'));
	await page.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(page.getByTestId('release-link')).toHaveText('release 1');
	const entry = await page.evaluate(async (path) => {
		const response = await fetch(path, { credentials: 'omit' });
		return ((await response.json()) as { data: { id: string }[] }).data[0]?.id ?? '';
	}, `/content/v1/${project}/production/articles`);

	await page.goto(`/studio/projects/${project}/changesets`);
	await page.getByRole('button', { name: 'New changeset' }).click();
	await page.getByLabel('Title', { exact: true }).fill('Take it down');
	await page.getByRole('checkbox', { name: 'Unpublish Short-lived' }).check();
	await expect(page.getByRole('checkbox', { name: 'Short-lived', exact: true })).toBeDisabled();
	expect(await violations(page)).toEqual([]);
	await page.getByRole('button', { name: 'Create changeset' }).click();
	await expect(page.getByTestId('changeset-items')).toContainText('Short-lived');
	await expect(page.getByTestId('unpublish')).toBeVisible();
	await page.getByRole('button', { name: 'Submit for review' }).click();
	await page.getByRole('button', { name: 'Approve this version' }).click();
	await expect(status(page)).toHaveAttribute('data-state', 'approved');
	await page.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(page.getByTestId('release-link')).toHaveText('release 2');

	await page.getByTestId('release-link').click();
	await expect(page.locator('[data-generation="2"]').getByTestId('unpublished')).toHaveText('1 unpublished');
	const after = await page.evaluate(
		async ({ list, one }) => {
			const listed = (await (await fetch(list, { credentials: 'omit' })).json()) as { data: unknown[] };
			const statuses = await Promise.all([one, `${one}?generation=1`].map(async (path) => (await fetch(path, { credentials: 'omit' })).status));
			return { listed: listed.data.length, statuses };
		},
		{ list: `/content/v1/${project}/production/articles?generation=1`, one: `/content/v1/${project}/production/articles/${entry}` }
	);
	expect(after).toEqual({ listed: 0, statuses: [410, 410] });
});
