// A newcomer's first minutes on the quick start (D-171, D-172), against the real containers: the
// Studio says the installation is new and where a container's setup code is (the compose logs,
// not a terminal); the first account becomes the administrator with the code read from those
// logs; and they create a project. scripts/quickstart.sh reads the code from
// `docker compose logs colophon` exactly as the Studio tells the person to.

import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';

const code = process.env['QUICKSTART_SETUP_CODE'] ?? '';

test('a newcomer finds the setup code in the compose logs, becomes the administrator, and makes a project', async ({ page }) => {
	expect(code, 'scripts/quickstart.sh passes the setup code from the logs').toMatch(/^[0-9A-Z]{5}(-[0-9A-Z]{5}){3}$/);

	await test.step('the Studio says this Colophon is new, and where its code is', async () => {
		await page.goto('/studio/');
		await expect(page.getByTestId('new-installation')).toContainText('This Colophon is new');
		await expect(page.getByTestId('new-installation')).toContainText('to its log when it started');
		await expect(page.getByTestId('setup-code-from-logs')).toContainText('docker compose logs colophon | grep "Setup code"');
		const serious = (await new AxeBuilder({ page }).analyze()).violations.filter((v) => v.impact === 'serious' || v.impact === 'critical');
		expect(serious.map((v) => v.id)).toEqual([]);
	});

	await test.step('they create the first account', async () => {
		await page.getByLabel('Your name').fill('Quick Start');
		await page.getByLabel('Email').fill(`quickstart-${Date.now().toString(36)}@example.test`);
		await page.getByLabel('Password').fill('a-long-passphrase-1');
		await page.getByRole('button', { name: 'Create account and continue' }).click();
		await expect(page.getByTestId('setup')).toBeVisible();
		await expect(page.getByTestId('setup')).toContainText('in its log');
		await expect(page.getByTestId('setup-code-from-logs')).toBeVisible();
	});

	await test.step('the code from the logs makes them the administrator', async () => {
		await page.getByLabel('Setup code').fill(code);
		await page.getByRole('button', { name: 'Become the administrator' }).click();
		await expect(page.getByTestId('signed-in')).toContainText('administrator');
	});

	await test.step('they make their first project', async () => {
		await expect(page.getByRole('heading', { name: 'Create your first project' })).toBeVisible();
		const form = page.getByTestId('new-project');
		await form.getByLabel('Name', { exact: true }).fill('Quick start');
		await form.getByRole('button', { name: 'Create project' }).click();
		await expect(page).toHaveURL(/\/studio\/projects\/[^/]+/);
	});
});
