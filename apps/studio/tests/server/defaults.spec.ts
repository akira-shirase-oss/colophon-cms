// Field defaults (step M.4, D-071) in the Studio, against the real server: set in the builder with
// each field's own kind of input and read back canonical (a web address as the server stores it),
// cleared, not offered on unique fields or inside structures; a value the field doesn't accept
// refused at its input; a new entry opening with the defaults, a required one prefilled in the
// new-entry dialog; and entries written before untouched.

import { expect, type Page, test } from '@playwright/test';
import { api, env, internalTerms, key, newEntry, newProject, signIn, violations } from './helpers.ts';

const THINGS = {
	name: 'Things',
	title_field: 'title',
	fields: [
		{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false },
		{ id: 'status', label: 'Status', type: 'enum', required: true, values: ['draft', 'review', 'done'] },
		{ id: 'price', label: 'Price', type: 'number', required: false, integer: false, min: 0, max: 100 },
		{ id: 'website', label: 'Website', type: 'url', required: false },
		{ id: 'featured', label: 'Featured', type: 'boolean', required: false },
		{ id: 'accent', label: 'Accent', type: 'color', required: false },
		{ id: 'code', label: 'Code', type: 'text', required: false, max_chars: 20, multiline: false, unique: true },
		{ id: 'extra', label: 'Extra', type: 'object', required: false, fields: [{ id: 'note', label: 'Note', type: 'text', required: false, max_chars: 200, multiline: false }] }
	]
};

async function things(page: Page): Promise<{ project: string; before: string }> {
	await signIn(page);
	const project = await newProject(page);
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'things', definition: THINGS }, { 'Idempotency-Key': key() })).status).toBe(201);
	const before = await newEntry(page, project, 'Written before', 'things', { title: 'Written before', status: 'draft' });
	return { project, before };
}

async function fields(page: Page, project: string): Promise<Record<string, any>[]> {
	return (await api(page, 'GET', `${env(project)}/content-types/things`)).body?.['data'].definition.fields as Record<string, any>[];
}

const defaultOf = (page: Page, id: string) => page.getByTestId(`field-${id}`).getByTestId('field-default');

test('defaults are set with each field’s own input, read back canonical, and cleared; not offered where they can’t be', async ({ page }) => {
	const { project } = await things(page);
	await page.goto(`/studio/projects/${project}/settings/types/things`);
	await expect(page.getByTestId('field-code').getByTestId('field-default')).toHaveCount(0);
	await expect(page.getByTestId('field-extra').getByTestId('field-default')).toHaveCount(0);
	await defaultOf(page, 'status').getByRole('combobox').selectOption('review');
	await defaultOf(page, 'price').getByRole('spinbutton').fill('10');
	await defaultOf(page, 'website').getByRole('textbox').fill('HTTPS://Example.com');
	await defaultOf(page, 'featured').getByRole('combobox').selectOption({ label: 'Yes' });
	await defaultOf(page, 'accent').getByLabel('Default value (optional)').fill('#FF8800');
	expect(await violations(page)).toEqual([]);
	await page.getByRole('button', { name: 'Save content type' }).click();
	await expect(page.getByText('Saved Things, version 2')).toBeVisible();
	const saved = await fields(page, project);
	expect(saved.find((item) => item['id'] === 'status')).toMatchObject({ default: 'review' });
	expect(saved.find((item) => item['id'] === 'price')).toMatchObject({ default: 10 });
	expect(saved.find((item) => item['id'] === 'website')).toMatchObject({ default: 'https://example.com/' });
	expect(saved.find((item) => item['id'] === 'featured')).toMatchObject({ default: true });
	expect(saved.find((item) => item['id'] === 'accent')).toMatchObject({ default: '#ff8800' });

	// Read back as stored, and cleared it's left out.
	await page.reload();
	await expect(defaultOf(page, 'website').getByRole('textbox')).toHaveValue('https://example.com/');
	await defaultOf(page, 'price').getByRole('spinbutton').fill('');
	await page.getByRole('button', { name: 'Save content type' }).click();
	await expect(page.getByText('Saved Things, version 3')).toBeVisible();
	expect((await fields(page, project)).find((item) => item['id'] === 'price')).not.toHaveProperty('default');

	// On a phone, the builder's defaults fit.
	await page.setViewportSize({ width: 390, height: 844 });
	await page.waitForLoadState('networkidle');
	expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'the builder scrolls sideways on a phone').toBeLessThanOrEqual(0);
});

test('a default the field doesn’t accept is refused at its input, plainly', async ({ page }) => {
	const { project } = await things(page);
	await page.goto(`/studio/projects/${project}/settings/types/things`);
	await defaultOf(page, 'price').getByRole('spinbutton').fill('500');
	await page.getByRole('button', { name: 'Save content type' }).click();
	const problem = defaultOf(page, 'price').getByTestId('default-problem');
	await expect(problem).toContainText("The default isn't a value this field accepts");
	expect(await violations(page)).toEqual([]);
});

test('a new entry opens with the defaults, a required one prefilled in the dialog; entries written before are untouched', async ({ page }) => {
	const { project, before } = await things(page);
	const definition = structuredClone(THINGS) as typeof THINGS & { fields: Record<string, unknown>[] };
	Object.assign(definition.fields[1] as object, { default: 'review' });
	Object.assign(definition.fields[3] as object, { default: 'HTTPS://Example.com' });
	Object.assign(definition.fields[4] as object, { default: true });
	const changed = await api(page, 'PUT', `${env(project)}/content-types/things`, definition, { 'If-Match': '"v1"', 'Idempotency-Key': key() });
	expect(changed.status, JSON.stringify(changed.body)).toBe(200);

	await page.goto(`/studio/projects/${project}/things?new=1`);
	// Status is required and has a default: the dialog shows it, already chosen.
	await expect(page.getByRole('dialog').getByLabel('Status', { exact: true })).toHaveValue('review');
	await page.getByLabel('Title', { exact: true }).fill('Made with defaults');
	expect(await violations(page)).toEqual([]);
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await expect(page.getByLabel('Status', { exact: true })).toHaveValue('review');
	await expect(page.getByLabel('Website (optional)')).toHaveValue('https://example.com/');
	await expect(page.getByLabel('Featured', { exact: true })).toBeChecked();
	expect(await internalTerms(page)).toEqual([]);
	const entry = new URL(page.url()).pathname.split('/').at(-1) ?? '';
	const stored = (await api(page, 'GET', `${env(project)}/collections/things/entries/${entry}`)).body?.['data'].content;
	expect(stored).toMatchObject({ title: 'Made with defaults', status: 'review', website: 'https://example.com/', featured: true });

	// Written before the defaults: nothing filled in, on reading or saving.
	await page.goto(`/studio/projects/${project}/things/${before}`);
	await expect(page.getByLabel('Website (optional)')).toHaveValue('');
	await expect(page.getByLabel('Featured', { exact: true })).not.toBeChecked();
	const old = (await api(page, 'GET', `${env(project)}/collections/things/entries/${before}`)).body?.['data'].content;
	expect(old).not.toHaveProperty('website');
	expect(old).not.toHaveProperty('featured');

	// On a phone, the new entry fits.
	await page.setViewportSize({ width: 390, height: 844 });
	await page.goto(`/studio/projects/${project}/things/${entry}`);
	await page.waitForLoadState('networkidle');
	expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'the editor scrolls sideways on a phone').toBeLessThanOrEqual(0);
	expect(await violations(page)).toEqual([]);
});
