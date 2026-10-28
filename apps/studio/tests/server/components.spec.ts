// Shared components (step M.4, D-073) in the Studio, against the real server: made in Settings,
// given fields on their own page, inserted into a content type from its builder (shown there
// read-only, with the way to its page), and filled in an entry like any group; a change saved for
// every content type that uses it, each with its new version said; a change one of them can't take
// refused, naming that content type from the server's answer (`details.content_type`) and listing
// its entries; and a role that may change only
// one content type reads components but can't change them, which the server refuses too.

import { expect, type Page, test } from '@playwright/test';
import { api, env, internalTerms, key, member, newEntry, newProject, shot, signIn, violations } from './helpers.ts';

const TITLE = { id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false };
const typeUsing = (name: string) => ({ name, title_field: 'title', fields: [TITLE, { id: 'seo', label: 'SEO', type: 'component', component: 'seo', required: false }] });

async function withTypes(page: Page, types: [string, Record<string, unknown>][]): Promise<string> {
	await signIn(page);
	const project = await newProject(page);
	for (const [slug, definition] of types) {
		const made = await api(page, 'POST', `${env(project)}/content-types`, { slug, definition }, { 'Idempotency-Key': key() });
		expect(made.status, JSON.stringify(made.body)).toBe(201);
	}
	return project;
}

async function seo(page: Page, project: string) {
	const made = await api(
		page,
		'POST',
		`${env(project)}/components`,
		{ slug: 'seo', definition: { name: 'SEO', fields: [{ id: 'title', label: 'Title', type: 'text', required: false, max_chars: 70, multiline: false }] } },
		{ 'Idempotency-Key': key() }
	);
	expect(made.status, JSON.stringify(made.body)).toBe(201);
}

const add = async (page: Page, form: string, label: string, type: string) => {
	const scope = page.getByRole('form', { name: form, exact: true });
	await scope.getByRole('textbox').fill(label);
	await scope.getByRole('combobox').selectOption({ label: type });
	await scope.getByRole('button', { name: 'Add field' }).click();
};

test('a component is made in Settings, inserted into a content type, and filled in an entry like a group', async ({ page }) => {
	const project = await withTypes(page, [['pages', { name: 'Pages', title_field: 'title', fields: [TITLE] }]]);
	await page.goto(`/studio/projects/${project}/settings`);
	await page.locator('#component-name').fill('SEO');
	await expect(page.locator('#component-slug')).toHaveValue('seo');
	await page.getByRole('button', { name: 'Create component' }).click();
	await expect(page.getByRole('heading', { level: 1, name: 'SEO' })).toBeVisible();
	await expect(page.getByTestId('component-users')).toContainText('No content type uses it yet.');

	// Its fields: a title to start, and a description added here.
	await expect(page.getByTestId('field-title')).toBeVisible();
	await add(page, 'Add a field to SEO', 'Description', 'Text');
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);
	await page.getByRole('button', { name: 'Save component' }).click();
	await expect(page.getByTestId('component-saved')).toContainText('Saved SEO, version 2.');
	await expect(page.getByTestId('component-saved')).toContainText('No content type needed a new version.');
	await shot(page, 'component-m4');

	// Inserted into Pages from its builder: shown with its fields and the way to its page.
	await page.goto(`/studio/projects/${project}/settings/types/pages`);
	await add(page, 'Add a field', 'Search', 'Component: SEO');
	const inserted = page.getByTestId('component-search');
	await expect(inserted.getByRole('list', { name: 'Fields of SEO' }).getByRole('listitem')).toHaveText(['Title', 'Description']);
	await expect(page.getByTestId('field-search')).toContainText('Component: SEO');
	expect(await violations(page)).toEqual([]);
	await page.getByRole('button', { name: 'Save content type' }).click();
	await expect(page.getByText('Saved Pages, version 2')).toBeVisible();
	const pages = (await api(page, 'GET', `${env(project)}/content-types/pages`)).body?.['data'].definition.fields as Record<string, any>[];
	expect(pages[1]).toMatchObject({ id: 'search', type: 'object', component: 'seo', fields: [{ id: 'title' }, { id: 'description' }] });
	await page.getByTestId('component-search').getByRole('link', { name: 'Open the SEO component' }).click();
	await expect(page.getByTestId('component-users')).toContainText('Used by Pages.');

	// Settings lists it with who uses it.
	await page.goto(`/studio/projects/${project}/settings`);
	await expect(page.getByTestId('components')).toContainText('2 fields · version 2 · Used by Pages');

	// In an entry it is a group of fields like any other.
	const entry = await newEntry(page, project, 'Home', 'pages', { title: 'Home' });
	await page.goto(`/studio/projects/${project}/pages/${entry}`);
	await page.getByTestId('group-search').getByLabel('Title (optional)').fill('Home, found');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	const stored = (await api(page, 'GET', `${env(project)}/collections/pages/entries/${entry}`)).body?.['data'].content;
	expect(stored).toMatchObject({ title: 'Home', search: { title: 'Home, found' } });

	// On a phone, the component's page fits.
	await page.setViewportSize({ width: 390, height: 844 });
	await page.goto(`/studio/projects/${project}/settings/components/seo`);
	await expect(page.getByTestId('field-title')).toBeVisible();
	expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'the component page scrolls sideways on a phone').toBeLessThanOrEqual(0);
	expect(await violations(page)).toEqual([]);
});

