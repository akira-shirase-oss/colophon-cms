// Accessibility: axe (WCAG 2.1 AA) on every page in light and in dark, the search dialog with
// results and the phone navigation sheet in both; reflow down to 320 px with no sideways scrolling
// on every page.

import AxeBuilder from '@axe-core/playwright';
import { expect, type Page, test } from '@playwright/test';
import { builtPages, docs } from './site.ts';

const pages = builtPages();

async function axe(page: Page) {
	const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze();
	return results.violations.map(
		(violation) => `${violation.id}: ${violation.help} (${violation.nodes.map((node) => node.target.join(' ')).join(', ')})`
	);
}

for (const scheme of ['light', 'dark'] as const) {
	test.describe(`${scheme} mode`, () => {
		test.use({ colorScheme: scheme });

		for (const address of pages) {
			test(`no WCAG 2.1 AA violations on ${address}`, async ({ page }) => {
				// axe takes several seconds on the longest reference pages, more on a busy machine.
				test.setTimeout(120_000);
				await page.goto(docs(address), { waitUntil: 'networkidle' });
				await expect(page.locator('html')).toHaveClass(scheme === 'dark' ? /\bdark\b/ : /^(?!.*\bdark\b)/);
				expect(await axe(page)).toEqual([]);
			});
		}

		test('no violations in the search dialog with results', async ({ page }) => {
			await page.goto(docs('/concepts/'), { waitUntil: 'networkidle' });
			await page.getByTestId('search-open').click();
			await page.getByTestId('search-input').fill('receipt');
			await expect(page.getByTestId('search-results')).toBeVisible();
			expect(await axe(page)).toEqual([]);
		});

		test('no violations in the navigation sheet on a phone', async ({ page }) => {
			await page.setViewportSize({ width: 390, height: 844 });
			await page.goto(docs('/editors/review/'), { waitUntil: 'networkidle' });
			await page.getByRole('button', { name: 'Open the navigation' }).click();
			await expect(page.getByRole('dialog')).toBeVisible();
			expect(await axe(page)).toEqual([]);
		});
	});
}

test('every page reflows at 320 px without sideways scrolling', async ({ page }) => {
	test.setTimeout(300_000);
	await page.setViewportSize({ width: 320, height: 720 });
	const overflowing: string[] = [];
	for (const address of pages) {
		await page.goto(docs(address), { waitUntil: 'networkidle' });
		const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
		if (overflow > 0) overflowing.push(`${address}: ${overflow}px`);
	}
	expect(overflowing).toEqual([]);
});

test('the navigation sheet opens on a phone and leads to a page', async ({ page }) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await page.goto(docs('/'), { waitUntil: 'networkidle' });
	await page.getByRole('button', { name: 'Open the navigation' }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByRole('link', { name: 'Taking something down' }).click();
	await expect(page).toHaveURL(/\/docs\/editors\/taking-down\/$/);
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Taking something down');
	await expect(sheet).toBeHidden();
});
