// Structures (step M.3, D-066) in the Studio, against the real server: a page type with a group of
// fields, a list and page sections built in the builder; an entry filled in (two kinds of section,
// a list of questions, a quote linking to a person), sections reordered, stored in the server's
// canonical form (`_block` first, fields in definition order) and nothing left to save after a
// reload; a refusal deep inside shown under its own control; and "Linked from" naming the place.

import { expect, type Page, test } from '@playwright/test';
import { addField, api, applyField, editField, env, internalTerms, key, newContentType, newEntry, newProject, saveBuilder, shot, signIn, startField, violations } from './helpers.ts';

const PEOPLE = {
	name: 'People',
	title_field: 'title',
	fields: [{ id: 'title', label: 'Name', type: 'text', required: true, max_chars: 200, multiline: false }]
};
const text = (id: string, label: string, required = false, max_chars = 200) => ({ id, label, type: 'text', required, max_chars, multiline: false });
const PAGES = {
	name: 'Pages',
	title_field: 'title',
	fields: [
		text('title', 'Title', true),
		{ id: 'hero', label: 'Hero', type: 'object', required: false, fields: [text('heading', 'Heading', true), text('tagline', 'Tagline')] },
		{
			id: 'faq',
			label: 'FAQ',
			type: 'list',
			required: false,
			max: 5,
			of: { type: 'object', fields: [text('q', 'Question', true), { id: 'a', label: 'Answer', type: 'markdown', required: false, max_chars: 2000 }] }
		},
		{ id: 'tags', label: 'Tags', type: 'list', required: false, max: 5, of: { type: 'text', max_chars: 40, multiline: false } },
		{
			id: 'sections',
			label: 'Sections',
			type: 'blocks',
			required: false,
			max: 10,
			blocks: [
				{ name: 'quote', label: 'Quote', fields: [text('text', 'Text', true, 500), { id: 'by', label: 'By', type: 'reference', required: false, to: 'people', many: false }] },
				{ name: 'cta', label: 'Call to action', fields: [text('label', 'Label', true, 60), { id: 'link', label: 'Link', type: 'url', required: false }] }
			]
		}
	]
};

async function site(page: Page): Promise<{ project: string; ada: string }> {
	await signIn(page);
	const project = await newProject(page);
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'people', definition: PEOPLE }, { 'Idempotency-Key': key() })).status).toBe(201);
	const ada = await newEntry(page, project, 'Ada Lovelace', 'people', { title: 'Ada Lovelace' });
	return { project, ada };
}

const status = (page: Page) => page.getByTestId('save-status');

