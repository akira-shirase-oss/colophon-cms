// Structures (step M.3, D-066) in the Studio, against the real server: a page type with a group of
// fields, a list and page sections built in the builder, which stops offering structures three
// deep and says what keeps a definition from being saved.

import { expect, type Page, test } from '@playwright/test';
import { api, env, key, newEntry, newProject, shot, signIn, violations } from './helpers.ts';

const PEOPLE = {
	name: 'People',
	title_field: 'title',
	fields: [{ id: 'title', label: 'Name', type: 'text', required: true, max_chars: 200, multiline: false }]
};
async function site(page: Page): Promise<{ project: string; ada: string }> {
	await signIn(page);
	const project = await newProject(page);
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'people', definition: PEOPLE }, { 'Idempotency-Key': key() })).status).toBe(201);
	const ada = await newEntry(page, project, 'Ada Lovelace', 'people', { title: 'Ada Lovelace' });
	return { project, ada };
}

test('a page type with a group, a list and page sections is built in the builder', async ({ page }) => {
	const { project } = await site(page);
	await page.goto(`/studio/projects/${project}/settings`);
	await page.locator('#type-name').fill('Pages');
	await page.getByRole('button', { name: 'Create and add fields' }).click();
	await expect(page.getByTestId('field-title')).toBeVisible();

	const add = async (form: string, label: string, type: string) => {
		const scope = page.getByRole('form', { name: form, exact: true });
		await scope.getByRole('textbox').fill(label);
		await scope.getByRole('combobox').selectOption({ label: type });
		await scope.getByRole('button', { name: 'Add field' }).click();
	};
	await add('Add a field', 'Hero', 'Group of fields');
	await add('Add a field to Hero', 'Heading', 'Text');
	await add('Add a field', 'FAQ', 'List');
	await page.getByLabel('What each item of FAQ is').selectOption({ label: 'Group of fields' });
	await add('Add a field to FAQ item', 'Question', 'Text');
	await add('Add a field to FAQ item', 'Answer', 'Markdown text');
	await page.getByTestId('field-faq/item/question').getByLabel('Required').check();
	await page.getByLabel('Most items in FAQ').fill('5');
	await add('Add a field', 'Sections', 'Page sections');
	// Nothing to save yet: page sections need a kind, and each kind a field.
	await expect(page.getByTestId('builder-problems')).toContainText('Sections needs at least one kind of section.');
	await expect(page.getByRole('button', { name: 'Save content type' })).toBeDisabled();
	const kinds = page.getByRole('form', { name: 'Add a kind of section to Sections' });
	await kinds.getByRole('textbox').fill('Quote');
	await kinds.getByRole('button', { name: 'Add kind of section' }).click();
	await expect(page.getByTestId('builder-problems')).toContainText('The Quote section of Sections needs at least one field.');
	await add('Add a field to Quote section', 'Text', 'Text');
	await add('Add a field to Quote section', 'By', 'Link to entries');
	await page.getByTestId('field-sections/quote/by').getByLabel('Links to').selectOption({ label: 'People' });
	await kinds.getByRole('textbox').fill('Call to action');
	await kinds.getByRole('button', { name: 'Add kind of section' }).click();
	await add('Add a field to Call to action section', 'Label', 'Text');
	await add('Add a field to Call to action section', 'Link', 'Web address');
	await expect(page.getByTestId('builder-problems')).toHaveCount(0);

	// Three deep is the limit: inside a group inside a group inside a group, none is offered.
	await add('Add a field to Hero', 'Inner', 'Group of fields');
	await add('Add a field to Inner', 'Innermost', 'Group of fields');
	await expect(page.getByRole('form', { name: 'Add a field to Innermost' }).getByTestId('nesting-limit')).toContainText('nest at most 3 deep');
	await expect(page.getByRole('form', { name: 'Add a field to Innermost' }).getByRole('combobox').locator('option', { hasText: 'Group of fields' })).toHaveCount(0);
	await page.getByTestId('field-hero').getByRole('button', { name: 'Remove Inner', exact: true }).click();
	expect(await violations(page)).toEqual([]);
	await shot(page, 'builder-m3');

	await page.getByRole('button', { name: 'Save content type' }).click();
	await expect(page.getByText('Saved Pages, version 2')).toBeVisible();
	const saved = await api(page, 'GET', `${env(project)}/content-types/pages`);
	const fields = saved.body?.['data'].definition.fields as Record<string, any>[];
	expect(fields.map((field) => field['type'])).toEqual(['text', 'object', 'list', 'blocks']);
	expect(fields[1]).toMatchObject({ id: 'hero', fields: [{ id: 'heading', type: 'text' }] });
	expect(fields[2]).toMatchObject({ id: 'faq', max: 5, of: { type: 'object', fields: [{ id: 'question', required: true }, { id: 'answer', type: 'markdown' }] } });
	const sectionKinds = fields[3]?.['blocks'] as { name: string; fields: Record<string, unknown>[] }[];
	expect(sectionKinds.map((block) => block.name)).toEqual(['quote', 'call_to_action']);
	expect(sectionKinds[0]?.fields[1]).toMatchObject({ id: 'by', type: 'reference', to: 'people' });
});
