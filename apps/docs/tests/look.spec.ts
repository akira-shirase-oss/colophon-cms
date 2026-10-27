// The docs look like the website because they share its look, not a copy of it: both stylesheets
// import @colophon/site-theme (the one token source), the few components the two must draw the
// same are byte-for-byte the website's, the status words are the website's, and the colours and
// fonts a reader sees are the shared tokens'. The theme switch is the website's one button.

import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, test } from '@playwright/test';
import * as siteStatus from '../../site/src/lib/status.ts';
import * as docsStatus from '../src/lib/status.ts';
import { docs, REPO } from './site.ts';

const read = (...parts: string[]) => readFileSync(join(REPO, ...parts), 'utf8');

test('both apps take their tokens from the shared theme, and neither redefines them', () => {
	const theme = read('packages', 'site-theme', 'src', 'theme.css');
	for (const app of ['site', 'docs']) {
		const css = read('apps', app, 'src', 'app.css');
		expect(css, `${app} imports the design system, then the shared theme`).toMatch(/@import '@colophon\/ui\/styles\.css';\s*@import '@colophon\/site-theme\/theme\.css';/);
		const tokens = [...theme.matchAll(/^\t(--[a-z0-9-]+):/gm)].map((match) => match[1] ?? '');
		const redefined = tokens.filter((token) => new RegExp(`^\\s*${token}:`, 'm').test(css));
		expect(redefined, `${app} redefines shared tokens`).toEqual([]);
	}
});

test("the theme switch, the mark and the brand icons are the website's own files", () => {
	for (const file of ['ThemeToggle.svelte', 'Mark.svelte', 'BrandIcon.svelte']) {
		expect(read('apps', 'docs', 'src', 'lib', 'components', file), file).toBe(read('apps', 'site', 'src', 'lib', 'components', file));
	}
});

test("the status words and what they promise are the website's", () => {
	expect(docsStatus.STATUS_WORD).toEqual(siteStatus.STATUS_WORD);
	expect(docsStatus.STATUS_MEANING).toEqual(siteStatus.STATUS_MEANING);
});

for (const [scheme, background, foreground] of [
	['light', 'rgb(250, 250, 250)', 'rgb(26, 22, 21)'],
	['dark', 'rgb(18, 17, 16)', 'rgb(244, 241, 238)']
] as const) {
	test(`in ${scheme} mode a page has the website's ground, ink and type`, async ({ page }) => {
		await page.emulateMedia({ colorScheme: scheme });
		await page.goto(docs('/reference/api/'), { waitUntil: 'networkidle' });
		const look = await page.evaluate(() => {
			const body = getComputedStyle(document.body);
			const code = document.querySelector('.prose-docs :not(pre) > code');
			return { background: body.backgroundColor, color: body.color, font: body.fontFamily, mono: code ? getComputedStyle(code).fontFamily : '' };
		});
		expect(look.background).toBe(background);
		expect(look.color).toBe(foreground);
		expect(look.font).toContain('Inter');
		expect(look.mono).toContain('Fragment Mono');
	});
}

test('a page says where it stands in the website’s words', async ({ page }) => {
	await page.goto(docs('/concepts/scheduled-publishing/'), { waitUntil: 'networkidle' });
	await expect(page.locator('article header [data-capability]')).toHaveText(/Building\s*·\s*e\/schedules/);
	await page.goto(docs('/operations/backups/'), { waitUntil: 'networkidle' });
	await expect(page.locator('article header [data-capability]')).toHaveText(/Planned\s*·\s*P6/);
	await page.goto(docs('/concepts/publish-requests/'), { waitUntil: 'networkidle' });
	await expect(page.locator('article header [data-capability]')).toHaveText(/Done\s*·\s*2\.1/);
});

test.describe('with motion', () => {
	test.use({ reducedMotion: 'no-preference', colorScheme: 'light' });

	test('one button switches between light and dark, and the choice survives a reload', async ({ page }) => {
		await page.goto(docs('/reference/'), { waitUntil: 'networkidle' });
		// One button, no Light/Dark/System menu.
		await expect(page.getByRole('menuitemradio')).toHaveCount(0);
		const toggle = page.getByRole('button', { name: 'Dark mode' });
		await expect(toggle).toHaveCount(1);
		await expect(toggle).toHaveAttribute('aria-pressed', 'false');
		await expect(page.locator('html')).not.toHaveClass(/\bdark\b/);

		await toggle.click();
		await expect(page.locator('html')).toHaveClass(/\bdark\b/);
		await expect(toggle).toHaveAttribute('aria-pressed', 'true');
		await page.reload({ waitUntil: 'networkidle' });
		await expect(page.locator('html')).toHaveClass(/\bdark\b/);

		await page.getByRole('button', { name: 'Dark mode' }).click();
		await expect(page.locator('html')).not.toHaveClass(/\bdark\b/);
		await page.reload({ waitUntil: 'networkidle' });
		await expect(page.locator('html')).not.toHaveClass(/\bdark\b/);
	});
});
