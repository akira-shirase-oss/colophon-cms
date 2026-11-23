// Self-serve sign-up (D-235) through the testbed, in a real browser, against the real server with
// open registration and no Turnstile (the launch configuration): the page says so, a fresh
// account signs up with no bot token, reads its own empty usage under the project cap, and its
// first project counts.

import { expect, test } from '@playwright/test';

test('open sign-up with no bot check makes an account that sees its own usage move', async ({ page }) => {
	await page.goto('/signup');
	await expect(page.getByTestId('providers')).toHaveText('registration open, bot check off');
	await page.getByLabel('Email').fill(`tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}@example.test`);
	await page.getByLabel('Password').fill('a testbed password');
	await page.getByRole('button', { name: 'Sign up' }).click();
	await expect(page.getByTestId('message')).toHaveText('signed up');
	const projects = page.getByTestId('usage').locator('[data-meter="projects"]');
	await expect(projects).toContainText(/0\s*of \d+/);
	await expect(page.getByTestId('usage').locator('[data-meter="entries"]')).toContainText('0');

	await page.getByLabel('Project slug').fill(`tb-${Date.now().toString(36)}`);
	await page.getByRole('button', { name: 'Create project' }).click();
	await expect(page.getByTestId('message')).toContainText('created');
	await expect(projects).toContainText(/1\s*of \d+/);
});
