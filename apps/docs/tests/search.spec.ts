// Search: the Pagefind index the build wrote, served under the docs' base path from this site's own
// origin, finds known pages (the changelog's entries and the design documents included), and its
// results lead to them.

import { expect, test } from '@playwright/test';
import { docs } from './site.ts';

test('search finds a known page and opens it', async ({ page }) => {
	await page.goto(docs('/'), { waitUntil: 'networkidle' });
	await page.getByTestId('search-open').click();
	await page.getByTestId('search-input').fill('idempotency');
	const results = page.getByTestId('search-results');
	await expect(results.getByRole('link', { name: /API conventions/ }).first()).toBeVisible();
	await results.getByRole('link', { name: /API conventions/ }).first().click();
	await expect(page).toHaveURL(/\/docs\/reference\/api\/(#.*)?$/);
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('API conventions');
});

test('search finds a changelog entry and lands on it', async ({ page }) => {
	await page.goto(docs('/concepts/'), { waitUntil: 'networkidle' });
	await page.getByTestId('search-open').click();
	await page.getByTestId('search-input').fill('Killing it on purpose');
	// The changelog answers with the entry the words are in, not just the page.
	const hit = page.getByTestId('search-results').locator(`a[href="${docs('/changelog/#step-2-4')}"]`);
	await expect(hit).toBeVisible();
	await expect(hit).toContainText(/Changelog\s*›\s*2\.4 · Killing it on purpose/);
	await hit.click();
	await expect(page).toHaveURL(/\/docs\/changelog\/#step-2-4$/);
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Changelog');
	await expect(page.locator('#step-2-4')).toBeInViewport();
});

test('the keyboard opens search, and Enter opens the first result', async ({ page }) => {
	await page.goto(docs('/concepts/'), { waitUntil: 'networkidle' });
	await page.keyboard.press('ControlOrMeta+k');
	const input = page.getByTestId('search-input');
	await expect(input).toBeFocused();
	await input.fill('tombstone');
	await expect(page.getByTestId('search-results').getByRole('link').first()).toBeVisible();
	const first = (await page.getByTestId('search-results').getByRole('link').first().getAttribute('href')) ?? '';
	await input.press('Enter');
	await expect(page).toHaveURL(new RegExp(`${first.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}$`));
});

test('search indexes the design documents too', async ({ page }) => {
	await page.goto(docs('/'), { waitUntil: 'networkidle' });
	await page.keyboard.press('/');
	// "Garage" was the term until the guides mentioned it as often as the decisions do.
	await page.getByTestId('search-input').fill('deviations');
	await expect(page.getByTestId('search-results').getByRole('link', { name: /Decisions and deviations/ }).first()).toBeVisible();
});

test('a search with no match says so', async ({ page }) => {
	await page.goto(docs('/'), { waitUntil: 'networkidle' });
	await page.getByTestId('search-open').click();
	// When nothing matches exactly, Pagefind falls back to indexed words the query begins with, one
	// letter long included: zebrafishxylophone stopped qualifying as the pages grew, and vvwvvw when
	// milestone V arrived. Probing "<letter>xqjzqk" for every letter on 2026-10-27, only a and t
	// matched nothing (Pagefind doesn't index either alone), and no word begins "ax".
	await page.getByTestId('search-input').fill('axqjzqk');
	await expect(page.getByTestId('search-status')).toContainText('Nothing matches');
});
