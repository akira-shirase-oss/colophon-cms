// Every page of the built site renders: no console errors, exactly one h1, a capability label,
// a title, and not a single request to another origin (module 14, U5: nothing loads from
// anyone else's servers).

import { expect, test } from '@playwright/test';
import { builtPages } from './site.ts';

const pages = builtPages();

test('the build has every section', () => {
	for (const prefix of ['/get-started/', '/editors/', '/concepts/', '/reference/', '/operations/', '/design/']) {
		expect(pages.filter((page) => page.startsWith(prefix)).length, prefix).toBeGreaterThan(0);
	}
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

		const response = await page.goto(address, { waitUntil: 'networkidle' });
		expect(response?.status(), 'status').toBe(200);
		await expect(page.locator('h1')).toHaveCount(1);
		await expect(page.locator('h1')).not.toBeEmpty();
		expect(await page.locator('[data-capability]').count(), 'a capability label').toBeGreaterThan(0);
		expect((await page.title()).length, 'a title').toBeGreaterThan(0);
		expect(problems, 'console').toEqual([]);
		expect(foreign, 'requests to other origins').toEqual([]);
	});
}
