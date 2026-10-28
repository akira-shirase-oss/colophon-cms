// Creating projects in the Studio (D-043, step 3.7): the bootstrapped administrator (dev-owner)
// creates one from the home page, from a starting point, and lands in it as its owner; anyone
// else sees no form, only how to join. (A first project, on an installation with none, is in
// journey.spec.ts.)

import { expect, test } from '@playwright/test';
import { newProject, signIn, violations } from './helpers.ts';

test('the administrator creates a project from the home page; others are told how to join one', async ({ page, browser }) => {
	await signIn(page);
	// With a project already, making another is the page's one action, in a dialog.
	await newProject(page);
	await page.reload();
	await expect(page.getByTestId('signed-in')).toContainText('administrator');
	await page.getByRole('button', { name: 'New project' }).click();
	const name = `Field notes ${Date.now().toString(36)}`;
	await page.getByLabel('Name', { exact: true }).fill(name);
	await expect(page.getByRole('textbox', { name: 'URL name' })).toHaveValue(/^field-notes-[a-z0-9]+$/);
	await page.getByRole('radio', { name: 'A documentation site' }).check();
	expect(await violations(page)).toEqual([]);
	await page.getByRole('button', { name: 'Create project' }).click();
	await expect(page.getByRole('heading', { name: 'Content' })).toBeVisible();
	await expect(page.getByTestId('content-types')).toContainText('Articles');
	await expect(page.getByTestId('content-types')).toContainText('Pages');
	// The starting point went through the content-type API: Pages has a title, a URL name and a body.
	const project = new URL(page.url()).pathname.split('/').at(-1) ?? '';
	const pages = await page.evaluate(async (id) => {
		const response = await fetch(`/api/v1/projects/${id}/environments/production/content-types/pages`);
		return (await response.json()) as { data: { definition: { fields: { type: string }[] } } };
	}, project);
	expect(pages.data.definition.fields.map((field) => field.type)).toEqual(['text', 'slug', 'rich_text']);
	await page.goto('/studio/');
	await expect(page.getByTestId('projects').getByRole('listitem').filter({ hasText: name })).toContainText('Owner');

	// The same URL name again is refused, and says why.
	await page.getByRole('button', { name: 'New project' }).click();
	await page.getByLabel('Name', { exact: true }).fill(name);
	await page.getByRole('button', { name: 'Create project' }).click();
	await expect(page.getByRole('alert')).toContainText('already exists');

	const other = await (await browser.newContext()).newPage();
	await signIn(other, 'dev-editor');
	await expect(other.getByTestId('not-administrator')).toContainText('invitation link');
	await expect(other.getByRole('button', { name: 'Create project' })).toHaveCount(0);
	expect(await violations(other)).toEqual([]);
});
