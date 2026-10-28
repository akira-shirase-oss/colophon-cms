// The content-type builder as a place of its own (the maintainer's "it works like a real CMS",
// 2026-10-28; Strapi's Content-Type Builder as the pattern): Content types in the menu for roles
// that change content types and not for others, Settings linking to it; a compact list of fields;
// Add a field as a picker of every kind in four groups, then the field's settings in two tabs (a
// code name set only before the first save, Cancel leaving things as they were, a clash refused
// before saving); rows reordered with the keyboard and by dragging; a deleted field brought back;
// the type renamed in Edit type; one save bar; and leaving with unsaved changes asked about first.
// A new type asks what each entry is called (D-082), and that label names entries everywhere.

import { expect, type Page, test } from '@playwright/test';
import { FIELD_TYPES } from '../../src/lib/api.ts';
import {
	api,
	applyField,
	editField,
	env,
	internalTerms,
	key,
	member,
	newEntry,
	newProject,
	publishThroughApi,
	saveBuilder,
	signIn,
	startField,
	violations
} from './helpers.ts';

/** The labels of the top-level field rows, in order. */
const order = (page: Page) => page.getByTestId('field-list').locator(':scope > li [data-testid="field-label-text"]').allTextContents();

test('Content types has its own place in the menu, for roles that change content types, and Settings links to it', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}`);
	await page.getByTestId('builder-link').click();
	await expect(page).toHaveURL(new RegExp(`/projects/${project}/settings/types$`));
	await expect(page.getByRole('heading', { level: 1, name: 'Content types' })).toBeVisible();
	await expect(page.getByTestId('type-card-articles')).toContainText('Many entries · 2 fields');
	await expect(page.getByTestId('builder-nav')).toContainText('Articles');
	expect(await violations(page)).toEqual([]);
	await page.getByTestId('type-card-articles').click();
	await expect(page.getByRole('heading', { level: 1, name: 'Articles' })).toBeVisible();
	await expect(page.getByRole('navigation', { name: 'breadcrumb' })).toContainText('Content types');

	// Settings keeps a link, not the forms.
	await page.goto(`/studio/projects/${project}/settings`);
	await expect(page.getByRole('form', { name: 'New content type' })).toHaveCount(0);
	await page.getByTestId('open-builder').click();
	await expect(page).toHaveURL(new RegExp(`/projects/${project}/settings/types$`));

	// An editor changes content, not content types: no Content types in their menu.
	const editor = await member(page, browser, project, 'editor');
	await expect(editor.getByTestId('members-link')).toBeVisible();
	await expect(editor.getByTestId('builder-link')).toHaveCount(0);
	await editor.context().close();
});

test('Add a field: every kind in four groups, then its settings in two tabs; Cancel changes nothing', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/settings/types/articles`);
	await expect(page.getByTestId('save-bar')).toHaveCount(0);

	await page.getByTestId('add-field').click();
	const picker = page.getByTestId('field-picker');
	await expect(picker.getByRole('heading', { level: 3 })).toHaveText(['Basics', 'Links', 'Structure', 'Advanced']);
	for (const kind of FIELD_TYPES) await expect(picker.getByTestId(`pick-${kind}`), kind).toBeVisible();
	await expect(picker.getByTestId('pick-enum')).toContainText('One option from a list you set, like a category.');
	await expect(picker.getByTestId('pick-component')).toBeDisabled();
	await expect(picker.getByTestId('pick-component')).toContainText('Make one first, under Components.');
	expect(await violations(page)).toEqual([]);
	await picker.getByTestId('pick-text').click();

	const dialog = page.getByTestId('field-dialog');
	await expect(dialog.getByRole('heading', { name: 'New text field' })).toBeVisible();
	await expect(dialog.getByTestId('field-apply')).toBeDisabled();
	await dialog.getByTestId('field-label').fill('Subtitle');
	await dialog.getByTestId('field-description').fill('A line under the title');
	await dialog.getByRole('tab', { name: 'Advanced' }).click();
	// The code name follows the label until it is typed in.
	await expect(dialog.getByTestId('field-code-name')).toHaveValue('subtitle');
	await dialog.getByTestId('field-code-name').fill('sub_title');
	await dialog.getByRole('tab', { name: 'Basics' }).click();
	await dialog.getByTestId('field-label').fill('Subtitle line');
	await dialog.getByRole('tab', { name: 'Advanced' }).click();
	await expect(dialog.getByTestId('field-code-name')).toHaveValue('sub_title');
	expect(await violations(page)).toEqual([]);
	await applyField(page);

	const row = page.getByTestId('field-sub_title');
	await expect(row).toContainText('Subtitle line');
	await expect(row).toContainText('New');
	await expect(row.getByTestId('field-kind')).toHaveText('Text');
	await expect(page.getByTestId('save-bar')).toContainText('You have unsaved changes');

	// Cancel leaves the field as it was.
	await (await editField(page, 'sub_title')).getByTestId('field-label').fill('Something else');
	await page.getByTestId('field-dialog').getByRole('button', { name: 'Cancel' }).click();
	await expect(row.getByTestId('field-label-text')).toHaveText('Subtitle line');

	// A code name another field has is caught before saving.
	const clash = await startField(page, 'Body', 'text');
	await expect(clash.getByTestId('field-dialog-problem')).toHaveText('Another field here already has the code name body.');
	await expect(clash.getByTestId('field-apply')).toBeDisabled();
	await page.keyboard.press('Escape');

	await saveBuilder(page);
	await expect(page.getByText('Saved Articles, version 2')).toBeVisible();
	await expect(page.getByTestId('save-bar')).toHaveCount(0);
	const fields = (await api(page, 'GET', `${env(project)}/content-types/articles`)).body?.['data'].definition.fields as Record<string, unknown>[];
	expect(fields[2]).toMatchObject({ id: 'sub_title', label: 'Subtitle line', type: 'text', description: 'A line under the title' });
	// Saved: no longer new, and its code name is fixed.
	await expect(row).not.toContainText('New');
	const saved = await editField(page, 'sub_title', 'Advanced');
	await expect(saved.getByTestId('field-code-name')).toHaveText('sub_title');
	await expect(saved.getByRole('textbox', { name: 'Code name' })).toHaveCount(0);
});

