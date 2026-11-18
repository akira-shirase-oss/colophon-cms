// An entry's language and its translations (D-087, D-184): one Language row in the side panel,
// "Default language ▾ · Translate…". Translate… names this entry's own language first when it
// has none and makes the translation in one step, which opens as its own entry; the collection
// list badges the tags; no button the editor enables is refused with a 409 (F4) — languages the
// group already has aren't offered, and a malformed tag is never sent. One-entry types (a theme,
// a menu) have no Language row.

import { expect, type Response, test } from '@playwright/test';
import { api, env, key, newProject, signIn, toast, violations } from './helpers.ts';

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

test('Translate… names the entry’s language and makes the translation in one step, without a 409', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const base = env(project);
	expect((await api(page, 'POST', `${base}/content-types`, NOTES, { 'Idempotency-Key': key() })).status).toBe(201);
	const made = await api(page, 'POST', `${base}/collections/notes/entries`, { title: 'Welcome', note: 'hi' }, { 'Idempotency-Key': key() });
	expect(made.status).toBe(201);
	const entry = (made.body?.['data'] as { id: string }).id;
	// Every answer the Studio gets from here on: none may be a 409 (F4).
	const refused: string[] = [];
	page.on('response', (response: Response) => {
		if (response.status() === 409) refused.push(`${response.request().method()} ${response.url()}`);
	});

	await page.goto(`/studio/projects/${project}/notes/${entry}`);
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	const language = page.getByTestId('entry-language');
	await expect(language.getByTestId('entry-locale')).toHaveText('Default language');
	// No Locale input and no Versions card any more: one row.
	await expect(page.getByRole('textbox', { name: /locale/i })).toHaveCount(0);

	// The entry has no language: the dialog asks for it first, starting from the browser's.
	await language.getByTestId('translate').click();
	const dialog = page.getByTestId('language-dialog');
	await expect(dialog.getByRole('heading', { name: 'Translate “Welcome”' })).toBeVisible();
	await expect(dialog.getByTestId('language-source')).toHaveValue('en');
	// Languages are offered by name, with the tag after.
	await expect(dialog.getByTestId('language-target').locator('option', { hasText: 'French — fr' })).toHaveCount(1);
	// Not into the entry's own language.
	await expect(dialog.getByTestId('language-target').locator('option[value="en"]')).toHaveCount(0);
	await dialog.getByTestId('language-target').selectOption('fr');
	expect(await violations(page)).toEqual([]);
	await dialog.getByTestId('language-go').click();

	// It opens the translation, which keeps the source's draft for its own words.
	await page.waitForURL(new RegExp(`/studio/projects/${project}/notes/(?!${entry})[0-9a-f-]+$`));
	const sibling = page.url().split('/').at(-1) ?? '';
	await expect(toast(page, 'Made the French translation')).toBeVisible();
	await expect(page.getByTestId('entry-locale')).toHaveText('French — fr');
	await expect(page.getByRole('textbox', { name: 'Title' })).toHaveValue('Welcome');
	await page.getByRole('textbox', { name: 'Title' }).fill('Bienvenue');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');

	// The menu lists the group; the source became English on the way.
	await page.getByTestId('language-menu').click();
	await expect(page.getByTestId('translation-this')).toContainText('French — fr');
	await page.getByTestId('translation').filter({ hasText: 'English — en' }).click();
	await expect(page).toHaveURL(new RegExp(`/studio/projects/${project}/notes/${entry}$`));
	await expect(page.getByTestId('entry-locale')).toHaveText('English — en');

	// Translating again doesn't offer the languages the group has.
	await page.getByTestId('translate').click();
	await expect(dialog.getByTestId('language-source')).toHaveCount(0);
	await expect(dialog.getByTestId('language-target').locator('option[value="fr"]')).toHaveCount(0);
	await expect(dialog.getByTestId('language-target').locator('option[value="en"]')).toHaveCount(0);
	// A typed tag is checked as the server checks it before the button is enabled.
	await dialog.getByTestId('language-target').selectOption('*');
	await dialog.getByTestId('language-tag').fill('not a tag');
	await expect(dialog.getByTestId('language-go')).toBeDisabled();
	await expect(dialog.getByTestId('language-waiting')).toContainText('Write a language tag');
	await dialog.getByTestId('language-tag').fill('FR');
	await expect(dialog.getByTestId('language-go')).toBeDisabled();
	await expect(dialog.getByTestId('language-waiting')).toContainText('already a translation in that language');
	await dialog.getByRole('button', { name: 'Cancel' }).click();

	// A grouped entry keeps a language: the default isn't offered when changing it.
	await page.getByTestId('language-menu').click();
	await page.getByTestId('change-language').click();
	await expect(dialog.getByTestId('language-target').locator('option[value=""]')).toBeDisabled();
	await expect(dialog.getByTestId('language-target').locator('option[value="fr"]')).toBeDisabled();
	await dialog.getByRole('button', { name: 'Cancel' }).click();

	// The collection list badges the tags.
	await page.goto(`/studio/projects/${project}/notes`);
	await expect(page.getByRole('link', { name: 'Welcome' })).toBeVisible();
	await expect(page.locator('[data-slot="badge"]', { hasText: 'en' })).toBeVisible();
	const listed = await api(page, 'GET', `${base}/collections/notes/entries?locale=fr`);
	expect((listed.body?.['data'] as { id: string }[]).map((row) => row.id)).toContain(sibling);
	expect(refused).toEqual([]);
});

test('an entry’s language changes through the menu and saves with the draft; one-entry types have none', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const base = env(project);
	expect((await api(page, 'POST', `${base}/content-types`, NOTES, { 'Idempotency-Key': key() })).status).toBe(201);
	const made = await api(page, 'POST', `${base}/collections/notes/entries`, { title: 'Hallo' }, { 'Idempotency-Key': key() });
	const entry = (made.body?.['data'] as { id: string }).id;
	await page.goto(`/studio/projects/${project}/notes/${entry}`);
	await page.getByTestId('language-menu').click();
	await page.getByTestId('change-language').click();
	const dialog = page.getByTestId('language-dialog');
	await dialog.getByTestId('language-target').selectOption('de');
	await dialog.getByTestId('language-go').click();
	await expect(dialog).toBeHidden();
	await expect(page.getByTestId('entry-locale')).toHaveText('German — de');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	const read = await api(page, 'GET', `${base}/collections/notes/entries/${entry}`);
	expect(read.body?.['data'].locale).toBe('de');

	// A one-entry type: no Language row (a theme or a menu isn't translated entry by entry).
	const single = { slug: 'banner', definition: { name: 'Banner', title_field: 'title', single: true, fields: [{ id: 'title', label: 'Title', type: 'text', required: true }] } };
	expect((await api(page, 'POST', `${base}/content-types`, single, { 'Idempotency-Key': key() })).status).toBe(201);
	const banner = await api(page, 'POST', `${base}/collections/banner/entries`, { title: 'Spring sale' }, { 'Idempotency-Key': key() });
	await page.goto(`/studio/projects/${project}/banner/${(banner.body?.['data'] as { id: string }).id}`);
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await expect(page.getByTestId('entry-language')).toHaveCount(0);
	expect(await violations(page)).toEqual([]);
});
