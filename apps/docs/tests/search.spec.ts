// Search: the Pagefind index the build wrote, served from this site's own origin, finds known
// pages, and its results lead to them.

import { expect, test } from '@playwright/test';

test('search finds a known page and opens it', async ({ page }) => {
	await page.goto('/', { waitUntil: 'networkidle' });
	await page.getByTestId('search-open').click();
	await page.getByTestId('search-input').fill('idempotency');
	const results = page.getByTestId('search-results');
	await expect(results.getByRole('link', { name: /Management API conventions/ })).toBeVisible();
	await results.getByRole('link', { name: /Management API conventions/ }).click();
	await expect(page).toHaveURL(/\/reference\/api\/$/);
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Management API conventions');
});

test('the keyboard opens search, and Enter opens the first result', async ({ page }) => {
	await page.goto('/concepts/', { waitUntil: 'networkidle' });
	await page.keyboard.press('ControlOrMeta+k');
	const input = page.getByTestId('search-input');
	await expect(input).toBeFocused();
	await input.fill('tombstone');
	await expect(page.getByTestId('search-results').getByRole('link').first()).toBeVisible();
	const first = await page.getByTestId('search-results').getByRole('link').first().getAttribute('href');
	await input.press('Enter');
	await expect(page).toHaveURL(new RegExp(`${first ?? ''}$`));
});

test('search indexes the design documents too', async ({ page }) => {
	await page.goto('/', { waitUntil: 'networkidle' });
	await page.keyboard.press('/');
	await page.getByTestId('search-input').fill('Garage');
	await expect(page.getByTestId('search-results').getByRole('link', { name: /Decisions and deviations/ })).toBeVisible();
});

test('a search with no match says so', async ({ page }) => {
	await page.goto('/', { waitUntil: 'networkidle' });
	await page.getByTestId('search-open').click();
	await page.getByTestId('search-input').fill('zebrafishxylophone');
	await expect(page.getByTestId('search-status')).toContainText('Nothing matches');
});
