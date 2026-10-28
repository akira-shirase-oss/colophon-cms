// Single types (step M.4, D-067) in the Studio, against the real server: the builder's switch makes
// a content type hold one entry; the sidebar opens that entry directly, or the page offers to
// create it when there is none; a second one is refused in plain words, with a way to the one that
// exists; and a type that already has two entries can't be made single, said plainly.

import { expect, test } from '@playwright/test';
import { api, env, internalTerms, key, newContentType, newEntry, newProject, saveBuilder, signIn, violations } from './helpers.ts';

const SETTINGS = {
	name: 'Site settings',
	title_field: 'title',
	single: true,
	fields: [{ id: 'title', label: 'Site name', type: 'text', required: true, max_chars: 200, multiline: false }]
};

test('the switch makes a type hold one entry; the sidebar creates it, then opens it directly', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await newContentType(page, project, 'Site settings');
	await page.getByTestId('edit-type').click();
	await page.getByLabel('Just one entry (like site settings or a home page)').check();
	expect(await violations(page)).toEqual([]);
	await page.getByRole('button', { name: 'Done' }).click();
	await expect(page.getByTestId('type-builder')).toContainText('Single type: just one entry');
	await saveBuilder(page);
	await expect(page.getByText('Saved Site settings, version 2')).toBeVisible();
	expect((await api(page, 'GET', `${env(project)}/content-types/site-settings`)).body?.['data'].definition.single).toBe(true);

	// None yet: the sidebar's link offers to create it.
	await page.getByTestId('nav-site-settings').click();
	await expect(page.getByTestId('single-empty')).toContainText("Site settings isn't written yet");
	await expect(page.getByRole('button', { name: 'New entry' })).toHaveCount(0);
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);
	await page.getByTestId('single-create').click();
	await page.getByLabel('Title', { exact: true }).fill('Field notes');
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	const entry = new URL(page.url()).pathname.split('/').at(-1) ?? '';

	// Once it exists, the sidebar opens it, and so does the type's own address.
	await page.goto(`/studio/projects/${project}`);
	await expect(page.getByTestId('nav-site-settings')).toHaveAttribute('href', `/studio/projects/${project}/site-settings/${entry}`);
	await page.getByTestId('nav-site-settings').click();
	await expect(page).toHaveURL(new RegExp(`/site-settings/${entry}$`));
	await expect(page.getByLabel('Title', { exact: true })).toHaveValue('Field notes');
	await page.goto(`/studio/projects/${project}/site-settings`);
	await expect(page).toHaveURL(new RegExp(`/site-settings/${entry}$`));
	expect(await internalTerms(page)).toEqual([]);

	// On a phone, the empty page and the editor fit (a second type, still empty, for the first).
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

test('a second entry is refused in plain words, with a way to the one that exists', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'site-settings', definition: SETTINGS }, { 'Idempotency-Key': key() })).status).toBe(201);
	await page.goto(`/studio/projects/${project}/site-settings`);
	await page.getByTestId('single-create').click();
	await page.getByLabel('Site name', { exact: true }).fill('Mine');
	// Meanwhile someone else creates it.
	const theirs = await newEntry(page, project, 'Theirs', 'site-settings', { title: 'Theirs' });
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	await expect(page.getByTestId('create-error')).toContainText('This already has its entry; edit that one.');
	expect(await internalTerms(page)).toEqual([]);
	await page.getByTestId('create-error').getByRole('button', { name: 'Open it' }).click();
	await expect(page).toHaveURL(new RegExp(`/site-settings/${theirs}$`));
	await expect(page.getByLabel('Site name', { exact: true })).toHaveValue('Theirs');
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
