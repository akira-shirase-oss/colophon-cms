// The project list a page at a time through the testbed: three projects whose names share a
// token, searched in lower case, two per page, then the last on the next page; an over-long
// search is refused with 400.

import { expect, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

test('search projects, page through them, and see a bad search refused', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const token = `pg${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;
	await page.evaluate(async (token) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		for (const letter of ['C', 'A', 'B']) {
			await fetch('/server/api/v1/projects', {
				method: 'POST',
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
				body: JSON.stringify({ slug: `tb-${token}-${letter.toLowerCase()}`, name: `Paging ${token.toUpperCase()} ${letter}` })
			});
		}
	}, token);

	await page.goto('/projects');
	await page.getByLabel('Search projects').fill(token);
	await page.getByLabel('Page size').selectOption('2');
	await page.getByRole('button', { name: 'Load' }).click();
	const rows = page.getByTestId('projects').locator('li');
	await expect(rows).toHaveCount(2);
	await expect(rows.nth(0)).toContainText(`Paging ${token.toUpperCase()} A`);
	await expect(rows.nth(1)).toContainText(`Paging ${token.toUpperCase()} B`);

	await page.getByRole('button', { name: 'Next page' }).click();
	await expect(rows).toHaveCount(1);
	await expect(rows.first()).toContainText(`Paging ${token.toUpperCase()} C`);
	await expect(page.getByRole('button', { name: 'Next page' })).toHaveCount(0);

	await page.getByLabel('Search projects').fill('x'.repeat(101));
	await page.getByRole('button', { name: 'Load' }).click();
	await expect(page.getByRole('status')).toContainText('400');
});
