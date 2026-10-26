// Step 3.5: the design system (D-053) on the Studio's playground. Accessibility in light and dark,
// with and without the system's "more contrast"; keyboard behaviour; the appearance choice; and
// no requests to anyone else's servers.

import AxeBuilder from '@axe-core/playwright';
import { expect, type Page, test } from '@playwright/test';
import { openFixture } from './helpers.ts';

async function choose(page: Page, mode: 'Light' | 'Dark' | 'System') {
	await page.getByRole('button', { name: 'Appearance' }).click();
	await page.getByRole('menuitemradio', { name: mode }).click();
}

async function axe(page: Page) {
	const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa']).analyze();
	return results.violations.map((violation) => `${violation.id}: ${violation.help} (${violation.nodes.map((node) => node.target.join(' ')).join(', ')})`);
}

const token = (page: Page, name: string) =>
	page.evaluate((property) => getComputedStyle(document.documentElement).getPropertyValue(property).trim(), name);

for (const mode of ['Light', 'Dark'] as const) {
	for (const contrast of ['no-preference', 'more'] as const) {
		test(`no accessibility violations: ${mode.toLowerCase()}, ${contrast === 'more' ? 'more' : 'standard'} contrast`, async ({ page }) => {
			await page.emulateMedia({ contrast });
			await page.goto('/studio/playground');
			await choose(page, mode);
			await expect(page.locator('html')).toHaveClass(mode === 'Dark' ? /\bdark\b/ : /^(?!.*\bdark\b)/);
			await openFixture(page, 'valid/12-blog-post');
			expect(await axe(page), 'editor').toEqual([]);
			await page.getByRole('tab', { name: 'Preview' }).click();
			expect(await axe(page), 'preview').toEqual([]);
			await page.getByRole('button', { name: 'Image', exact: true }).click();
			await expect(page.getByRole('dialog')).toBeVisible();
			expect(await axe(page), 'image dialog').toEqual([]);
		});
	}
}

test('the formatting toolbar is one tab stop, with arrow keys inside it that wrap', async ({ page }) => {
	await page.goto('/studio/playground');
	await page.getByRole('toolbar', { name: 'Formatting' }).getByRole('button').first().focus();
	await expect(page.getByTitle('Bold (⌘B)')).toBeFocused();
	await page.keyboard.press('ArrowRight');
	await expect(page.getByTitle('Italic (⌘I)')).toBeFocused();
	await page.keyboard.press('ArrowLeft');
	await page.keyboard.press('ArrowLeft');
	await expect(page.getByTitle('Redo (⌘⇧Z)')).toBeFocused();
	// Only one button in the toolbar is in the tab order.
	const tabbable = await page.getByRole('toolbar', { name: 'Formatting' }).locator('button[tabindex="0"]').count();
	expect(tabbable).toBe(1);
});

test('the appearance choice changes the colours and survives a reload', async ({ page }) => {
	await page.goto('/studio/playground');
	await choose(page, 'Light');
	const light = await token(page, '--background');
	await choose(page, 'Dark');
	await expect(page.locator('html')).toHaveClass(/\bdark\b/);
	const dark = await token(page, '--background');
	expect(dark).not.toBe(light);
	await page.reload();
	await expect(page.locator('html')).toHaveClass(/\bdark\b/);
	expect(await token(page, '--background')).toBe(dark);
	// "System" follows the operating system's scheme.
	await choose(page, 'System');
	await page.emulateMedia({ colorScheme: 'light' });
	await expect(page.locator('html')).not.toHaveClass(/\bdark\b/);
});

test('asking the system for more contrast darkens secondary text without a setting', async ({ page }) => {
	await page.goto('/studio/playground');
	await choose(page, 'Light');
	const standard = await token(page, '--muted-foreground');
	await page.emulateMedia({ contrast: 'more' });
	expect(await token(page, '--muted-foreground')).not.toBe(standard);
});

test('the image dialog closes on Escape and gives focus back', async ({ page }) => {
	await page.goto('/studio/playground');
	const opener = page.getByRole('button', { name: 'Image', exact: true });
	await opener.click();
	await expect(page.getByRole('dialog', { name: 'Insert an image' })).toBeVisible();
	await expect(page.getByLabel('Alt text')).toBeFocused();
	await page.keyboard.press('Escape');
	await expect(page.getByRole('dialog')).toHaveCount(0);
	await expect(opener).toBeFocused();
});

test('the Studio makes no requests to anyone else: the fonts are self-hosted', async ({ page, baseURL }) => {
	const foreign: string[] = [];
	page.on('request', (request) => {
		if (!request.url().startsWith(baseURL ?? '') && !request.url().startsWith('data:')) foreign.push(request.url());
	});
	await page.goto('/studio/playground');
	await openFixture(page, 'valid/12-blog-post');
	await page.getByRole('tab', { name: 'Preview' }).click();
	await page.waitForLoadState('networkidle');
	expect(foreign).toEqual([]);
	expect(await page.evaluate(() => getComputedStyle(document.body).fontFamily)).toContain('Inter Variable');
	expect(await page.evaluate(() => document.fonts.check('16px "Newsreader Variable"'))).toBe(true);
});
