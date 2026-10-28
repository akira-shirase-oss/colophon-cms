// Allowed forms of text (step M.4, D-069) in the Studio, against the real server: set in the builder
// on a text field (and on a list's text items), kept when other options change, and a form the
// server can't use refused at its own input; in the editor, a value not in its allowed form is
// refused by the server and said under the field with its help text, inside a list too; and adding
// a form that stored content doesn't fit lists the entries in the way.

import { expect, type Page, test } from '@playwright/test';
import { api, applyField, editField, env, internalTerms, key, newContentType, newEntry, newProject, saveBuilder, signIn, startField, violations } from './helpers.ts';

const CODES = {
	name: 'Products',
	title_field: 'title',
	fields: [
		{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false },
		{ id: 'code', label: 'Code', type: 'text', required: false, max_chars: 20, multiline: false, pattern: '[A-Z]{3}-[0-9]{4}', description: 'Like ABC-1234' },
		{ id: 'tags', label: 'Tags', type: 'list', required: false, max: 5, of: { type: 'text', max_chars: 40, multiline: false, pattern: '[a-z]+' } }
	]
};

async function definition(page: Page, project: string, slug: string): Promise<Record<string, any>[]> {
	return (await api(page, 'GET', `${env(project)}/content-types/${slug}`)).body?.['data'].definition.fields as Record<string, any>[];
}

test('an allowed form is set in the builder, kept when other options change, and refused at its input when unusable', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await newContentType(page, project, 'Products');
	const dialog = await startField(page, 'Code', 'text');
	await dialog.getByTestId('field-description').fill('Like ABC-1234');
	await dialog.getByRole('tab', { name: 'Advanced' }).click();
	// The allowed form is explained, with examples to start from.
	await expect(dialog).toContainText('Only values in this form are accepted.');
	await dialog.getByRole('button', { name: /\[A-Z\]\{3\}-\[0-9\]\{4\}/ }).click();
	await expect(dialog.getByTestId('field-pattern')).toHaveValue('[A-Z]{3}-[0-9]{4}');
	expect(await violations(page)).toEqual([]);
	await applyField(page);
	await saveBuilder(page);
	await expect(page.getByText('Saved Products, version 2')).toBeVisible();
	expect((await definition(page, project, 'products'))[1]).toMatchObject({ id: 'code', pattern: '[A-Z]{3}-[0-9]{4}' });

	// Another option changed: the form stays.
	await page.reload();
	let code = await editField(page, 'code', 'Advanced');
	await expect(code.getByTestId('field-pattern')).toHaveValue('[A-Z]{3}-[0-9]{4}');
	await code.getByLabel('Up to').fill('30');
	await applyField(page);
	await saveBuilder(page);
	await expect(page.getByText('Saved Products, version 3')).toBeVisible();
	expect((await definition(page, project, 'products'))[1]).toMatchObject({ max_chars: 30, pattern: '[A-Z]{3}-[0-9]{4}' });

	// A form the server can't use (look-around) is refused on its field, and at its own input.
	await page.reload();
	code = await editField(page, 'code', 'Advanced');
	await code.getByTestId('field-pattern').fill('(?=A)[A-Z]+');
	await applyField(page);
	await saveBuilder(page);
	await expect(page.getByTestId('field-code').getByTestId('row-problem')).toContainText('Colophon can’t use this allowed form');
	await page.getByTestId('field-code').getByTestId('edit-field').click();
	// The settings open on the tab with the problem.
	await expect(page.getByTestId('field-dialog').getByTestId('pattern-problem')).toContainText('Colophon can’t use this allowed form');
	expect(await violations(page)).toEqual([]);

	// Emptied, it's left out of the definition.
	await page.getByTestId('field-dialog').getByTestId('field-pattern').fill('');
	await applyField(page);
	await saveBuilder(page);
	await expect(page.getByText('Saved Products, version 4')).toBeVisible();
	expect((await definition(page, project, 'products'))[1]).not.toHaveProperty('pattern');
});

test('a value not in its allowed form is refused and said under the field, with its help text, inside a list too', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'products', definition: CODES }, { 'Idempotency-Key': key() })).status).toBe(201);
	await page.goto(`/studio/projects/${project}/products?new=1`);
	await page.getByLabel('Title', { exact: true }).fill('Lamp');
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	const status = page.getByTestId('save-status');
	await expect(status).toHaveAttribute('data-status', 'saved');

	await page.getByLabel('Code (optional)').fill('abc');
	await expect(status).toHaveAttribute('data-status', 'refused');
	await expect(status).toContainText("Not saved: Code isn't in the form this field asks for.");
	await expect(page.locator('[data-testid="field-problem"][data-field="code"]')).toHaveText("Code isn't in its allowed form. Like ABC-1234");
	expect(await internalTerms(page)).toEqual([]);
	expect(await violations(page)).toEqual([]);
	await page.getByLabel('Code (optional)').fill('ABC-1234');
	await expect(status).toHaveAttribute('data-status', 'saved');

	const tags = page.getByTestId('list-tags');
	await tags.getByTestId('list-add').click();
	await tags.getByLabel('Tags 1', { exact: true }).fill('News');
	await expect(status).toHaveAttribute('data-status', 'refused');
	await expect(page.locator('[data-testid="field-problem"][data-field="tags/0"]')).toContainText("Tags 1 isn't in its allowed form.");
	await tags.getByLabel('Tags 1', { exact: true }).fill('news');
	await expect(status).toHaveAttribute('data-status', 'saved');
});

test('adding an allowed form that stored content doesn’t fit lists the entries in the way', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await newEntry(page, project, 'lowercase title');
	await page.goto(`/studio/projects/${project}/settings/types/articles`);
	await (await editField(page, 'title', 'Advanced')).getByTestId('field-pattern').fill('[A-Z].*');
	await applyField(page);
	await saveBuilder(page);
	const refusal = page.getByTestId('type-refusal');
	await expect(refusal.getByRole('link', { name: 'lowercase title' })).toBeVisible();
	expect(await violations(page)).toEqual([]);
});
