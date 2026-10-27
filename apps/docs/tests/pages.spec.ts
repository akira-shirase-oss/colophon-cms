// Every page of the built site renders: no console errors, exactly one h1, a status label, a title,
// the pre-release banner, and not a single request to another origin (module 14, U5: nothing loads
// from anyone else's servers).

import { expect, test } from '@playwright/test';
import { builtPages, docs } from './site.ts';

const pages = builtPages();

test('the build has every section', () => {
	for (const prefix of ['/get-started/', '/editors/', '/concepts/', '/reference/', '/operations/', '/resources/', '/changelog/', '/releases/', '/upgrading/', '/design/']) {
		expect(pages.filter((page) => page.startsWith(prefix)).length, prefix).toBeGreaterThan(0);
	}
	// Deliberately no lower: the site map grew from 68 pages to this, and losing pages should fail.
	expect(pages.length).toBeGreaterThanOrEqual(90);
});

for (const address of pages) {
	test(`renders ${address}`, async ({ page, baseURL }) => {
		const origin = new URL(baseURL ?? '').origin;
		const problems: string[] = [];
		const foreign: string[] = [];
		page.on('console', (message) => {
			if (message.type() === 'error' || message.type() === 'warning') problems.push(`${message.type()}: ${message.text()}`);
		});
		page.on('pageerror', (error) => problems.push(`page error: ${error.message}`));
		page.on('request', (request) => {
			const url = request.url();
			if (!url.startsWith('data:') && !url.startsWith('blob:') && new URL(url).origin !== origin) foreign.push(url);
		});

		const response = await page.goto(docs(address), { waitUntil: 'networkidle' });
		expect(response?.status(), 'status').toBe(200);
		await expect(page.locator('h1')).toHaveCount(1);
		await expect(page.locator('h1')).not.toBeEmpty();
		expect(await page.locator('[data-capability]').count(), 'a status label').toBeGreaterThan(0);
		await expect(page.getByTestId('pre-release-banner'), 'the pre-release banner').toContainText('Pre-release, not for production');
		expect((await page.title()).length, 'a title').toBeGreaterThan(0);
		expect(problems, 'console').toEqual([]);
		expect(foreign, 'requests to other origins').toEqual([]);
	});
}
