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
