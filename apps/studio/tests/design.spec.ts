// Step 0.7: the Material 3 design system on the Studio's first screen. Accessibility in every
// theme, keyboard behaviour, appearance choices, and no requests to anyone else's servers.

import AxeBuilder from '@axe-core/playwright';
import { expect, type Page, test } from '@playwright/test';
import { openFixture } from './helpers.ts';

const THEMES = [
	{ theme: 'light', contrast: 'standard' },
	{ theme: 'dark', contrast: 'standard' },
	{ theme: 'light', contrast: 'high' },
	{ theme: 'dark', contrast: 'high' }
] as const;

async function setAppearance(page: Page, theme: string, contrast: string) {
	await page.getByRole('button', { name: 'Appearance' }).click();
	await page.getByRole('menuitemradio', { name: theme[0]?.toUpperCase() + theme.slice(1) }).first().click();
	await page.getByRole('button', { name: 'Appearance' }).click();
	const contrastItems = page.getByRole('menuitemradio', { name: contrast[0]?.toUpperCase() + contrast.slice(1) });
	await contrastItems.last().click();
}

async function axe(page: Page) {
	const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa']).analyze();
	return results.violations.map((violation) => `${violation.id}: ${violation.help} (${violation.nodes.map((node) => node.target.join(' ')).join(', ')})`);
}

for (const { theme, contrast } of THEMES) {
	test(`no accessibility violations: ${theme}, ${contrast} contrast`, async ({ page }) => {
		await page.goto('/studio/playground');
		await setAppearance(page, theme, contrast);
		await expect(page.locator('html')).toHaveAttribute('data-theme', theme);
		await openFixture(page, 'valid/12-blog-post');
		expect(await axe(page), 'editor').toEqual([]);
		await page.getByRole('tab', { name: 'Preview' }).click();
		expect(await axe(page), 'preview').toEqual([]);
		await page.getByRole('button', { name: 'Image', exact: true }).click();
		await expect(page.getByRole('dialog')).toBeVisible();
		expect(await axe(page), 'image dialog').toEqual([]);
	});
}

test('the formatting toolbar is one tab stop, with arrow keys inside it', async ({ page }) => {
	await page.goto('/studio/playground');
	await page.getByRole('button', { name: 'Appearance' }).focus();
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

test('appearance choices change the colours and survive a reload', async ({ page }) => {
	await page.goto('/studio/playground');
	// Material keeps surfaces fixed across contrast levels and moves text and outlines instead, so
	// the outline colour is the one that changes in every step below.
	const surface = () => page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--md-sys-color-outline').trim());
	await setAppearance(page, 'light', 'standard');
	const light = await surface();
	await setAppearance(page, 'dark', 'standard');
	const dark = await surface();
	expect(dark).not.toBe(light);
	await setAppearance(page, 'dark', 'high');
	await expect(page.locator('html')).toHaveAttribute('data-contrast', 'high');
	const darkHigh = await surface();
	expect(darkHigh).not.toBe(dark);
	await page.reload();
	await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
	await expect(page.locator('html')).toHaveAttribute('data-contrast', 'high');
	expect(await surface()).toBe(darkHigh);
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

test('the Studio makes no requests to anyone else: the font is self-hosted', async ({ page, baseURL }) => {
	const foreign: string[] = [];
	page.on('request', (request) => {
		if (!request.url().startsWith(baseURL ?? '') && !request.url().startsWith('data:')) foreign.push(request.url());
	});
	await page.goto('/studio/playground');
	await openFixture(page, 'valid/12-blog-post');
	await page.getByRole('tab', { name: 'Preview' }).click();
	await page.waitForLoadState('networkidle');
	expect(foreign).toEqual([]);
	const font = await page.evaluate(() => getComputedStyle(document.body).fontFamily);
	expect(font).toContain('Roboto Flex');
});
