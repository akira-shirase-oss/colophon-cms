// A newcomer's first minutes on the quick start (D-172, D-173), against the real containers: the
// Studio says the installation is new and where a container's setup code is (the compose logs,
// not a terminal); the first account becomes the administrator with the code read from those
// logs; and they create a project. scripts/quickstart.sh reads the code from
// `docker compose logs colophon` exactly as the Studio tells the person to. Both first-run screens
// pass axe and the plain-words check in light and dark, at 1440 and 360 pixels wide; with
// QUICKSTART_SHOTS set they are photographed there too.

import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { expect, type Page, test } from '@playwright/test';
import { internalTerms, violations } from '../server/helpers.ts';

const code = process.env['QUICKSTART_SETUP_CODE'] ?? '';
const SHOTS = process.env['QUICKSTART_SHOTS'];

/** One screen: axe and plain words in light and dark, wide and narrow; photographed on request. */
async function screen(page: Page, name: string) {
	await page.waitForLoadState('networkidle');
	for (const width of [1440, 360]) {
		await page.setViewportSize({ width, height: width === 360 ? 780 : 900 });
		for (const scheme of ['light', 'dark'] as const) {
			await page.emulateMedia({ colorScheme: scheme });
			await expect(page.locator('html')).toHaveClass(scheme === 'dark' ? /\bdark\b/ : /^(?!.*\bdark\b)/);
			expect(await violations(page), `${name}, ${scheme}, ${width}px`).toEqual([]);
			if (SHOTS !== undefined) {
				mkdirSync(SHOTS, { recursive: true });
				await page.screenshot({ path: join(SHOTS, `quickstart-${name}-${width}-${scheme}.png`), fullPage: true });
			}
		}
	}
	await page.emulateMedia({ colorScheme: 'light' });
	await page.setViewportSize({ width: 1440, height: 900 });
	expect(await internalTerms(page), `${name}: internal terms`).toEqual([]);
}

test('a newcomer finds the setup code in the compose logs, becomes the administrator, and makes a project', async ({ page }) => {
	expect(code, 'scripts/quickstart.sh passes the setup code from the logs').toMatch(/^[0-9A-Z]{5}(-[0-9A-Z]{5}){3}$/);

	await test.step('the Studio says this Colophon is new, and where its code is', async () => {
		await page.goto('/studio/');
		await expect(page.getByTestId('new-installation')).toContainText('This Colophon is new');
		await expect(page.getByTestId('new-installation')).toContainText('to its log when it started');
		await expect(page.getByTestId('setup-code-from-logs')).toContainText('docker compose logs colophon | grep "Setup code"');
		await screen(page, 'welcome');
	});

	await test.step('they create the first account', async () => {
		await page.getByLabel('Your name').fill('Quick Start');
		await page.getByLabel('Email').fill(`quickstart-${Date.now().toString(36)}@example.test`);
		await page.getByLabel('Password').fill('a-long-passphrase-1');
		await page.getByRole('button', { name: 'Create account and continue' }).click();
		await expect(page.getByTestId('setup')).toBeVisible();
		await expect(page.getByTestId('setup')).toContainText('in its log');
		await expect(page.getByTestId('setup-code-from-logs')).toBeVisible();
		await screen(page, 'setup');
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