test('fields reorder with the keyboard and by dragging, a deleted one comes back with Undo, and the type is renamed in Edit type', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const text = (id: string, label: string) => ({ id, label, type: 'text', required: false, max_chars: 200, multiline: false });
	const definition = { name: 'Notes', title_field: 'title', fields: [{ ...text('title', 'Title'), required: true }, text('a', 'A'), text('b', 'B'), text('c', 'C')] };
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'notes', definition }, { 'Idempotency-Key': key() })).status).toBe(201);
	await page.goto(`/studio/projects/${project}/settings/types/notes`);
	await expect.poll(() => order(page)).toEqual(['Title', 'A', 'B', 'C']);

	// The keyboard: the handle, then the arrow keys; focus stays on the moved field, and it's said.
	await page.getByRole('button', { name: 'Move C' }).focus();
	await page.keyboard.press('ArrowUp');
	await page.keyboard.press('ArrowUp');
	await expect.poll(() => order(page)).toEqual(['Title', 'C', 'A', 'B']);
	await expect(page.getByRole('button', { name: 'Move C' })).toBeFocused();
	await expect(page.getByText('C moved to place 2 of 4.')).toBeAttached();

	// Dragging Title by its handle onto B's row puts it there.
	await page.getByRole('button', { name: 'Move Title' }).dragTo(page.getByTestId('field-b'));
	await expect.poll(() => order(page)).toEqual(['C', 'A', 'B', 'Title']);

	// Deleted, then brought back where it was.
	await page.getByRole('button', { name: 'Delete A' }).click();
	await expect.poll(() => order(page)).toEqual(['C', 'B', 'Title']);
	await page.getByRole('button', { name: 'Undo' }).click();
	await expect.poll(() => order(page)).toEqual(['C', 'A', 'B', 'Title']);
	// The title stays.
	await expect(page.getByRole('button', { name: 'Delete Title' })).toBeDisabled();

	// The type's own settings: renamed in Edit type.
	await page.getByTestId('edit-type').click();
	await page.getByTestId('type-name').fill('Journal');
	await page.getByRole('button', { name: 'Done' }).click();
	await expect(page.getByRole('heading', { level: 1, name: 'Journal' })).toBeVisible();
	expect(await violations(page)).toEqual([]);
	await saveBuilder(page);
	await expect(page.getByText('Saved Journal, version 2')).toBeVisible();
	const saved = (await api(page, 'GET', `${env(project)}/content-types/notes`)).body?.['data'].definition as { name: string; fields: { id: string }[] };
	expect(saved.name).toBe('Journal');
	expect(saved.fields.map((field) => field.id)).toEqual(['c', 'a', 'b', 'title']);
});