test('a change reaches every content type that uses it; one their entries can’t take is refused, naming the type and entries', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await seo(page, project);
	for (const [slug, name] of [
		['pages', 'Pages'],
		['posts', 'Posts']
	] as const) {
		const made = await api(page, 'POST', `${env(project)}/content-types`, { slug, definition: typeUsing(name) }, { 'Idempotency-Key': key() });
		expect(made.status, JSON.stringify(made.body)).toBe(201);
	}
	await newEntry(page, project, 'An old post', 'posts', { title: 'An old post', seo: { title: 'Old' } });

	await page.goto(`/studio/projects/${project}/settings/components/seo`);
	await expect(page.getByTestId('component-users')).toContainText('Used by Pages, Posts.');
	await add(page, 'Add a field to SEO', 'Description', 'Text');
	await page.getByTestId('field-description').getByLabel('Required').check();
	// The server's answer names the type: no entry is read to find it.
	const reads: string[] = [];
	page.on('request', (request) => {
		if (request.method() === 'GET' && request.url().includes('/entries/')) reads.push(request.url());
	});
	const answer = page.waitForResponse((response) => response.request().method() === 'PUT' && response.url().endsWith('/components/seo'));
	await page.getByRole('button', { name: 'Save component' }).click();
	expect((await (await answer).json()).error.details).toMatchObject({ reason: 'needs_migration', content_type: 'posts' });
	// Pages has no entries in the way; Posts does, so nothing changes, and Posts is named and linked.
	const refusal = page.getByTestId('component-refusal');
	await expect(refusal.getByTestId('refusal-words')).toHaveText('Posts has entries that don’t fit this change yet. Change them (they are listed below), or keep the component as it is.');
	await expect(refusal.getByRole('link', { name: 'Posts', exact: true })).toHaveAttribute('href', /settings\/types\/posts$/);
	await expect(refusal.getByRole('link', { name: 'An old post' })).toHaveAttribute('href', /\/posts\//);
	await expect(refusal.getByTestId('blocker-problem')).toHaveText('It has no Description, which it would need.');
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);
	expect((await api(page, 'GET', `${env(project)}/content-types/pages`)).body?.['data'].version).toBe(1);
	expect(reads).toEqual([]);

	// Optional, it goes through: both types get a new version, said with links.
	await page.getByTestId('field-description').getByLabel('Required').uncheck();
	await page.getByRole('button', { name: 'Save component' }).click();
	const saved = page.getByTestId('component-saved');
	await expect(saved).toContainText('Saved SEO, version 2.');
	await expect(saved).toContainText('Also changed: Pages, now version 2, Posts, now version 2.');
	await expect(saved.getByRole('link', { name: 'Posts' })).toHaveAttribute('href', /settings\/types\/posts$/);
	for (const slug of ['pages', 'posts']) {
		const type = (await api(page, 'GET', `${env(project)}/content-types/${slug}`)).body?.['data'];
		expect(type.version, slug).toBe(2);
		expect(type.definition.fields[1].fields.map((field: { id: string }) => field.id), slug).toEqual(['title', 'description']);
	}
	await shot(page, 'component-saved-m4');
});

