// Self-serve sign-up (D-235) in the Studio against the real server, as it launches: registration
// open (deploy/local.env) and no Turnstile site key, so the form carries no widget and needs no
// token. A visitor signs up at /studio/signup, signs out, signs back in at /studio/signin, and
// the usage page shows the account's own meters; its first project counts.

import { expect, test } from '@playwright/test';
import { newProject, violations } from './helpers.ts';

test('open sign-up with no Turnstile, then sign-in, then the usage page', async ({ page }) => {
	const email = `ss-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}@example.test`;
	const providers = await (await page.request.get('/auth/providers')).json();
	expect(providers.data).toMatchObject({ registration: 'open', turnstile_site_key: null });

	await page.goto('/studio/signup');
	await expect(page.locator('#auth-email')).toBeVisible();
	await expect(page.getByTestId('turnstile')).toHaveCount(0);
	expect(await violations(page)).toEqual([]);
	await page.getByLabel('Your name').fill('Ada Self-Serve');
	await page.locator('#auth-email').fill(email);
	await page.locator('#auth-password').fill('a-long-passphrase-1');
	await page.getByTestId('password-submit').click();
	await expect(page).toHaveURL(/\/studio\/$/);
	expect((await page.request.get('/me')).status()).toBe(200);

	await page.getByTestId('account-menu').click();
	await page.getByRole('menuitem', { name: 'Sign out' }).click();
	await expect(page).toHaveURL(/signed-out=1/);

	await page.goto('/studio/signin?return_to=%2Fstudio%2Fusage');
	await page.locator('#auth-email').fill(email);
	await page.locator('#auth-password').fill('a-long-passphrase-1');
	await page.getByTestId('password-submit').click();
	await expect(page).toHaveURL(/\/studio\/usage$/);
	await expect(page.getByTestId('usage-projects')).toContainText(/^Projects\s*0 of \d+/);
	expect(await violations(page)).toEqual([]);

	await newProject(page, 'First site');
	await page.reload();
	await expect(page.getByTestId('usage-projects')).toContainText(/^Projects\s*1 of \d+/);
});
