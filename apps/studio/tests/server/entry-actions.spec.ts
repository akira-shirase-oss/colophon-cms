// Finishing the editor: restore an older version (saved as a new one, so the history keeps
// everything), unpublish after a confirmation (and publish again to undo it), and a collection
// longer than a page loads more with the server's cursor. An entry older than the newest 100 still
// shows that it is live, from its own answer.

import { expect, type Page, test } from '@playwright/test';
import { env, internalTerms, newEntry, newProject, publishThroughApi, signIn, violations } from './helpers.ts';

/** Readers' view of the project's articles (no sign-in): the listed ids, and one entry's status. */
async function delivered(page: Page, project: string, entry: string): Promise<{ ids: string[]; status: number }> {
	return page.evaluate(
		async ({ list, one }) => {
			const listed = (await (await fetch(list, { credentials: 'omit', cache: 'no-store' })).json()) as { data: { id: string }[] };
			const status = (await fetch(one, { credentials: 'omit', cache: 'no-store' })).status;
			return { ids: listed.data.map((item) => item.id), status };
		},
		{ list: `/content/v1/${project}/production/articles`, one: `/content/v1/${project}/production/articles/${entry}` }
	);
}

/** A toast that says `text`. */
const toast = (page: Page, text: string) => page.locator('[data-sonner-toast]').filter({ hasText: text });

/** The number on the History tab. */
async function versions(page: Page): Promise<number> {
	const text = (await page.getByRole('tab', { name: /History/ }).textContent()) ?? '';
	return Number(/(\d+)\s*$/.exec(text)?.[1] ?? 'NaN');
}

test('restoring an older version saves it as a new one, and the history keeps them all', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/articles?new=1`);
	await page.getByLabel('Title', { exact: true }).fill('The first title');
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await page.getByLabel('Title', { exact: true }).fill('A second title');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await expect.poll(() => versions(page)).toBe(2);

	// Two saves a minute apart by one person are one sitting, opened to its single saves (D-184).
	await page.getByRole('tab', { name: /History/ }).click();
	await expect(page.getByTestId('history-row')).toHaveCount(1);
	await expect(page.getByTestId('history-row')).toContainText('You');
	await expect(page.getByTestId('history-row')).toContainText('Current draft');
	await page.getByTestId('history-saves').click();
	await expect(page.getByTestId('history-sitting').getByRole('listitem')).toHaveCount(2);
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);
	await page.getByTestId('history-sitting').getByRole('listitem').last().getByTestId('history-restore').click();

	await expect(page.getByTestId('restore-note')).toContainText('as a new version');
	await expect(page.getByLabel('Title', { exact: true })).toHaveValue('The first title');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await expect.poll(() => versions(page)).toBe(3);
	await page.reload();
	await expect(page.getByLabel('Title', { exact: true })).toHaveValue('The first title');
	await page.getByRole('tab', { name: /History/ }).click();
	await expect(page.getByTestId('history-saves')).toHaveText('3 saves');
	await page.getByTestId('history-saves').click();
	await expect(page.getByTestId('history-sitting').getByRole('listitem')).toHaveCount(3);
});

test('unpublishing, once confirmed, takes an entry off the public API, and publishing again puts it back', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/articles?new=1`);
	await page.getByLabel('Title', { exact: true }).fill('Here today');
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	const entry = new URL(page.url()).pathname.split('/').at(-1) ?? '';
	await page.getByTestId('publish-button').click();
	await expect(toast(page, 'Published in Release 1')).toBeVisible();
	await expect(page.getByTestId('publish-state')).toContainText('Live in Release 1');
	expect((await delivered(page, project, entry)).ids).toContain(entry);

	await page.getByTestId('more-menu').click();
	await page.getByTestId('unpublish-button').click();
	await page.getByTestId('unpublish-confirm').click();
	await expect(toast(page, 'Unpublished in Release 2')).toBeVisible();
	await expect(page.getByTestId('entry-state').first()).toHaveAttribute('data-state', 'taken_down');
	await expect(page.getByTestId('publish-state')).toHaveText('Readers no longer get it');
	expect(await delivered(page, project, entry)).toEqual({ ids: [], status: 410 });
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);

	await expect(page.getByTestId('publish-button')).toHaveText('Publish again');
	await page.getByTestId('publish-button').click();
	await expect(toast(page, 'Published in Release 3')).toBeVisible();
	await expect(page.getByTestId('entry-state').first()).toHaveAttribute('data-state', 'live');
	expect(await delivered(page, project, entry)).toEqual({ ids: [entry], status: 200 });
});

