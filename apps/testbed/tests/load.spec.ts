import { expect, test } from '@playwright/test';

test('the load page lists the committed runs, including a million-request run that passed', async ({ page }) => {
	await page.goto('/load');
	const rows = page.getByTestId('results').locator('tbody tr');
	await expect(rows.first()).toBeVisible();
	const passedThroughput = page.locator('tr[data-scenario="throughput"][data-passed="true"]');
	await expect(passedThroughput.first()).toBeVisible();
	await expect(passedThroughput.filter({ hasText: '1,000,000' }).first()).toBeVisible();
});
