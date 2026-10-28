import { expect, test } from '@playwright/test';

test('the load page lists the committed runs, including a million-request run that passed', async ({ page }) => {
	await page.goto('/load');
	const rows = page.getByTestId('results').locator('tbody tr');
	await expect(rows.first()).toBeVisible();
	const passedThroughput = page.locator('tr[data-scenario="throughput"][data-passed="true"]');
	await expect(passedThroughput.first()).toBeVisible();
	await expect(passedThroughput.filter({ hasText: '1,000,000' }).first()).toBeVisible();
});

test('stress runs have a table of their own, so their shape never breaks the runs table', async ({ page }) => {
	await page.goto('/load');
	await expect(page.getByRole('heading', { name: 'Stress runs' })).toBeVisible();
	const modelling = page.getByTestId('stress').locator('tr[data-endpoint="modelling"]');
	await expect(modelling.first()).toBeVisible();
	await expect(modelling.first()).toContainText('737');
	await expect(page.getByTestId('results').locator('tr[data-scenario="stress"]')).toHaveCount(0);
});
