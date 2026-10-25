import { expect, test } from '@playwright/test';
import { checks } from '../src/lib/contract.ts';

test('every API contract check passes from the browser', async ({ page }) => {
	await page.goto('/contract');
	await page.getByRole('button', { name: 'Run all checks' }).click();
	await expect(page.getByTestId('summary')).toHaveText(`${checks.length} of ${checks.length} passed`, { timeout: 30_000 });
	for (const check of checks) {
		await expect(page.getByTestId(`check-${check.id}`), check.title).toHaveAttribute('data-result', 'pass');
	}
});
