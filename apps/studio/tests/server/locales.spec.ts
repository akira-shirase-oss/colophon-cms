// An entry carries a locale tag (D-087): the editor's Locale input saves it with the draft,
// the collection list badges it, and the Versions card adds a sibling in another locale —
// which opens as its own entry — and refuses a tag the group already holds.

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

test('an entry takes a locale and grows a versions group', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const base = env(project);
	expect((await api(page, 'POST', `${base}/content-types`, NOTES, { 'Idempotency-Key': key() })).status).toBe(201);
	const made = await api(page, 'POST', `${base}/collections/notes/entries`, { title: 'Welcome', note: 'hi' }, { 'Idempotency-Key': key() });
	expect(made.status).toBe(201);
	const entry = (made.body?.['data'] as { id: string }).id;

	await page.goto(`/studio/projects/${project}/notes/${entry}`);

	// The Locale input saves with the draft like any change.
	const locale = page.getByTestId('entry-locale').getByRole('textbox');
	await locale.fill('en');
	await locale.blur();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await expect(page.getByTestId('versions').getByText('No other versions')).toBeVisible();

	// The collection list badges the tag.
	await page.goto(`/studio/projects/${project}/notes`);
	await expect(page.getByRole('link', { name: 'Welcome' })).toBeVisible();
	await expect(page.locator('[data-slot="badge"]', { hasText: 'en' })).toBeVisible();

	// Add a French version: it opens as its own entry, and both sides list the group.
	await page.goto(`/studio/projects/${project}/notes/${entry}`);
	await page.getByTestId('version-locale').fill('fr');
	await page.getByTestId('add-version').click();
	await page.waitForURL(new RegExp(`/studio/projects/${project}/notes/(?!${entry})[0-9a-f-]+`));
	const sibling = page.url().split('/').at(-1)!;
	await expect(page.getByTestId('versions').getByText('This entry')).toBeVisible();
	await expect(page.getByTestId('version-list').getByRole('link', { name: 'en' })).toBeVisible();
	await expect(page.getByTestId('entry-locale').getByRole('textbox')).toHaveValue('fr');

	// The sibling keeps the source's draft — the French title gets its own save.
	await page.getByRole('textbox', { name: 'Title' }).fill('Bienvenue');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');

	// Back on the source, the group lists the sibling; a taken tag is refused in place.
	await page.getByTestId('version-list').getByRole('link', { name: 'en' }).click();
	await expect(page).toHaveURL(new RegExp(`/studio/projects/${project}/notes/${entry}$`));
	await expect(page.getByTestId('version-list').getByRole('link', { name: 'fr' })).toBeVisible();
	await page.getByTestId('version-locale').fill('en');
	await page.getByTestId('add-version').click();
	await expect(page.getByTestId('version-problem')).toBeVisible();
	await expect(page.getByTestId('version-problem')).toContainText('locale already exists');

	// A malformed tag is refused by the save, not accepted.
	await page.getByTestId('entry-locale').getByRole('textbox').fill('not a tag');
	await page.getByTestId('entry-locale').getByRole('textbox').blur();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'refused');
	expect(await violations(page)).toEqual([]);

	const listed = await api(page, 'GET', `${base}/collections/notes/entries?locale=fr`);
	expect(listed.status).toBe(200);
	expect((listed.body?.['data'] as { id: string }[]).map((row) => row.id)).toContain(sibling);
});