test('a page type with a group, a list and page sections is built in the builder', async ({ page }) => {
	const { project } = await site(page);
	await newContentType(page, project, 'Pages');

	await addField(page, 'Hero', 'object');
	await addField(page, 'Heading', 'text', 'Hero');
	const faq = await startField(page, 'FAQ', 'list');
	await faq.getByLabel('What each item of FAQ is').selectOption({ label: 'Group of fields' });
	await faq.getByLabel('Most items in FAQ').fill('5');
	await applyField(page);
	await addField(page, 'Question', 'text', 'FAQ item');
	await addField(page, 'Answer', 'markdown', 'FAQ item');
	await (await editField(page, 'faq/item/question')).getByLabel('Required').check();
	await applyField(page);
	await addField(page, 'Sections', 'blocks');
	// Nothing to save yet: page sections need a kind, and each kind a field.
	await expect(page.getByTestId('builder-problems')).toContainText('Sections needs at least one kind of section.');
	await expect(page.getByTestId('save-bar').getByTestId('save')).toBeDisabled();
	const kinds = page.getByRole('form', { name: 'Add a kind of section to Sections' });
	await kinds.getByRole('textbox').fill('Quote');
	await kinds.getByRole('button', { name: 'Add kind of section' }).click();
	await expect(page.getByTestId('builder-problems')).toContainText('The Quote section of Sections needs at least one field.');
	await addField(page, 'Text', 'text', 'Quote section');
	await (await startField(page, 'By', 'reference', 'Quote section')).getByLabel('Links to').selectOption({ label: 'People' });
	await applyField(page);
	await kinds.getByRole('textbox').fill('Call to action');
	await kinds.getByRole('button', { name: 'Add kind of section' }).click();
	await addField(page, 'Label', 'text', 'Call to action section');
	await addField(page, 'Link', 'url', 'Call to action section');
	await expect(page.getByTestId('builder-problems')).toHaveCount(0);

	// Three deep is the limit: inside a group inside a group inside a group, none is offered.
	await addField(page, 'Inner', 'object', 'Hero');
	await addField(page, 'Innermost', 'object', 'Inner');
	await page.getByRole('button', { name: 'Add a field to Innermost', exact: true }).click();
	await expect(page.getByTestId('field-picker').getByTestId('nesting-limit')).toContainText('nest at most 3 deep');
	await expect(page.getByTestId('pick-object')).toBeDisabled();
	await expect(page.getByTestId('pick-text')).toBeEnabled();
	await page.keyboard.press('Escape');
	await page.getByTestId('field-hero').getByRole('button', { name: 'Delete Inner', exact: true }).click();
	expect(await violations(page)).toEqual([]);
	await shot(page, 'builder-m3');

	await saveBuilder(page);
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

test('a page is filled in, sections reordered, stored canonically, and a nested refusal shown in place', async ({ page }) => {
	const { project, ada } = await site(page);
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'pages', definition: PAGES }, { 'Idempotency-Key': key() })).status).toBe(201);
	await page.goto(`/studio/projects/${project}/pages?new=1`);
	await page.getByLabel('Title', { exact: true }).fill('Home');
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	const entry = new URL(page.url()).pathname.split('/').at(-1) ?? '';

	await page.getByTestId('group-hero').getByLabel('Heading', { exact: true }).fill('Welcome');
	const faq = page.getByTestId('list-faq');
	await faq.getByTestId('list-add').click();
	await faq.getByTestId('list-item').nth(0).getByLabel('Question', { exact: true }).fill('What is it?');
	await faq.getByTestId('list-item').nth(0).getByLabel('Answer (optional)').fill('A CMS.');
	await faq.getByTestId('list-add').click();
	await faq.getByTestId('list-item').nth(1).getByLabel('Question', { exact: true }).fill('Who is it for?');
	const tags = page.getByTestId('list-tags');
	await tags.getByTestId('list-add').click();
	await tags.getByLabel('Tags 1', { exact: true }).fill('news');
	await tags.getByTestId('list-add').click();
	await tags.getByLabel('Tags 2', { exact: true }).fill('launch');

	const sections = page.getByTestId('sections-sections');
	await sections.getByTestId('section-kind').selectOption({ label: 'Call to action' });
	await sections.getByTestId('section-add').click();
	await sections.getByTestId('section').nth(0).getByLabel('Label', { exact: true }).fill('Sign up');
	await sections.getByTestId('section').nth(0).getByLabel('Link (optional)').fill('https://example.com/join');
	await sections.getByTestId('section-kind').selectOption({ label: 'Quote' });
	await sections.getByTestId('section-add').click();
	const quote = sections.getByTestId('section').nth(1);
	await expect(quote).toHaveAttribute('data-kind', 'quote');
	await quote.getByLabel('Text', { exact: true }).fill('Proof over breadth.');
	await quote.getByTestId('reference-search').fill('Ada');
	await quote.getByRole('button', { name: /Ada Lovelace/ }).click();
	await expect(quote.getByTestId('reference-chip')).toHaveText(/Ada Lovelace/);
	await expect(status(page)).toHaveAttribute('data-status', 'saved');

	// Reordered: the quote first.
	await sections.getByRole('button', { name: 'Move Quote 2 up' }).click();
	await expect(sections.getByTestId('section')).toHaveText([/Quote 1/, /Call to action 2/]);
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);
	await shot(page, 'editor-m3');

	// Stored canonically: `_block` first, every field in its definition's order.
	const stored = await page.evaluate(async (path) => (await fetch(path, { cache: 'no-store' })).text(), `${env(project)}/collections/pages/entries/${entry}`);
	expect(stored).toContain('"hero":{"heading":"Welcome"}');
	expect(stored).toContain('"faq":[{"q":"What is it?","a":"A CMS."},{"q":"Who is it for?"}]');
	expect(stored).toContain('"tags":["news","launch"]');
	expect(stored).toContain(`"sections":[{"_block":"quote","text":"Proof over breadth.","by":"${ada}"},{"_block":"cta","label":"Sign up","link":"https://example.com/join"}]`);

	// Reopened: the same values, and nothing to save.
	const saves: string[] = [];
	page.on('request', (request) => {
		if (request.method() === 'PUT' && request.url().includes('/entries/')) saves.push(request.url());
	});
	await page.reload();
	await expect(page.getByTestId('sections-sections').getByTestId('section')).toHaveText([/Quote 1/, /Call to action 2/]);
	await expect(page.getByTestId('list-faq').getByTestId('list-item').nth(1).getByLabel('Question', { exact: true })).toHaveValue('Who is it for?');
	await page.waitForTimeout(2_000);
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	expect(saves).toEqual([]);

	// A refusal deep inside: the second question emptied is refused where it is.
	await page.getByTestId('list-faq').getByTestId('list-item').nth(1).getByLabel('Question', { exact: true }).fill('');
	await expect(status(page)).toHaveAttribute('data-status', 'refused');
	await expect(page.locator('[data-testid="field-problem"][data-field="faq/1/q"]')).toBeVisible();
	await expect(page.locator('[data-testid="field-problem"][data-field="faq/0/q"]')).toHaveCount(0);
	expect(await internalTerms(page)).toEqual([]);
	await page.getByTestId('list-faq').getByTestId('list-item').nth(1).getByLabel('Question', { exact: true }).fill('Who is it for?');
	await expect(status(page)).toHaveAttribute('data-status', 'saved');

	// On a phone, the structures fit.
	await page.setViewportSize({ width: 390, height: 844 });
	await page.waitForLoadState('networkidle');
	expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'the editor scrolls sideways on a phone').toBeLessThanOrEqual(0);
	expect(await violations(page)).toEqual([]);

	// The person's editor says where the page links to them, in words.
	await page.setViewportSize({ width: 1280, height: 800 });
	await page.goto(`/studio/projects/${project}/people/${ada}`);
	await expect(page.getByTestId('linked-from-entry')).toContainText('Home');
	await expect(page.getByTestId('linked-from-entry')).toContainText('Pages, in Sections → Quote 1 → By');
	expect(await internalTerms(page)).toEqual([]);
});
