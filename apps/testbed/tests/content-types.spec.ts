// Content types through the testbed, in a real browser: create one, change it safely, see an
// out-of-date change refused, see a change refused while an entry is in its way (and named), and
// accepted when none is (D-055); give entries slugs and see a taken one refused (D-054).

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

	// An entry uses `price`: removing it is refused, naming the entry (D-055).
	const kettle = await page.evaluate(
		async ({ project, body }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const response = await fetch(`/server/api/v1/projects/${project}/environments/production/collections/products/entries`, {
				method: 'POST',
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, 'Idempotency-Key': crypto.randomUUID() },
				body: JSON.stringify(body)
			});
			return response.status;
		},
		{ project: projectId, body: { name: 'Kettle', price: 20 } }
	);
	expect(kettle).toBe(201);
	await page.getByRole('button', { name: 'Products' }).click();
	await expect(page.getByTestId('editing')).toHaveAttribute('data-etag', '"v2"');
	await page.getByLabel('Current definition').fill(JSON.stringify({ ...wider, fields: [wider.fields[0], wider.fields[2]] }));
	await page.getByRole('button', { name: 'Save definition' }).click();
	await expect(page.getByTestId('message')).toContainText('422 validation_failed (needs_migration');
	await expect(page.getByTestId('message')).toContainText('in the way: Kettle (draft, unknown_field)');

	// Nobody uses `in_stock`: removing it goes through.
	await page.getByLabel('Current definition').fill(JSON.stringify({ ...wider, fields: wider.fields.slice(0, 2) }));
	await page.getByRole('button', { name: 'Save definition' }).click();
	await expect(page.getByTestId('message')).toHaveText('updated products to v3');
});

test('a slug field gives each entry its own URL name', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/content-types');
	await page.getByLabel('Project').selectOption(projectId);
	const pages = {
		name: 'Pages',
		title_field: 'title',
		fields: [
			{ id: 'title', label: 'Title', type: 'text', required: true },
			{ id: 'slug', label: 'URL name', type: 'slug', source: 'title' }
		]
	};
	await page.getByLabel('Slug').fill('pages');
	await page.getByLabel('Definition', { exact: true }).fill(JSON.stringify(pages));
	await page.getByRole('button', { name: 'Create content type' }).click();
	await expect(page.getByTestId('message')).toHaveText('created pages v1');

	const create = (body: object) =>
		page.evaluate(
			async ({ project, body }) => {
				const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
				const response = await fetch(`/server/api/v1/projects/${project}/environments/production/collections/pages/entries`, {
					method: 'POST',
					headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, 'Idempotency-Key': crypto.randomUUID() },
					body: JSON.stringify(body)
				});
				return { status: response.status, body: (await response.json()) as { error?: { details?: { reason?: string; entries?: { title: string }[] } } } };
			},
			{ project: projectId, body }
		);
	expect((await create({ title: 'About', slug: 'about' })).status).toBe(201);
	const taken = await create({ title: 'About us', slug: 'about' });
	expect(taken.status).toBe(409);
	expect(taken.body.error?.details?.reason).toBe('slug_taken');
	expect(taken.body.error?.details?.entries?.[0]?.title).toBe('About');
	expect((await create({ title: 'About us', slug: 'About Us' })).body.error?.details?.reason).toBe('invalid_slug');
});
