// Accessibility: axe (WCAG 2.1 AA) in light and dark on a sample of every section, the menu
// sheet and the search dialog; reflow down to 320 px with no sideways scrolling; and the
// appearance choice.

import AxeBuilder from '@axe-core/playwright';
import { expect, type Page, test } from '@playwright/test';

const SAMPLE = [
	'/',
	'/get-started/quickstart/',
	'/editors/website-status/',
	'/concepts/releases-and-generations/',
	'/reference/api/',
	'/reference/api/errors/',
	'/reference/cli/',
	'/operations/load-testing/',
	'/design/',
	'/design/decisions/',
	'/design/modules/04-api/'
];

async function axe(page: Page) {
	const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze();
	return results.violations.map(
		(violation) => `${violation.id}: ${violation.help} (${violation.nodes.map((node) => node.target.join(' ')).join(', ')})`
	);
}

for (const scheme of ['light', 'dark'] as const) {
	test.describe(`${scheme} mode`, () => {
		test.use({ colorScheme: scheme });

		for (const address of SAMPLE) {
			test(`no WCAG 2.1 AA violations on ${address}`, async ({ page }) => {
				await page.goto(address, { waitUntil: 'networkidle' });
				await expect(page.locator('html')).toHaveClass(scheme === 'dark' ? /\bdark\b/ : /^(?!.*\bdark\b)/);
				expect(await axe(page)).toEqual([]);
			});
		}

		test('no violations in the search dialog with results', async ({ page }) => {
			await page.goto('/concepts/', { waitUntil: 'networkidle' });
			await page.getByTestId('search-open').click();
			await page.getByTestId('search-input').fill('receipt');
			await expect(page.getByTestId('search-results')).toBeVisible();
			expect(await axe(page)).toEqual([]);
		});

		test('no violations in the navigation sheet on a phone', async ({ page }) => {
			await page.setViewportSize({ width: 390, height: 844 });
			await page.goto('/editors/review/', { waitUntil: 'networkidle' });
			await page.getByRole('button', { name: 'Open the navigation' }).click();
			await expect(page.getByRole('dialog')).toBeVisible();
			expect(await axe(page)).toEqual([]);
		});
	});
}

test('every page in the sample reflows at 320 px without sideways scrolling', async ({ page }) => {
	await page.setViewportSize({ width: 320, height: 720 });
	for (const address of SAMPLE) {
		await page.goto(address, { waitUntil: 'networkidle' });
		const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
		expect(overflow, address).toBeLessThanOrEqual(0);
	}
});

test('the navigation sheet opens on a phone and leads to a page', async ({ page }) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await page.goto('/', { waitUntil: 'networkidle' });
	await page.getByRole('button', { name: 'Open the navigation' }).click();
	const sheet = page.getByRole('dialog');
	await sheet.getByRole('link', { name: 'Unpublishing' }).click();
	await expect(page).toHaveURL(/\/editors\/unpublishing\/$/);
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Unpublishing');
	await expect(sheet).toBeHidden();
});

test.describe('with motion', () => {
	test.use({ reducedMotion: 'no-preference' });
	test('the appearance choice changes the colours and survives a reload', async ({ page }) => {
		await page.goto('/reference/', { waitUntil: 'networkidle' });
		await page.getByRole('button', { name: 'Appearance' }).click();
		await page.getByRole('menuitemradio', { name: 'Dark' }).click();
		await expect(page.locator('html')).toHaveClass(/\bdark\b/);
		await page.reload();
		await expect(page.locator('html')).toHaveClass(/\bdark\b/);
		await page.getByRole('button', { name: 'Appearance' }).click();
		await page.getByRole('menuitemradio', { name: 'Light' }).click();
		await expect(page.locator('html')).not.toHaveClass(/\bdark\b/);
	});
});
