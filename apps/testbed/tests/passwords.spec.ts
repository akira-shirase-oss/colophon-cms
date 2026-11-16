// Email-and-password accounts (D-084) through the testbed, in a real browser, against the real
// server: signing up sets an ordinary session, signing out ends it, and a wrong password
// refuses like a missing account does.

import { expect, test } from '@playwright/test';

const email = () => `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}@example.test`;

test('an email account signs up, signs out, and signs back in', async ({ page }) => {
	const address = email();
	const password = 'a testbed password';
	await page.goto('/session');
	await expect(page.getByTestId('signed-out')).toBeVisible();

	await page.getByLabel('Email').fill(address);
	await page.getByLabel('Password').fill(password);
	await page.getByRole('button', { name: 'Create account' }).click();
	await expect(page.getByTestId('signed-in')).toBeVisible();
	await expect(page.getByTestId('signed-in')).toContainText('(password)');
	await expect(page.getByTestId('standing')).toContainText(address);

	// The session ends; the same email and password start a new one.
	await page.getByRole('button', { name: 'Sign out' }).click();
	await expect(page.getByTestId('signed-out')).toBeVisible();
	await page.getByLabel('Email').fill(address);
	await page.getByLabel('Password').fill(password);
	await page.getByRole('button', { name: 'Sign in' }).click();
	await expect(page.getByTestId('signed-in')).toBeVisible();
	await expect(page.getByTestId('standing')).toContainText(address);

	// And the email can't make a second account.
	await page.getByRole('button', { name: 'Sign out' }).click();
	await page.getByLabel('Email').fill(address);
	await page.getByLabel('Password').fill('another password');
	await page.getByRole('button', { name: 'Create account' }).click();
	await expect(page.getByTestId('password-message')).toContainText('409 conflict');
});

test('a wrong password refuses the same way a missing account does', async ({ page }) => {
	const address = email();
	await page.goto('/session');
	await page.getByLabel('Email').fill(address);
	await page.getByLabel('Password').fill('the real password');
	await page.getByRole('button', { name: 'Create account' }).click();
	await expect(page.getByTestId('signed-in')).toBeVisible();
	await page.getByRole('button', { name: 'Sign out' }).click();
	await expect(page.getByTestId('signed-out')).toBeVisible();

	for (const attempt of [
		{ address, password: 'the wrong password' },
		{ address: email(), password: 'the real password' }
	]) {
		await page.getByLabel('Email').fill(attempt.address);
		await page.getByLabel('Password').fill(attempt.password);
		await page.getByRole('button', { name: 'Sign in' }).click();
		await expect(page.getByTestId('password-message')).toContainText('400 sign_in_failed');
	}
});