test('a change one content type can’t take at all is refused, naming that type from the server’s answer', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await seo(page, project);
	// Pages uses it at the top; Posts two groups down, where it can't hold a group of its own.
	const nested = {
		name: 'Posts',
		title_field: 'title',
		fields: [
			TITLE,
			{
				id: 'meta',
				label: 'Meta',
				type: 'object',
				required: false,
				fields: [{ id: 'more', label: 'More', type: 'object', required: false, fields: [{ id: 'seo', label: 'SEO', type: 'component', component: 'seo', required: false }] }]
			}
		]
	};
	for (const [slug, definition] of [
		['pages', typeUsing('Pages')],
		['posts', nested]
	] as const) {
		const made = await api(page, 'POST', `${env(project)}/content-types`, { slug, definition }, { 'Idempotency-Key': key() });
		expect(made.status, JSON.stringify(made.body)).toBe(201);
	}

	await page.goto(`/studio/projects/${project}/settings/components/seo`);
	await expect(page.getByTestId('component-users')).toContainText('Used by Pages, Posts.');
	await add(page, 'Add a field to SEO', 'Links', 'Group of fields');
	await add(page, 'Add a field to Links', 'Label', 'Text');
	await page.getByRole('button', { name: 'Save component' }).click();
	// No entries are in the way, and two types use it: only the server's answer can say which refused.
	const refusal = page.getByTestId('component-refusal');
	await expect(refusal.getByTestId('refusal-words')).toHaveText('Posts can’t take this change.');
	await expect(refusal.getByRole('link', { name: 'Posts', exact: true })).toHaveAttribute('href', /settings\/types\/posts$/);
	await expect(refusal.getByRole('list', { name: 'Entries in the way' })).toHaveCount(0);
	expect(await violations(page)).toEqual([]);
	for (const slug of ['pages', 'posts']) expect((await api(page, 'GET', `${env(project)}/content-types/${slug}`)).body?.['data'].version, slug).toBe(1);
	expect((await api(page, 'GET', `${env(project)}/components/seo`)).body?.['data'].version).toBe(1);
});

test('a role that may change only one content type reads components but can’t change them', async ({ page, browser }) => {
	const project = await withTypes(page, []);
	await seo(page, project);
	const made = await api(page, 'POST', `${env(project)}/content-types`, { slug: 'pages', definition: typeUsing('Pages') }, { 'Idempotency-Key': key() });
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	const role = await api(page, 'POST', `/api/v1/projects/${project}/roles`, {
		key: 'pages-designer',
		name: 'Pages designer',
		permissions: [
			{ action: 'content.read', subject: '*' },
			{ action: 'schema.change', subject: 'pages' }
		]
	});
	expect(role.status, JSON.stringify(role.body)).toBe(201);
	const designer = await member(page, browser, project, 'pages-designer');

	await designer.goto(`/studio/projects/${project}/settings`);
	await expect(designer.getByTestId('components')).toContainText('View');
	await expect(designer.getByRole('form', { name: 'New component' })).toHaveCount(0);
	await designer.goto(`/studio/projects/${project}/settings/components/seo`);
	await expect(designer.getByText('Changing it takes a role that may change every content type.')).toBeVisible();
	await expect(designer.getByTestId('field-title')).toBeVisible();
	await expect(designer.getByRole('button', { name: 'Save component' })).toHaveCount(0);
	await expect(designer.getByRole('form', { name: 'Add a field to SEO' })).toHaveCount(0);
	expect(await violations(designer)).toEqual([]);

	// The server decides: the change is refused.
	const refused = await api(
		designer,
		'PUT',
		`${env(project)}/components/seo`,
		{ name: 'SEO', fields: [{ id: 'title', label: 'Title', type: 'text', required: false, max_chars: 60, multiline: false }] },
		{ 'If-Match': '"v1"', 'Idempotency-Key': key() }
	);
	expect(refused.status).toBe(403);
	await designer.context().close();
});
