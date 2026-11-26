// A collection's drafts leave and return as files (D-086): the collection page's Export
// downloads every draft as JSONL, and Import reads a file of entries back through the same
// page — a bad file is refused in the page's own words, not left half-written.

import { expect, test } from '@playwright/test';
import { api, env, internalTerms, key, newProject, signIn, violations } from './helpers.ts';

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

// Import content (the project-level page): Markdown files go through the real check and the real
// import. The mocked journeys in tests/import-page.spec.ts cover the page's states; this one
// proves the documents it builds are ones the server takes, and that what lands is a draft.
const POSTS = {
	slug: 'posts',
	definition: {
		name: 'Posts',
		title_field: 'title',
		fields: [
			{ id: 'title', label: 'Title', type: 'text', required: true },
			{ id: 'slug', label: 'URL name', type: 'slug', required: true },
			{ id: 'pub_date', label: 'Published on', type: 'date' },
			{ id: 'body', label: 'Body', type: 'markdown' }
		]
	}
};

test('Import content brings Markdown files in as drafts, after a check that writes nothing', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const base = env(project);
	expect((await api(page, 'POST', `${base}/content-types`, POSTS, { 'Idempotency-Key': key() })).status).toBe(201);

	// The sidebar leads there; the schema page is named for what it is and points here too.
	await page.goto(`/studio/projects/${project}/migrations`);
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Content type changes');
	await page.getByTestId('migrations-import-note').getByRole('link', { name: 'Import content' }).click();
	await expect(page).toHaveURL(new RegExp(`/studio/projects/${project}/import$`));
	await expect(page.getByTestId('import-link')).toBeVisible();

	await page.getByTestId('source-markdown').check();
	await page.getByTestId('import-type').selectOption('posts');
	await page.getByTestId('import-file').setInputFiles([
		{
			name: 'hello-world.md',
			mimeType: 'text/markdown',
			buffer: Buffer.from('---\ntitle: Hello world\npubDate: 2024-05-01T10:00:00Z\nheroImage: ./hero.jpg\n---\n\n# Hello\r\n\r\nFirst post.\n')
		},
		{ name: 'untitled.md', mimeType: 'text/markdown', buffer: Buffer.from('No front matter here.\n') }
	]);
	await expect(page.getByTestId('import-found')).toContainText('2 files read: 2 entries.');
	await expect(page.getByTestId('import-left-out')).toContainText('heroImage');

	// The check: one will be created, one can't be, and nothing was written.
	await page.getByTestId('import-check').click();
	await expect(page.getByTestId('will-create')).toHaveText('1');
	await expect(page.getByTestId('will-refuse')).toHaveText('1');
	await expect(page.getByTestId('import-problems')).toContainText('untitled.md: Title is required.');
	expect(await internalTerms(page)).toEqual([]);
	expect(await violations(page)).toEqual([]);
	const before = await api(page, 'GET', `${base}/collections/posts/entries?limit=100`);
	expect(before.body?.['data']).toEqual([]);

	// The import: the one the check accepted, as a draft with the front matter's fields, the
	// body in the Markdown field, a date for the date field and the URL name from the file's name.
	await page.getByTestId('import-apply').click();
	await expect(page.getByTestId('import-done')).toContainText('Imported 1 entry');
	const listed = await api(page, 'GET', `${base}/collections/posts/entries?limit=100`);
	const rows = listed.body?.['data'] as { id: string; published: unknown }[];
	expect(rows).toHaveLength(1);
	expect(rows[0]?.published).toBeNull();
	const entry = await api(page, 'GET', `${base}/collections/posts/entries/${rows[0]?.id}`);
	expect(entry.body?.['data'].content).toEqual({ title: 'Hello world', slug: 'hello-world', pub_date: '2024-05-01', body: '# Hello\n\nFirst post.' });

	// The link opens the collection, where the draft is.
	await page.getByTestId('import-open').click();
	await expect(page.getByRole('link', { name: 'Hello world' })).toBeVisible();
});
