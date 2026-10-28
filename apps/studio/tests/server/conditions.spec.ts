// Conditional fields (step M.4, D-072) in the Studio, against the real server: "Show only when" set
// in the builder and read back in the other field's order, cleared, never on the title, and a
// field others depend on said to always show; in the editor, a field shown only while its
// condition holds, required only then, and its value cleared (and said so) when it stops
// applying, so a save never earns `not_applicable`; and a condition stored entries don't fit,
// refused with the entries listed.

import { expect, type Page, test } from '@playwright/test';
import { api, applyField, editField, env, internalTerms, key, newEntry, newProject, saveBuilder, signIn, violations } from './helpers.ts';

const media = (conditional: boolean) => ({
	name: 'Media',
	title_field: 'title',
	fields: [
		{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false },
		{ id: 'kind', label: 'Kind', type: 'enum', required: true, values: ['article', 'video', 'podcast'] },
		{
			id: 'duration',
			label: 'Duration',
			type: 'number',
			required: conditional,
			integer: true,
			...(conditional ? { when: { field: 'kind', is: ['video', 'podcast'] } } : {})
		},
		{ id: 'featured', label: 'Featured', type: 'boolean', required: false },
		{ id: 'badge', label: 'Badge', type: 'text', required: false, max_chars: 20, multiline: false, ...(conditional ? { when: { field: 'featured', is: [true] } } : {}) }
	]
});

async function project(page: Page, conditional: boolean): Promise<string> {
	await signIn(page);
	const id = await newProject(page);
	expect((await api(page, 'POST', `${env(id)}/content-types`, { slug: 'media', definition: media(conditional) }, { 'Idempotency-Key': key() })).status).toBe(201);
	return id;
}

async function fields(page: Page, id: string): Promise<Record<string, any>[]> {
	return (await api(page, 'GET', `${env(id)}/content-types/media`)).body?.['data'].definition.fields as Record<string, any>[];
}

/** A field's "Show only when", in its advanced settings (the dialog stays open). */
const when = async (page: Page, id: string) => (await editField(page, id, 'Advanced')).getByTestId('field-when');

test('“Show only when” is set in the builder, read back in the other field’s order, and cleared', async ({ page }) => {
	const id = await project(page, false);
	await page.goto(`/studio/projects/${id}/settings/types/media`);
	await expect(await when(page, 'title')).toHaveCount(0);
	await page.keyboard.press('Escape');
	const duration = await when(page, 'duration');
	await duration.getByRole('combobox').selectOption({ label: 'Kind is…' });
	const values = duration.getByRole('group', { name: 'Values of Kind that show Duration' });
	await expect(values.getByLabel('article')).toBeChecked();
	await values.getByLabel('podcast').check();
	await values.getByLabel('video').check();
	await values.getByLabel('article').uncheck();
	expect(await violations(page)).toEqual([]);
	await applyField(page);
	await expect(page.getByTestId('field-duration')).toContainText('Shown only when Kind is video or podcast');
	// Kind now decides Duration, so Kind always shows.
	await expect(await when(page, 'kind')).toContainText('Duration shows only for some of its values, so this one always shows.');
	await page.keyboard.press('Escape');
	await saveBuilder(page);
	await expect(page.getByText('Saved Media, version 2')).toBeVisible();
	expect((await fields(page, id))[2]).toMatchObject({ id: 'duration', when: { field: 'kind', is: ['video', 'podcast'] } });

	await page.reload();
	const again = await when(page, 'duration');
	await expect(again.getByRole('group').getByLabel('video')).toBeChecked();
	await again.getByRole('combobox').selectOption({ label: 'Always shown' });
	await applyField(page);
	await saveBuilder(page);
	await expect(page.getByText('Saved Media, version 3')).toBeVisible();
	expect((await fields(page, id))[2]).not.toHaveProperty('when');
});

test('a field shows only while its condition holds, is required only then, and is cleared when it stops applying', async ({ page }) => {
	const id = await project(page, true);
	await page.goto(`/studio/projects/${id}/media?new=1`);
	// Duration doesn't apply to an article, so the new entry doesn't ask for it though it's required.
	await expect(page.getByRole('dialog').getByLabel('Duration', { exact: true })).toHaveCount(0);
	await page.getByLabel('Title', { exact: true }).fill('A short film');
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	const status = page.getByTestId('save-status');
	await expect(status).toHaveAttribute('data-status', 'saved');
	const entry = new URL(page.url()).pathname.split('/').at(-1) ?? '';
	await expect(page.getByLabel('Duration', { exact: true })).toHaveCount(0);

	// A video: Duration shows, and is required.
	await page.getByLabel('Kind', { exact: true }).selectOption('video');
	await expect(page.getByLabel('Duration', { exact: true })).toBeVisible();
	await expect(status).toHaveAttribute('data-status', 'refused');
	await expect(page.locator('[data-testid="field-problem"][data-field="duration"]')).toContainText('Duration is required');
	await page.getByLabel('Duration', { exact: true }).fill('12');
	await expect(status).toHaveAttribute('data-status', 'saved');
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);

	// An article again: Duration goes, its value cleared and said so, and the save goes through.
	await page.getByLabel('Kind', { exact: true }).selectOption('article');
	await expect(page.getByLabel('Duration', { exact: true })).toHaveCount(0);
	await expect(page.getByTestId('cleared-notes')).toContainText('Duration is cleared because it only applies when Kind is video or podcast.');
	await expect(status).toHaveAttribute('data-status', 'saved');
	const stored = (await api(page, 'GET', `${env(id)}/collections/media/entries/${entry}`)).body?.['data'].content;
	expect(stored).toMatchObject({ kind: 'article' });
	expect(stored).not.toHaveProperty('duration');

	// A yes-or-no decides too.
	await page.getByLabel('Featured', { exact: true }).check();
	await expect(page.getByLabel('Badge (optional)')).toBeVisible();
	await page.getByLabel('Badge (optional)').fill('New');
	await expect(status).toHaveAttribute('data-status', 'saved');
	await page.getByLabel('Featured', { exact: true }).uncheck();
	await expect(page.getByLabel('Badge (optional)')).toHaveCount(0);
	await expect(page.getByTestId('cleared-notes')).toContainText('Badge is cleared because it only applies when Featured is Yes.');
	await expect(status).toHaveAttribute('data-status', 'saved');

	// On a phone, it all fits.
	await page.setViewportSize({ width: 390, height: 844 });
	await page.waitForLoadState('networkidle');
	expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'the editor scrolls sideways on a phone').toBeLessThanOrEqual(0);
	expect(await violations(page)).toEqual([]);
});

test('a condition stored entries don’t fit is refused with the entries listed', async ({ page }) => {
	const id = await project(page, false);
	await newEntry(page, id, 'An article with a duration', 'media', { title: 'An article with a duration', kind: 'article', duration: 5 });
	await page.goto(`/studio/projects/${id}/settings/types/media`);
	const duration = await when(page, 'duration');
	await duration.getByRole('combobox').selectOption({ label: 'Kind is…' });
	const values = duration.getByRole('group', { name: 'Values of Kind that show Duration' });
	await values.getByLabel('video').check();
	await values.getByLabel('article').uncheck();
	await applyField(page);
	await saveBuilder(page);
	const refusal = page.getByTestId('type-refusal');
	await expect(refusal).toContainText('Some entries don’t fit this condition yet.');
	await expect(refusal.getByRole('link', { name: 'An article with a duration' })).toBeVisible();
	await expect(refusal.getByTestId('blocker-problem')).toHaveText("It has a Duration, which wouldn't apply to it.");
	expect(await violations(page)).toEqual([]);
});
