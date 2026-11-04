// Comments and tasks in the Studio against a real server (E1, D-102): a comment on the version
// on screen, resolved and reopened on its etag; a task made on an entry, assigned, finished, and
// unassigned — nobody sent as null, never left out; and a deleted entry's comments stay readable
// with nothing new to add.

import { expect, test } from '@playwright/test';
import { api, env, internalTerms, key, newEntry, newProject, signIn, violations } from './helpers.ts';

test('comment on the version on screen, and a task through its life', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Winter note');

	// A comment on the entry's current version, written and answered in the editor's Comments tab.
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await page.getByRole('tab', { name: 'Comments' }).click();
	await page.getByTestId('comment-input').fill('Check the opening sentence');
	await page.getByTestId('comment-post').click();
	const comment = page.getByTestId('comment').first();
	await expect(comment).toContainText('Check the opening sentence');
	await expect(comment).toHaveAttribute('data-resolved', 'false');
	await comment.getByTestId('comment-resolve').click();
	await expect(comment).toHaveAttribute('data-resolved', 'true');
	await comment.getByTestId('comment-reopen').click();
	await expect(comment).toHaveAttribute('data-resolved', 'false');
	expect(await internalTerms(page)).toEqual([]);
	expect(await violations(page)).toEqual([]);

	// A deleted entry's comments stay: delete it through the API (on its own etag, as deletes
	// bind). The editor's read freezes where it stood — the tab a hint never reaches — so its
	// composer learns only when a write answers not-found: the panel closes, what was said stays.
	const read = `${env(project)}/collections/articles/entries/${entry}`;
	const frozen = await page.evaluate(async (path) => {
		const response = await fetch(path);
		return { etag: response.headers.get('etag'), body: await response.text() };
	}, read);
	await page.route(`**/collections/articles/entries/${entry}`, (route) =>
		route.request().method() === 'GET'
			? route.fulfill({ status: 200, contentType: 'application/json', headers: { etag: frozen.etag ?? '' }, body: frozen.body })
			: route.continue()
	);
	const gone = await api(page, 'DELETE', read, undefined, {
		'Idempotency-Key': key(),
		'If-Match': frozen.etag ?? ''
	});
	expect([200, 204]).toContain(gone.status);
	await page.getByTestId('comment-input').fill('One more thing');
	await page.getByTestId('comment-post').click();
	await expect(page.getByTestId('comments-closed')).toBeVisible();
	await expect(comment).toContainText('Check the opening sentence');

	// Unfrozen, the hint's refetch — and every fresh visit — answers the same truth: the
	// deleted entry's page becomes "Not found" without a reload (E3 live lists).
	await page.unroute(`**/collections/articles/entries/${entry}`);
	await page.reload();
	await expect(page.getByRole('alert')).toContainText('Not found');
});

test('a task is made on an entry, assigned, finished, and unassigned', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Winter note');

	await page.goto(`/studio/projects/${project}/tasks`);
	await page.getByTestId('new-task').click();
	await page.getByTestId('task-title').fill('Fix the byline');
	await page.getByTestId('task-entry').selectOption(`articles/${entry}`);
	await page.getByTestId('task-assignee').selectOption({ label: 'You' });
	await page.getByTestId('task-create').click();

	// The detail opens on what was made: waiting on you, open, and editable.
	const detail = page.getByTestId('task-detail');
	await expect(detail).toBeVisible();
	await expect(detail.getByTestId('detail-status')).toHaveText('open');
	await detail.getByTestId('task-done').click();
	await expect(detail.getByTestId('detail-status')).toHaveText('done');
	await detail.getByTestId('task-reopen').click();
	await expect(detail.getByTestId('detail-status')).toHaveText('open');

	// Unassigned is an explicit choice: it sends assignee null, never an omitted field.
	await detail.getByTestId('edit-assignee').selectOption('');
	await expect(detail.getByTestId('edit-assignee')).toHaveValue('');
	const tasks = await api(page, 'GET', `${env(project)}/tasks?entry=${entry}`);
	expect(tasks.status).toBe(200);
	expect(tasks.body?.['data'][0].assignee_id).toBeNull();

	// The list shows it the same way.
	await page.keyboard.press('Escape');
	await expect(page.getByTestId('task').first()).toContainText('Fix the byline');
	expect(await internalTerms(page)).toEqual([]);
	expect(await violations(page)).toEqual([]);
});
