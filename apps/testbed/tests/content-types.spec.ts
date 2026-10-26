// Content types through the testbed, in a real browser: create one, change it safely, and see
// an unsafe change and an out-of-date one refused, each with its reason.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch('/server/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'Content types testbed' })
		});
		return ((await response.json()) as { data: { id: string } }).data.id;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

const products = {
	name: 'Products',
	title_field: 'name',
	fields: [
		{ id: 'name', label: 'Name', type: 'text', required: true },
		{ id: 'price', label: 'Price', type: 'number', min: 0 }
	]
};

test('define a content type, change it safely, and see unsafe and stale changes refused', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/content-types');
	await page.getByLabel('Project').selectOption(projectId);
	await expect(page.getByTestId('types').locator('[data-slug="articles"]')).toContainText('v1');

	await page.getByLabel('Slug').fill('products');
	await page.getByLabel('Definition', { exact: true }).fill(JSON.stringify(products));
	await page.getByRole('button', { name: 'Create content type' }).click();
	await expect(page.getByTestId('message')).toHaveText('created products v1');
	await expect(page.getByTestId('types').locator('[data-slug="products"]')).toContainText('v1, 2 fields');

	// A safe change: a new optional field.
	await page.getByRole('button', { name: 'Products' }).click();
	await expect(page.getByTestId('editing')).toHaveAttribute('data-etag', '"v1"');
	const wider = { ...products, fields: [...products.fields, { id: 'in_stock', label: 'In stock', type: 'boolean' }] };
	await page.getByLabel('Current definition').fill(JSON.stringify(wider));
	await page.getByRole('button', { name: 'Save definition' }).click();
	await expect(page.getByTestId('message')).toHaveText('updated products to v2');

	// Saving again from the same (now old) version is refused as stale.
	await page.getByRole('button', { name: 'Save definition' }).click();
	await expect(page.getByTestId('message')).toHaveText('409 conflict (stale_version)');

	// Removing a field could break stored entries: it needs a migration.
	await page.getByRole('button', { name: 'Products' }).click();
	await expect(page.getByTestId('editing')).toHaveAttribute('data-etag', '"v2"');
	await page.getByLabel('Current definition').fill(JSON.stringify({ ...products, fields: [products.fields[0]] }));
	await page.getByRole('button', { name: 'Save definition' }).click();
	await expect(page.getByTestId('message')).toContainText('422 validation_failed (needs_migration');
});
