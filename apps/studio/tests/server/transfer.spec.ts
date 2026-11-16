// A collection's drafts leave and return as files (D-086): the collection page's Export
// downloads every draft as JSONL, and Import reads a file of entries back through the same
// page — a bad file is refused in the page's own words, not left half-written.

import { expect, test } from '@playwright/test';
import { api, env, key, newProject, signIn, violations } from './helpers.ts';

const NOTES = {
	slug: 'notes',
	definition: {
		name: 'Notes',
		title_field: 'title',
		fields: [
			{ id: 'title', label: 'Title', type: 'text', required: true },
			{ id: 'note', label: 'Note', type: 'text' }
		]
	}
};

test('a collection page exports its drafts and imports a file of new ones', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const base = env(project);
	expect((await api(page, 'POST', `${base}/content-types`, NOTES, { 'Idempotency-Key': key() })).status).toBe(201);
	for (const title of ['First', 'Second']) {
		const made = await api(page, 'POST', `${base}/collections/notes/entries`, { title, note: title.toLowerCase() }, { 'Idempotency-Key': key() });
		expect(made.status).toBe(201);
	}

	await page.goto(`/studio/projects/${project}/notes`);

	// Export downloads every draft as JSONL, newest first, named for the collection.
	const [download] = await Promise.all([page.waitForEvent('download'), page.getByTestId('export-entries').click()]);
	expect(download.suggestedFilename()).toBe('notes.jsonl');
	const file = await download.path();
	const { readFileSync } = await import('node:fs');
	const lines = readFileSync(file, 'utf8').split('\n').filter((line) => line !== '');
	expect(lines).toHaveLength(2);
	expect(lines[0]).toContain('"title":"Second"');
	for (const line of lines) expect(() => JSON.parse(line)).not.toThrow();

	// Import reads a file back: a toast counts it and the rows appear.
	await page.getByTestId('import-file').setInputFiles({
		name: 'notes.jsonl',
		mimeType: 'application/jsonlines',
		buffer: Buffer.from('{"title":"Third","note":"three"}\n{"title":"Fourth"}\n')
	});
	await expect(page.getByText('Imported 2 entries')).toBeVisible();
	await expect(page.getByRole('link', { name: 'Third' })).toBeVisible();
	await expect(page.getByRole('link', { name: 'Fourth' })).toBeVisible();
	expect(await violations(page)).toEqual([]);

	// A file that fails validation is refused where it fails — and nothing of it lands.
	await page.getByTestId('import-file').setInputFiles({
		name: 'bad.jsonl',
		mimeType: 'application/jsonlines',
		buffer: Buffer.from('{"title":"Fine"}\n{"note":"no title"}\n')
	});
	await expect(page.getByText('Title is required.')).toBeVisible();
	const listed = await api(page, 'GET', `${base}/collections/notes/entries?limit=100`);
	const titles = (listed.body?.['data'] as { revision: { title: string } }[]).map((entry) => entry.revision.title);
	expect(titles).toHaveLength(4);
	expect(titles).not.toContain('Fine');
});
