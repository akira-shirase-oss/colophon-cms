import { readdirSync } from 'node:fs';
import { join } from 'node:path';
import { expect, test } from '@playwright/test';

const root = join(import.meta.dirname, '../../../fixtures/rich-text/v1');
const total = ['valid', 'invalid'].reduce(
	(sum, kind) => sum + readdirSync(join(root, kind)).filter((file) => file.endsWith('.json')).length,
	0
);

test('the server agrees with every shared rich-text fixture, through the real HTTP API', async ({ page }) => {
	await page.goto('/rich-text');
	await page.getByRole('button', { name: `Check all ${total} fixtures` }).click();
	await expect(page.getByTestId('summary')).toHaveText(`${total} of ${total} agree`, { timeout: 30_000 });
	await expect(page.locator('[data-result="fail"]')).toHaveCount(0);
});

test('a document typed by hand gets the server verdict', async ({ page }) => {
	await page.goto('/rich-text');
	await page.getByLabel('Rich-text document').fill('{"schema":"colophon/rich-text","version":1,"doc":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"x","marks":[{"type":"link","attrs":{"href":"javascript:alert(1)"}}]}]}]}}');
	await page.getByRole('button', { name: 'Validate' }).click();
	await expect(page.getByTestId('try-status')).toContainText('HTTP 422');
	await expect(page.locator('pre')).toContainText('"reason":"unsafe_link"');
});
