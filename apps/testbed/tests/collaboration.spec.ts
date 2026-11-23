// Comments and tasks through the testbed, in a real browser (E1, D-102): comment on the entry's
// current revision, refuse a stale etag, resolve and reopen on it, then a task — create it
// assigned, mark it done, reopen it, and unassign it with an explicit `assignee: null`.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function setup(page: Page): Promise<{ project: string }> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const json = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
		const created = await fetch('/server/api/v1/projects', { method: 'POST', headers: json, body: JSON.stringify({ slug, name: 'Collaboration testbed' }) });
		const project = ((await created.json()) as { data: { id: string } }).data.id;
		const body = { title: 'Note under review', body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } };
		await fetch(`/server/api/v1/projects/${project}/environments/production/collections/articles/entries`, {
			method: 'POST',
			headers: { ...json, 'Idempotency-Key': crypto.randomUUID() },
			body: JSON.stringify(body)
		});
		return { project };
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('comment on a revision, resolve and reopen on its etag, and run a task', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const { project } = await setup(page);
	await page.goto('/collaboration');
	await page.getByLabel('Project').selectOption(project);

	// A comment on the revision on screen — the only entry is already picked.
	await page.getByLabel('New comment').fill('Check the opening sentence');
	await page.getByTestId('add-comment').click();
	await expect(page.getByTestId('message')).toHaveText('comment added');
	await page.getByRole('button', { name: 'Check the opening sentence' }).click();

	// A stale etag is refused `comment_moved`; the one just read resolves, then reopens.
	const etag = await page.getByLabel('If-Match etag').inputValue();
	expect(etag).toMatch(/^sha256:[0-9a-f]{64}$/);
	await page.getByLabel('If-Match etag').fill(`sha256:${'0'.repeat(64)}`);
	await page.getByTestId('resolve').click();
	await expect(page.getByTestId('message')).toHaveText('409 conflict (comment_moved)');
	// The 409's refresh puts the comment's etag back in the field when it lands; typing before then races it
	// (it landed mid-fill in CI 44790031484 and the retry sent the etag twice). Type once it has.
	await expect(page.getByLabel('If-Match etag')).toHaveValue(etag);
	await page.getByLabel('If-Match etag').fill(etag);
	await page.getByTestId('resolve').click();
	await expect(page.getByTestId('comment-state')).toHaveText('resolved');
	await page.getByTestId('reopen').click();
	await expect(page.getByTestId('comment-state')).toHaveText('open');

	// A task: created waiting on someone, done on its etag, reopened, then unassigned with null.
	await page.getByLabel('Task title').fill('Fix the byline');
	await page.getByLabel('Waiting on').selectOption({ label: 'Dev Owner' });
	await page.getByTestId('create-task').click();
	await expect(page.getByTestId('message')).toHaveText('task created');
	await page.getByRole('button', { name: 'Fix the byline' }).click();
	await expect(page.getByTestId('task-waiting')).toHaveText('Dev Owner');
	await page.getByTestId('done').click();
	await expect(page.getByTestId('task-status')).toHaveText('done');
	await page.getByTestId('reopen-task').click();
	await expect(page.getByTestId('task-status')).toHaveText('open');
	await page.getByLabel('Reassign to').selectOption('none');
	await page.getByTestId('assign').click();
	await expect(page.getByTestId('task-waiting')).toHaveText('nobody');
});

// Switching project picks that project's own entry: the page used to keep the previous project's entry (it only
// picked one when none was picked), so with another project listed first the comment form never appeared.
test('switching project picks that project\'s entry', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const first = await setup(page);
	const { project } = await setup(page);
	await page.goto('/collaboration');
	await page.getByLabel('Project').selectOption(first.project);
	await expect(page.getByTestId('add-comment')).toBeVisible();
	const before = await page.getByLabel('Entry').inputValue();
	await page.getByLabel('Project').selectOption(project);
	await expect(page.getByLabel('Entry')).not.toHaveValue(before);
	await expect(page.getByTestId('comments')).toBeVisible();
	await expect(page.getByTestId('add-comment')).toBeVisible();
});

// A refresh started before the resolve can't overwrite it: the reload after the stale etag's 409 is held back (its
// tasks read answers 2 s late) until the resolve and its own reload have landed. The page must keep "resolved" —
// a load that a newer one has overtaken is dropped, not applied last (CI 44560804246's flake, made certain).
test('a reload overtaken by a newer one is dropped, so a resolve stays resolved', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const { project } = await setup(page);
	let refused = false;
	let held = false;
	let released = false;
	page.on('response', (response) => {
		if (response.url().includes('/resolve') && response.status() === 409) refused = true;
	});
	await page.route('**/tasks?entry=*', async (route) => {
		if (refused && !held) {
			held = true;
			await new Promise((resolve) => setTimeout(resolve, 2000));
			await route.continue();
			released = true;
			return;
		}
		await route.continue();
	});
	await page.goto('/collaboration');
	await page.getByLabel('Project').selectOption(project);
	await page.getByLabel('New comment').fill('Check the closing sentence');
	await page.getByTestId('add-comment').click();
	await expect(page.getByTestId('message')).toHaveText('comment added');
	await page.getByRole('button', { name: 'Check the closing sentence' }).click();

	const etag = await page.getByLabel('If-Match etag').inputValue();
	await page.getByLabel('If-Match etag').fill(`sha256:${'0'.repeat(64)}`);
	await page.getByTestId('resolve').click();
	await expect(page.getByTestId('message')).toHaveText('409 conflict (comment_moved)');
	await page.getByLabel('If-Match etag').fill(etag);
	await page.getByTestId('resolve').click();
	await expect(page.getByTestId('message')).toHaveText('resolved');
	await expect(page.getByTestId('comment-state')).toHaveText('resolved');
	// The held reload lands now, last; it must change nothing.
	await expect.poll(() => released, { timeout: 5000 }).toBe(true);
	expect(held).toBe(true);
	await page.waitForTimeout(300);
	await expect(page.getByTestId('comment-state')).toHaveText('resolved');
});
