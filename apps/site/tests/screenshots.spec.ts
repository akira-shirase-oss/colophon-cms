// Screenshots of every page at desktop and phone widths, light and dark, for review. Runs only
// when SITE_SHOTS_DIR names a folder: `SITE_SHOTS_DIR=/tmp/shots npm test -w @colophon/site -- screenshots`.
import { test } from '@playwright/test';
import path from 'node:path';
import { PAGES } from './pages.ts';

const directory = process.env['SITE_SHOTS_DIR'];
const SIZES = [
	{ name: 'desktop', width: 1440, height: 900 },
	{ name: 'mobile', width: 390, height: 844 }
] as const;

test.describe('screenshots', () => {
	test.skip(!directory, 'set SITE_SHOTS_DIR to take screenshots');

	for (const size of SIZES) {
		for (const scheme of ['light', 'dark'] as const) {
			test(`${size.name}, ${scheme}`, async ({ page }) => {
				test.setTimeout(120_000);
				await page.setViewportSize({ width: size.width, height: size.height });
				await page.emulateMedia({ colorScheme: scheme, reducedMotion: 'reduce' });
				for (const { path: route, name } of PAGES) {
					await page.goto(route);
					await page.waitForLoadState('networkidle');
					await page.screenshot({ path: path.join(directory ?? '', `${name}-${size.name}-${scheme}.png`), fullPage: true });
				}
			});
		}
	}
});