test('a change made after publishing can be published too: the button comes back once it is saved', async ({ page }) => {
	// Regression: the "Live as Release N" message stayed for the page's life and hid the publish
	// button, so an edit after publishing showed "Changed" with only Unpublish on offer. Now what
	// pressing did is a toast and the status card, and the button returns with the next save.
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/articles?new=1`);
	await page.getByLabel('Title', { exact: true }).fill('First words');
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	const entry = new URL(page.url()).pathname.split('/').at(-1) ?? '';
	await page.getByTestId('publish-button').click();
	await expect(toast(page, 'Published in Release 1')).toBeVisible();
	await expect(page.getByTestId('publish-button')).toHaveCount(0);
	await expect(page.getByTestId('nothing-to-publish')).toBeVisible();

	await page.getByLabel('Title', { exact: true }).fill('Second words');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await expect(page.getByTestId('entry-state').first()).toHaveAttribute('data-state', 'changed');
	await expect(page.getByTestId('publish-state')).toHaveText('Live in Release 1 · edited since');
	await expect(page.getByTestId('publish-button')).toHaveText('Publish changes');
	await page.getByTestId('publish-button').click();
	await expect(toast(page, 'Published in Release 2')).toBeVisible();
	await expect(page.getByTestId('entry-state').first()).toHaveAttribute('data-state', 'live');
	const listed = await page.evaluate(
		async (list) => ((await (await fetch(list, { credentials: 'omit', cache: 'no-store' })).json()) as { data: { id: string; title: string }[] }).data,
		`/content/v1/${project}/production/articles`
	);
	expect(listed).toEqual([expect.objectContaining({ id: entry, title: 'Second words' })]);
	expect(await violations(page)).toEqual([]);
});

test('a collection of 101 entries loads more, and the oldest, published, shows as live in its editor', async ({ page }) => {
	test.setTimeout(120_000);
	await signIn(page);
	const project = await newProject(page);
	const oldest = await newEntry(page, project, 'The oldest one');
	await publishThroughApi(page, project, 'The oldest one', [oldest]);
	const made = await page.evaluate(async (path) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('__Host-colophon_csrf='))?.slice('__Host-colophon_csrf='.length) ?? '';
		const doc = { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } };
		let created = 0;
		for (let batch = 0; batch < 10; batch += 1) {
			const statuses = await Promise.all(
				Array.from({ length: 10 }, async (_, index) => {
					const response = await fetch(path, {
						method: 'POST',
						headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, 'Idempotency-Key': crypto.randomUUID().replaceAll('-', '') },
						body: JSON.stringify({ title: `Filler ${batch * 10 + index + 1}`, body: doc })
					});
					return response.status;
				})
			);
			created += statuses.filter((status) => status === 201).length;
		}
		return created;
	}, `${env(project)}/collections/articles/entries`);
	expect(made).toBe(100);

	await page.goto(`/studio/projects/${project}/articles`);
	const rows = page.getByTestId('entries').getByRole('row');
	await expect(rows).toHaveCount(100);
	await expect(page.getByTestId('entries-shown')).toHaveText('Showing the newest 100.');
	await expect(page.getByTestId('entries')).not.toContainText('The oldest one');
	await page.getByTestId('load-more').click();
	await expect(rows).toHaveCount(101);
	await expect(page.getByTestId('load-more')).toHaveCount(0);
	const row = rows.filter({ hasText: 'The oldest one' });
	await expect(row.getByTestId('entry-state')).toHaveAttribute('data-state', 'live');
	expect(await violations(page)).toEqual([]);

	await row.getByRole('link').click();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await expect(page.getByTestId('publish-state')).toContainText('Live in Release 1');
	await expect(page.getByTestId('entry-state').first()).toHaveAttribute('data-state', 'live');
});
