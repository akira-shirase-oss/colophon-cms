// Live lists in the Studio against a real server (E3, D-108): the project shell holds a
// {P}/events stream and turns each commit's hint into the queries it makes stale — another
// member's entry appears in the list without a reload, and a save on an entry being edited
// surfaces the conflict screen, never a silent overwrite.

import { expect, test } from '@playwright/test';
import { api, env, key, member, newEntry, newProject, signIn } from './helpers.ts';

test("a member's commit lands in the owner's list live, and a moved draft opens the conflict screen", async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	const other = await member(page, browser, project, 'editor', 'dev-editor');

	// The owner's articles list is open with the stream behind it (the response's headers
	// arrive only once the subscription exists — after it, a commit is heard).
	const opened = page.waitForResponse((response) => response.url().includes('/events') && response.status() === 200);
	await page.goto(`/studio/projects/${project}/articles`);
	await opened;
	await expect(page.getByTestId('entries-empty')).toBeVisible();

	// The member's first entry commits: the hint makes the list stale and it refetches.
	const entry = await newEntry(other, project, 'Member note');
	await expect(page.getByTestId('entries')).toContainText('Member note');

	// The owner opens it to edit; meanwhile the member saves a newer version.
	await page.getByRole('link', { name: 'Member note' }).click();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	const etag = await other.evaluate(async (path) => (await fetch(path)).headers.get('etag'), `${env(project)}/collections/articles/entries/${entry}`);
	expect(etag).toBeTruthy();
	const moved = await api(
		other,
		'PUT',
		`${env(project)}/collections/articles/entries/${entry}`,
		{
			title: 'Member note — moved on',
			body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Saved by the member.' }] }] } }
		},
		{ 'If-Match': etag ?? '', 'Idempotency-Key': key() }
	);
	expect(moved.status, JSON.stringify(moved.body)).toBe(200);

	// The draft's own read refreshed live: the header names the member's version. The form's
	// contents are untouched — an editor's work is never swapped out under their hands.
	await expect(page.getByRole('heading', { name: 'Member note — moved on' })).toBeVisible();

	// The owner's next autosave goes against the revision it read and is refused: the
	// conflict screen shows both versions — theirs and mine — so nobody is overwritten.
	await page.getByRole('textbox', { name: 'Body' }).click();
	await page.keyboard.type('Owner edits.');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'conflict');
	const conflict = page.getByTestId('conflict');
	await expect(conflict.getByTestId('theirs')).toContainText('Saved by the member.');
	await expect(conflict.getByTestId('mine')).toContainText('Owner edits.');
});
