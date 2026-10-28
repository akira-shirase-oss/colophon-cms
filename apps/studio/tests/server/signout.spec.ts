// Signing out (POST /auth/logout) from any screen: the account menu in the project shell and on
// pages outside a project. The session ends on the server (/me answers 401 afterwards); then the
// browser follows the identity provider's sign-out address, confirms there, and comes back to the
// front door, so signing in again asks for a password (the next person at a shared computer isn't
// let straight in). When the provider gives no address, the front door says what that means; when
// the server can't be reached, the Studio says you're still signed in rather than pretending.

import { expect, type Page, test } from '@playwright/test';
import { internalTerms, keycloak, newProject, signIn, violations } from './helpers.ts';

/** Keycloak's "Do you want to log out?" page, confirmed. */
async function confirmAtProvider(page: Page) {
	await expect(page).toHaveURL(/\/realms\/colophon\/protocol\/openid-connect\/logout/);
	await page.locator('#kc-logout').click();
}

test('signing out from the projects page ends the session here and at the sign-in provider', async ({ page }) => {
	await signIn(page);
	await expect(page.getByRole('button', { name: /^Account: / })).toBeVisible();
	expect(await violations(page)).toEqual([]);
	// From the keyboard: open the menu, move to Sign out, press Enter.
	await page.getByTestId('account-menu').focus();
	await page.keyboard.press('Enter');
	await expect(page.getByRole('menuitem', { name: 'Sign out' })).toBeVisible();
	await page.getByRole('menuitem', { name: 'Sign out' }).focus();
	await page.keyboard.press('Enter');

	await confirmAtProvider(page);
	await expect(page).toHaveURL(/\/studio\/$/);
	await expect(page.getByTestId('signed-out')).toBeVisible();
	await expect(page.getByTestId('sign-in')).toBeVisible();
	await expect(page.getByTestId('account-menu')).toHaveCount(0);
	expect((await page.request.get('/me')).status()).toBe(401);
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);

	// Signing in again asks for a password: the provider's session ended too.
	await page.getByTestId('sign-in').click();
	await expect(page.locator('#password')).toBeVisible();
	await keycloak(page);
	await expect(page.getByTestId('signed-in')).toBeVisible();
});

test('signing out from inside a project ends the session too', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/articles`);
	await expect(page.getByTestId('entries-empty')).toBeVisible();
	await page.getByTestId('account-menu').click();
	await expect(page.getByRole('menu')).toContainText('Signed in as');
	await page.getByRole('menuitem', { name: 'Sign out' }).click();

	await confirmAtProvider(page);
	await expect(page.getByTestId('sign-in')).toBeVisible();
	expect((await page.request.get('/me')).status()).toBe(401);
	// The project's pages now ask the person to sign in instead of showing its content.
	await page.goto(`/studio/projects/${project}/articles`);
	await expect(page.getByRole('alert')).toContainText('Sign in');
});

test('when the provider gives no sign-out address, the front door says what that means', async ({ page }) => {
	await signIn(page);
	// The server ends the session as usual; only its answer about the provider is replaced.
	await page.route('**/auth/logout', async (route) => {
		const response = await route.fetch();
		await route.fulfill({ response, json: { data: { provider_sign_out: null } } });
	});
	await page.getByTestId('account-menu').click();
	await page.getByRole('menuitem', { name: 'Sign out' }).click();
	await expect(page).toHaveURL(/\/studio\/\?signed-out=1$/);
	await expect(page.getByTestId('signed-out-note')).toContainText("You're signed out of Colophon.");
	await expect(page.getByTestId('signed-out-note')).toContainText('close the browser');
	expect((await page.request.get('/me')).status()).toBe(401);
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);
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
