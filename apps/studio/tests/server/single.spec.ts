// Single types (step M.4, D-067) in the Studio, against the real server: the builder's switch makes
// a content type hold one entry; the sidebar opens that entry directly, and with none yet makes it,
// named after the type, with no dialog and no list (D-082); a required field that needs an answer
// brings the dialog instead; a second one is refused in plain words, with a way to the one that
// exists; and a type that already has two entries can't be made single, said plainly.

import { expect, test } from '@playwright/test';
import { api, env, internalTerms, key, newContentType, newEntry, newProject, saveBuilder, signIn, violations } from './helpers.ts';

const SETTINGS = {
	name: 'Site settings',
	title_field: 'title',
	single: true,
	fields: [{ id: 'title', label: 'Site name', type: 'text', required: true, max_chars: 200, multiline: false }]
};

test('the switch makes a type hold one entry; the sidebar makes it on first open, then opens it directly', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await newContentType(page, project, 'Site settings');
	await page.getByTestId('edit-type').click();
	await page.getByLabel('Just one entry (like site settings or a home page)').check();
	expect(await violations(page)).toEqual([]);
	await page.getByRole('button', { name: 'Done' }).click();
	await expect(page.getByTestId('type-builder')).toContainText('Single type: just one entry');
	await expect(page.getByTestId('open-entries')).toHaveText('See its entries');
	await saveBuilder(page);
	await expect(page.getByText('Saved Site settings, version 2')).toBeVisible();
	await expect(page.getByTestId('open-entries')).toHaveText('Open it');
	expect((await api(page, 'GET', `${env(project)}/content-types/site-settings`)).body?.['data'].definition.single).toBe(true);

	// None yet: the sidebar's link makes it, named after the type, and opens its editor: no dialog,
	// no list, no search.
	await page.getByTestId('nav-site-settings').click();
	await expect(page).toHaveURL(new RegExp(`/site-settings/[0-9a-f-]{36}$`));
	await expect(page.getByLabel('Title', { exact: true })).toHaveValue('Site settings');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await expect(page.getByRole('dialog')).toHaveCount(0);
	await expect(page.getByTestId('entry-search')).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'New entry' })).toHaveCount(0);
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);
	await page.getByLabel('Title', { exact: true }).fill('Field notes');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	const entry = new URL(page.url()).pathname.split('/').at(-1) ?? '';
	expect((await api(page, 'GET', `${env(project)}/collections/site-settings/entries`)).body?.['data']).toHaveLength(1);

	// Once it exists, the sidebar opens it, and so does the type's own address.
	await page.goto(`/studio/projects/${project}`);
	await expect(page.getByTestId('nav-site-settings')).toHaveAttribute('href', `/studio/projects/${project}/site-settings/${entry}`);
	await page.getByTestId('nav-site-settings').click();
	await expect(page).toHaveURL(new RegExp(`/site-settings/${entry}$`));
	await expect(page.getByLabel('Title', { exact: true })).toHaveValue('Field notes');
	await page.goto(`/studio/projects/${project}/site-settings`);
	await expect(page).toHaveURL(new RegExp(`/site-settings/${entry}$`));
	expect(await internalTerms(page)).toEqual([]);

	// On a phone, a second single type opens (made on the way) and the editor fits.
	const empty = { ...SETTINGS, name: 'Home page' };
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'home', definition: empty }, { 'Idempotency-Key': key() })).status).toBe(201);
	await page.setViewportSize({ width: 390, height: 844 });
	for (const path of [`/studio/projects/${project}/home`, `/studio/projects/${project}/site-settings/${entry}`]) {
		await page.goto(path);
		await page.waitForLoadState('networkidle');
		expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), `${path} scrolls sideways on a phone`).toBeLessThanOrEqual(0);
		expect(await violations(page)).toEqual([]);
	}
});

test('a required field that needs an answer brings the dialog; a second entry is refused in plain words, with a way to the one that exists', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const year = { id: 'year', label: 'Year', type: 'number', required: true, integer: true };
	const definition = { ...SETTINGS, fields: [...SETTINGS.fields, year] };
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'site-settings', definition }, { 'Idempotency-Key': key() })).status).toBe(201);
	await page.goto(`/studio/projects/${project}/site-settings`);
	// The dialog opens by itself, named after the type, and asks only for what it can't guess.
	await expect(page.getByRole('dialog')).toBeVisible();
	await expect(page.getByLabel('Site name', { exact: true })).toHaveValue('Site settings');
	await page.getByLabel('Site name', { exact: true }).fill('Mine');
	await page.getByLabel('Year', { exact: true }).fill('2026');
	// Meanwhile someone else creates it.
	const theirs = await newEntry(page, project, 'Theirs', 'site-settings', { title: 'Theirs', year: 2025 });
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	await expect(page.getByTestId('create-error')).toContainText('This already has its entry; edit that one.');
	expect(await internalTerms(page)).toEqual([]);
	await page.getByTestId('create-error').getByRole('button', { name: 'Open it' }).click();
	await expect(page).toHaveURL(new RegExp(`/site-settings/${theirs}$`));
	await expect(page.getByLabel('Site name', { exact: true })).toHaveValue('Theirs');
});

test('made by someone else at the same moment, the first open opens theirs instead of a second', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'site-settings', definition: SETTINGS }, { 'Idempotency-Key': key() })).status).toBe(201);
	const theirs = await newEntry(page, project, 'Theirs', 'site-settings', { title: 'Theirs' });
	// This page's list was read just before theirs arrived: it says there is none until it tries.
	let tried = false;
	await page.route(/\/collections\/site-settings\/entries(\?|$)/, async (route) => {
		if (route.request().method() === 'POST') tried = true;
		if (route.request().method() === 'GET' && !tried) await route.fulfill({ json: { data: [], next_cursor: null } });
		else await route.continue();
	});
	await page.goto(`/studio/projects/${project}/site-settings`);
	await expect(page).toHaveURL(new RegExp(`/site-settings/${theirs}$`));
	await expect(page.getByLabel('Site name', { exact: true })).toHaveValue('Theirs');
	expect(tried).toBe(true);
	await expect(page.getByTestId('create-error')).toHaveCount(0);
	expect((await api(page, 'GET', `${env(project)}/collections/site-settings/entries`)).body?.['data']).toHaveLength(1);
});

test('a type with two entries can’t be made single, and the builder says so plainly', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await newEntry(page, project, 'One');
	await newEntry(page, project, 'Two');
	await page.goto(`/studio/projects/${project}/settings/types/articles`);
	await page.getByTestId('edit-type').click();
	await page.getByLabel('Just one entry (like site settings or a home page)').check();
	await page.getByRole('button', { name: 'Done' }).click();
	await saveBuilder(page);
	await expect(page.getByTestId('type-refusal')).toContainText('It has more than one entry already, so it can’t hold just one.');
	expect(await violations(page)).toEqual([]);
	expect((await api(page, 'GET', `${env(project)}/content-types/articles`)).body?.['data'].definition.single).toBeUndefined();
});
