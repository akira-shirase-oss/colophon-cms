// Signing in through the testbed: the server's real OpenID Connect flow and Keycloak's own form.

import { expect, type Page } from '@playwright/test';

export async function signIn(page: Page, user: string) {
	await page.goto('/session');
	await expect(page.getByTestId('signed-out')).toBeVisible();
	await page.getByTestId('sign-in').click();
	// Keycloak's own login form.
	await page.locator('#username').fill(user);
	await page.locator('#password').fill('colophon-dev');
	await page.locator('#kc-login').click();
	// The server finishes the sign-in and sends the browser to /me on its own origin.
	await page.waitForURL(/\/me$/);
	await page.goto('/session');
	await expect(page.getByTestId('signed-in')).toBeVisible();
}