test('leaving with unsaved changes asks first; Stay keeps them, Discard puts things back', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/settings/types/articles`);
	await startField(page, 'Summary', 'text');
	await applyField(page);
	await expect(page.getByTestId('field-summary')).toBeVisible();

	// Another page of the Studio: asked first, and Stay keeps the change.
	await page.getByRole('link', { name: 'Overview' }).click();
	const question = page.getByRole('alertdialog');
	await expect(question).toContainText('Leave without saving?');
	await expect(question).toContainText('Your changes to Articles haven');
	expect(await violations(page)).toEqual([]);
	await question.getByRole('button', { name: 'Stay' }).click();
	await expect(page).toHaveURL(/settings\/types\/articles$/);
	await expect(page.getByTestId('field-summary')).toBeVisible();

	// Discard: back as saved, and nothing to ask about.
	await page.getByTestId('save-bar').getByRole('button', { name: 'Discard' }).click();
	await expect(page.getByTestId('field-summary')).toHaveCount(0);
	await expect(page.getByTestId('save-bar')).toHaveCount(0);

	// Leave without saving goes, and the change is gone.
	await startField(page, 'Summary', 'text');
	await applyField(page);
	await page.getByRole('link', { name: 'Overview' }).click();
	await page.getByRole('alertdialog').getByRole('button', { name: 'Leave without saving' }).click();
	await expect(page).toHaveURL(new RegExp(`/projects/${project}$`));
	const fields = (await api(page, 'GET', `${env(project)}/content-types/articles`)).body?.['data'].definition.fields as { id: string }[];
	expect(fields.map((field) => field.id)).toEqual(['title', 'body']);
});

test('a new type asks what each one is called; that names its entries in the builder, the list, search and what readers get', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/settings/types`);

	// The default answer keeps the `title` code name.
	await page.getByTestId('new-type').click();
	await expect(page.getByLabel('What is each one called?')).toHaveValue('Title');
	await page.locator('#type-name').fill('Pages');
	await page.getByTestId('create').click();
	await expect(page.getByRole('heading', { level: 1, name: 'Pages' })).toBeVisible();
	await expect(page.getByTestId('field-title').getByTestId('title-badge')).toHaveText('Shown in lists');
	const pages = (await api(page, 'GET', `${env(project)}/content-types/pages`)).body?.['data'].definition;
	expect(pages.title_field).toBe('title');
	expect(pages.fields).toEqual([{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false }]);

	// "Name": the field is called Name, and its code name follows.
	await page.getByTestId('new-type').click();
	await page.locator('#type-name').fill('Products');
	await page.getByLabel('What is each one called?').fill('Name');
	expect(await violations(page)).toEqual([]);
	await page.getByTestId('create').click();
	await expect(page.getByRole('heading', { level: 1, name: 'Products' })).toBeVisible();
	const row = page.getByTestId('field-name');
	await expect(row.getByTestId('field-label-text')).toHaveText('Name');
	await expect(row.getByTestId('title-badge')).toHaveText('Shown in lists');
	await expect(row.getByTestId('delete-field')).toBeDisabled();
	await expect(page.getByTestId('field-title')).toHaveCount(0);
	await expect(page.getByTestId('field-list')).not.toContainText(/\bTitle\b/);
	const products = (await api(page, 'GET', `${env(project)}/content-types/products`)).body?.['data'].definition;
	expect(products.title_field).toBe('name');
	expect(products.fields).toEqual([{ id: 'name', label: 'Name', type: 'text', required: true, max_chars: 200, multiline: false }]);
	expect(await violations(page)).toEqual([]);

	// The list's column, its search and an empty search all say Name.
	const beanie = await newEntry(page, project, 'Wool beanie', 'products', { name: 'Wool beanie' });
	await page.goto(`/studio/projects/${project}/products`);
	await expect(page.getByTestId('title-column')).toHaveText('Name');
	await expect(page.getByTestId('entry-search')).toHaveAttribute('placeholder', 'Search names');
	await expect(page.getByTestId('entries')).toContainText('Wool beanie');
	await page.getByTestId('entry-search').fill('tote');
	await expect(page.getByTestId('entries')).toContainText('No names match “tote”.');
	expect(await internalTerms(page)).toEqual([]);
	expect(await violations(page)).toEqual([]);

	// What readers get names it the same way.
	await publishThroughApi(page, project, 'Beanie', [beanie]);
	await page.goto(`/studio/projects/${project}/products/${beanie}`);
	await expect(page.getByLabel('Name', { exact: true })).toHaveValue('Wool beanie');
	await page.getByTestId('entry-details').getByTestId('technical-details').locator('summary').click();
	await page.getByTestId('see-what-readers-get').click();
	await expect(page.getByTestId('reader-title-label')).toHaveText('Name');
	await expect(page.getByTestId('reader-title')).toHaveText('Wool beanie');
});
