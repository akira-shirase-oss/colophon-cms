// The collection table's bounded bulk actions: pick entries and publish them together, in one
// publish request (one click when the role may approve and publish its own requests; a request
// sent for review otherwise); and duplicate an entry, whose copy gets the next free URL name.

import { expect, test } from '@playwright/test';
import { api, env, internalTerms, key, member, newEntry, newProject, signIn, violations } from './helpers.ts';

test('three picked entries are published together, in one publish request', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	for (const title of ['One of three', 'Two of three', 'Three of three', 'Left alone']) await newEntry(page, project, title);
	await page.goto(`/studio/projects/${project}/articles`);
	const table = page.getByTestId('entries');
	for (const title of ['One of three', 'Two of three', 'Three of three']) await table.getByRole('checkbox', { name: `Pick ${title}` }).check();
	await expect(page.getByTestId('bulk-bar')).toContainText('3 picked');
	await expect(page.getByTestId('bulk-bar')).toContainText('Goes live together, as one release.');
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);
	await page.getByTestId('bulk-publish').click();
	await expect(page.getByTestId('bulk-outcome')).toContainText('Live as Release 1');
	await expect(page.getByTestId('bulk-bar')).toHaveCount(0);
	for (const title of ['One of three', 'Two of three', 'Three of three']) {
		await expect(table.getByRole('row').filter({ hasText: title }).getByTestId('entry-state')).toHaveAttribute('data-state', 'live');
	}
	await expect(table.getByRole('row').filter({ hasText: 'Left alone' }).getByTestId('entry-state')).toHaveAttribute('data-state', 'draft');

	const requests = await api(page, 'GET', `${env(project)}/changesets`);
	const made = requests.body?.['data'] as { title: string; items: number; status: { state: string } }[];
	expect(made).toHaveLength(1);
	expect(made[0]).toMatchObject({ items: 3, status: { state: 'published' }, title: 'Three of three and 2 more' });

	// Someone who can't approve sends the picked entries for review instead.
	const editor = await member(page, browser, project, 'editor');
	await newEntry(editor, project, 'For review A');
	await newEntry(editor, project, 'For review B');
	await editor.goto(`/studio/projects/${project}/articles`);
	await editor.getByRole('checkbox', { name: 'Pick For review A' }).check();
	await editor.getByRole('checkbox', { name: 'Pick For review B' }).check();
	await expect(editor.getByTestId('bulk-publish')).toHaveText('Ask for review of 2');
	await editor.getByTestId('bulk-publish').click();
	await expect(editor.getByTestId('bulk-outcome')).toContainText('Sent for review');
	await editor.getByTestId('bulk-outcome').getByRole('link').click();
	await expect(editor.getByTestId('changeset-status')).toHaveAttribute('data-state', 'awaiting_review');
	await expect(editor.getByTestId('changeset-items').getByRole('listitem')).toHaveCount(2);
	await editor.context().close();
});

test('a duplicate gets “(copy)” and the next free URL name, and opens in the editor', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const definition = {
		name: 'Pages',
		title_field: 'title',
		fields: [
			{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false },
			{ id: 'slug', label: 'URL name', type: 'slug', required: true, source: 'title' }
		]
	};
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'pages', definition }, { 'Idempotency-Key': key() })).status).toBe(201);
	await newEntry(page, project, 'Hello world', 'pages', { title: 'Hello world', slug: 'hello-world' });
	await newEntry(page, project, 'Hello world again', 'pages', { title: 'Hello world again', slug: 'hello-world-2' });

	await page.goto(`/studio/projects/${project}/pages`);
	await page.getByRole('button', { name: 'Duplicate Hello world', exact: true }).click();
	await expect(page.getByLabel('Title', { exact: true })).toHaveValue('Hello world (copy)');
	await expect(page.getByLabel('URL name')).toHaveValue('hello-world-3');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await page.goto(`/studio/projects/${project}/pages`);
	await expect(page.getByTestId('entries').getByRole('row')).toHaveCount(3);
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);
});
