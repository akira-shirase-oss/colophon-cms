// Signing out (POST /auth/logout) from any screen: the account menu in the project shell and on
// pages outside a project. The session ends on the server (/me answers 401 afterwards) and the
// front door says so; when the server can't be reached, the Studio says you're still signed in
// rather than pretending.

import { expect, test } from '@playwright/test';
import { internalTerms, newProject, signIn, violations } from './helpers.ts';

test('signing out from the projects page ends the session, and the front door says so', async ({ page }) => {
	await signIn(page);
	await expect(page.getByRole('button', { name: /^Account: / })).toBeVisible();
	expect(await violations(page)).toEqual([]);
	// From the keyboard: open the menu, move to Sign out, press Enter.
	await page.getByTestId('account-menu').focus();
	await page.keyboard.press('Enter');
	await expect(page.getByRole('menuitem', { name: 'Sign out' })).toBeVisible();
	await page.getByRole('menuitem', { name: 'Sign out' }).focus();
	await page.keyboard.press('Enter');

	await expect(page.getByTestId('signed-out')).toBeVisible();
	await expect(page.getByTestId('signed-out-note')).toContainText("You're signed out");
	await expect(page.getByTestId('sign-in')).toBeVisible();
	await expect(page.getByTestId('account-menu')).toHaveCount(0);
	expect((await page.request.get('/me')).status()).toBe(401);
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);
});

test('signing out from inside a project ends the session too', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/articles`);
	await expect(page.getByTestId('entries-empty')).toBeVisible();
	await page.getByTestId('account-menu').click();
	await expect(page.getByRole('menu')).toContainText('Signed in as');
	await page.getByRole('menuitem', { name: 'Sign out' }).click();

	await expect(page).toHaveURL(/\/studio\/\?signed-out=1$/);
	await expect(page.getByTestId('sign-in')).toBeVisible();
	expect((await page.request.get('/me')).status()).toBe(401);
	// The project's pages now ask the person to sign in instead of showing its content.
	await page.goto(`/studio/projects/${project}/articles`);
	await expect(page.getByRole('alert')).toContainText('Sign in');
});

test('when the server can’t be reached, signing out says you are still signed in', async ({ page }) => {
	await signIn(page);
	await page.route('**/auth/logout', (route) => route.abort('connectionrefused'));
	await page.getByTestId('account-menu').click();
	await page.getByRole('menuitem', { name: 'Sign out' }).click();
	await expect(page.getByText("You're still signed in.")).toBeVisible();
	await expect(page.getByTestId('signed-in')).toBeVisible();
	expect((await page.request.get('/me')).status()).toBe(200);
});
