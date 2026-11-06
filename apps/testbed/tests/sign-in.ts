// Signing in through the testbed: the built-in password card on the Session page itself
// (D-146) — no identity provider in the way.

import { expect, type Page } from '@playwright/test';

export async function signIn(page: Page, user: string) {
	await page.goto('/session');
	await expect(page.getByTestId('signed-out')).toBeVisible();
	await page.locator('input[name="email"]').fill(`${user}@colophon.test`);
	await page.locator('input[name="password"]').fill('colophon-dev');
	await page.getByRole('button', { name: 'Sign in', exact: true }).click();
	await expect(page.getByTestId('signed-in')).toBeVisible();
}
