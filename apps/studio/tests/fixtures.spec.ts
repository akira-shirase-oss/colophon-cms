// Every shared fixture, opened through the UI in a real browser.

import { expect, test } from '@playwright/test';
import { fixtures, openFixture, stored } from './helpers.ts';

test.describe('valid fixtures open and survive byte for byte', () => {
	for (const fixture of fixtures('valid')) {
		test(fixture.name, async ({ page }) => {
			await page.goto('/studio/playground');
			await openFixture(page, `valid/${fixture.name}`);
			await expect(page.getByTestId('open-status')).toHaveAttribute('data-kind', 'opened');
			expect(await stored(page)).toBe(JSON.stringify(fixture.value));
		});
	}
});

test.describe('invalid fixtures are refused or kept intact, never trimmed', () => {
	for (const fixture of fixtures('invalid')) {
		test(fixture.name, async ({ page }) => {
			await page.goto('/studio/playground');
			await openFixture(page, `invalid/${fixture.name}`);
			const kind = await page.getByTestId('open-status').getAttribute('data-kind');
			if (kind === 'opened') {
				expect(await stored(page)).toBe(JSON.stringify(fixture.value));
			} else {
				expect(kind).toBe('refused');
			}
		});
	}
});
