// Creating projects in the Studio (D-043): the bootstrapped administrator (dev-owner) creates one
// from the home page and lands in it as its owner; anyone else sees no form, only how to join.

import { expect, test } from '@playwright/test';
import { signIn, violations } from './helpers.ts';

test('the administrator creates a project from the home page; others are told how to join one', async ({ page, browser }) => {
	await signIn(page);
	await expect(page.getByTestId('signed-in')).toContainText('administrator');
	const name = `Field notes ${Date.now().toString(36)}`;
	await page.getByLabel('Name', { exact: true }).fill(name);
	await expect(page.getByLabel('Slug (in addresses and the CLI)')).toHaveValue(/^field-notes-[a-z0-9]+$/);
	expect(await violations(page)).toEqual([]);
	await page.getByRole('button', { name: 'Create project' }).click();
	await expect(page.getByRole('heading', { name: 'Content' })).toBeVisible();
	await expect(page.getByTestId('content-types')).toContainText('Articles');
	await page.goto('/studio/');
	await expect(page.getByTestId('projects').getByRole('listitem').filter({ hasText: name })).toContainText('Owner');

	// The same slug again is refused, and says why.
	await page.getByLabel('Name', { exact: true }).fill(name);
	await page.getByRole('button', { name: 'Create project' }).click();
	await expect(page.getByRole('alert')).toContainText('already exists');

	const other = await (await browser.newContext()).newPage();
	await signIn(other, 'dev-editor');
	await expect(other.getByTestId('not-administrator')).toContainText('invitation link');
	await expect(other.getByRole('button', { name: 'Create project' })).toHaveCount(0);
	expect(await violations(other)).toEqual([]);
});
