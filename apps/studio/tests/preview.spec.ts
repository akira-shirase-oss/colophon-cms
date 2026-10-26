// Invariant R1 (module 12): the Studio preview is the reviewed renderer output, exactly.

import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, test } from '@playwright/test';
import { fixtures, openFixture } from './helpers.ts';

const rendered = join(import.meta.dirname, '../../../fixtures/rich-text/v1/rendered');

test.describe('the preview matches the reviewed rendering of every valid fixture', () => {
	for (const fixture of fixtures('valid')) {
		test(fixture.name, async ({ page }) => {
			await page.goto('/studio/playground');
			await openFixture(page, `valid/${fixture.name}`);
			await page.getByRole('tab', { name: 'Preview' }).click();
			// The reviewed output uses a test asset origin; the Studio previews images as a placeholder.
			const expected = readFileSync(join(rendered, `${fixture.name}.html`), 'utf8')
				.trim()
				.replaceAll('https://assets.example/', '/studio/placeholder.svg#');
			// Compare DOMs, not strings: the browser re-serializes HTML (it writes &#39; as ').
			const same = await page.evaluate((html) => {
				const template = document.createElement('template');
				template.innerHTML = html;
				const preview = document.querySelector('[data-testid="preview"] .rich-text');
				return preview !== null && preview.innerHTML === template.innerHTML;
			}, expected);
			expect(same).toBe(true);
		});
	}
});

test('the preview follows the editor as you type', async ({ page }) => {
	await page.goto('/studio/playground');
	await page.locator('.tiptap').click();
	await page.keyboard.type('## Live heading');
	await page.getByRole('tab', { name: 'Preview' }).click();
	await expect(page.getByTestId('preview').locator('h2')).toHaveText('Live heading');
	await expect(page.getByTestId('preview').locator('script')).toHaveCount(0);
});

test('a document the renderer refuses is explained in the preview, and the Studio keeps working', async ({ page }) => {
	await page.goto('/studio/playground');
	await openFixture(page, 'invalid/12-javascript-link');
	await page.getByRole('tab', { name: 'Preview' }).click();
	await expect(page.getByTestId('preview-refused')).toContainText("can't be previewed: a link that is not allowed");
	// The editor still works: open a valid document and preview it.
	await openFixture(page, 'valid/02-marks');
	await expect(page.getByTestId('preview').locator('strong').first()).toHaveText('bold');
});
