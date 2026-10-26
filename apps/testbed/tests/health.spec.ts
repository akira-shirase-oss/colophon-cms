import { expect, test } from '@playwright/test';

test('the health page shows a ready server with both dependencies up', async ({ page }) => {
	await page.goto('/');
	await expect(page.getByTestId('status')).toHaveAttribute('data-status', 'ready');
	await expect(page.getByTestId('http-status')).toHaveText('200');
	await expect(page.getByTestId('check-database')).toHaveAttribute('data-state', 'up');
	await expect(page.getByTestId('check-cache')).toHaveAttribute('data-state', 'up');
});

test('navigation reaches every testbed page', async ({ page }) => {
	await page.goto('/');
	await page.getByRole('link', { name: 'API contract' }).click();
	await expect(page.getByRole('heading', { name: 'API contract' })).toBeVisible();
	await page.getByRole('link', { name: 'Entries' }).click();
	await expect(page.getByRole('heading', { name: 'Entries', exact: true })).toBeVisible();
	await page.getByRole('link', { name: 'Content types' }).click();
	await expect(page.getByRole('heading', { name: 'Content types', exact: true })).toBeVisible();
	await page.getByRole('link', { name: 'Load tests' }).click();
	await expect(page.getByRole('heading', { name: 'Load tests' })).toBeVisible();
	await page.getByRole('link', { name: 'Health' }).click();
	await expect(page.getByRole('heading', { name: 'Health' })).toBeVisible();
});
