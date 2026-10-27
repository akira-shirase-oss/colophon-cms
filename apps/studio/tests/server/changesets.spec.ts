// Changesets in the Studio against a real server (D-044): pick entries, submit, approve that exact
// version (a self-review, shown as one), see an edit make it stale and a refresh re-pin it, and
// see a second tab's approval refused because the version it shows is no longer current.

import { type Page, expect, test } from '@playwright/test';
import { newProject, signIn, violations } from './helpers.ts';

/** Creates an article through the API as the signed-in page; its id. */
async function article(page: Page, project: string, title: string): Promise<string> {
	return page.evaluate(
		async ([project, title]) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const body = {
				title,
				body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Hello.' }] }] } }
			};
			const response = await fetch(`/api/v1/projects/${project}/environments/production/collections/articles/entries`, {
				method: 'POST',
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, 'Idempotency-Key': crypto.randomUUID().replaceAll('-', '') },
				body: JSON.stringify(body)
			});
			return ((await response.json()) as { data: { id: string } }).data.id;
		},
		[project, title]
	);
}

/** Saves a new title on the article through the API, as another tab would. */
async function retitle(page: Page, project: string, entry: string, title: string) {
	const status = await page.evaluate(
		async ([project, entry, title]) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const path = `/api/v1/projects/${project}/environments/production/collections/articles/entries/${entry}`;
			const current = await fetch(path);
			const draft = ((await current.json()) as { data: { content: Record<string, unknown> } }).data;
			const saved = await fetch(path, {
				method: 'PUT',
				headers: {
					'Content-Type': 'application/json',
					'X-CSRF-Token': csrf,
					'If-Match': current.headers.get('etag') ?? '',
					'Idempotency-Key': crypto.randomUUID().replaceAll('-', '')
				},
				body: JSON.stringify({ ...draft.content, title })
			});
			return saved.status;
		},
		[project, entry, title]
	);
	expect(status).toBe(200);
}

const status = (page: Page) => page.getByTestId('changeset-status');

test('a changeset is approved as the exact version on screen, and an edit makes it stale', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	const first = await article(page, project, 'Spring launch');
	await article(page, project, 'Pricing page');

	await page.goto(`/studio/projects/${project}`);
	await page.getByTestId('changesets-link').click();
	await page.getByRole('button', { name: 'New changeset' }).click();
	await page.getByLabel('Title', { exact: true }).fill('Launch week');
	await page.getByLabel('Spring launch', { exact: true }).check();
	await page.getByLabel('Pricing page', { exact: true }).check();
	expect(await violations(page)).toEqual([]);
	await page.getByRole('button', { name: 'Create changeset' }).click();
	await expect(status(page)).toHaveAttribute('data-state', 'open');
	await expect(page.getByTestId('changeset-items').getByRole('listitem')).toHaveCount(2);
	const changesetPath = new URL(page.url()).pathname;

	await page.getByRole('button', { name: 'Submit for review' }).click();
	await expect(status(page)).toHaveAttribute('data-state', 'awaiting_review');
	await page.getByLabel('Comment (optional)').fill('Looks right.');
	await page.getByRole('button', { name: 'Approve this version' }).click();
	await expect(status(page)).toHaveAttribute('data-state', 'approved');
	await expect(page.getByTestId('self-review')).toBeVisible();
	expect(await violations(page)).toEqual([]);

	// An edit after approval: the changeset is stale until refreshed, and the approval stops counting.
	await retitle(page, project, first, 'Spring launch, final');
	await page.reload();
	await expect(status(page)).toHaveAttribute('data-state', 'stale');
	await expect(page.getByTestId('edited')).toHaveCount(1);
	await expect(page.getByRole('button', { name: 'Approve this version' })).toHaveCount(0);
	await page.getByRole('button', { name: 'Refresh to the current versions' }).click();
	await expect(status(page)).toHaveAttribute('data-state', 'awaiting_review');
	await expect(page.getByTestId('changeset-items')).toContainText('Spring launch, final');
	await expect(page.getByTestId('changeset-reviews')).toContainText('on an earlier version');

	// A second tab still shows this version when the first edits and refreshes again: its
	// approval is refused, and it shows the current version instead.
	const other = await (await browser.newContext()).newPage();
	await signIn(other);
	await other.goto(changesetPath);
	await expect(status(other)).toHaveAttribute('data-state', 'awaiting_review');
	await retitle(page, project, first, 'Spring launch, really final');
	await page.reload();
	await page.getByRole('button', { name: 'Refresh to the current versions' }).click();
	await expect(status(page)).toHaveAttribute('data-state', 'awaiting_review');
	await other.getByRole('button', { name: 'Approve this version' }).click();
	await expect(other.getByTestId('changeset-notice')).toContainText('changed since you loaded it');
	await expect(other.getByTestId('changeset-items')).toContainText('Spring launch, really final');
	await expect(status(other)).toHaveAttribute('data-state', 'awaiting_review');
});
