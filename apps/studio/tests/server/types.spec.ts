// The content-type builder against a real server (step 3.6; D-054, D-055): an owner creates a
// type in Settings, adds a URL name and a number, and editors then give entries URL names
// suggested from their titles, a taken one refused. A change that would break a saved entry is
// refused naming it, and goes through once the entry is fixed. Set STUDIO_SHOTS_DIR to keep
// screenshots of the builder and the editor.

import { expect, type Page, test } from '@playwright/test';
import { newProject, signIn, violations } from './helpers.ts';

const shots = process.env['STUDIO_SHOTS_DIR'];
async function shot(page: Page, name: string) {
	if (shots !== undefined) await page.screenshot({ path: `${shots}/${name}.png`, fullPage: true });
}

const status = (page: Page) => page.getByTestId('save-status');

/** Calls the API as the signed-in page; status and body. */
async function api(page: Page, method: string, path: string, body?: unknown, headers: Record<string, string> = {}) {
	return page.evaluate(
		async ({ method, path, body, headers }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const response = await fetch(path, {
				method,
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, 'Idempotency-Key': crypto.randomUUID(), ...headers },
				...(body === undefined ? {} : { body: JSON.stringify(body) })
			});
			return { status: response.status, etag: response.headers.get('etag'), body: (await response.json()) as Record<string, unknown> };
		},
		{ method, path, body, headers }
	);
}

test('an owner builds a content type in Settings, and its entries get URL names', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/settings`);
	await page.locator('#type-name').fill('Recipes');
	await expect(page.getByLabel('URL name')).toHaveValue('recipes');
	await page.getByRole('button', { name: 'Create and add fields' }).click();
	await expect(page.getByRole('heading', { level: 1, name: 'Recipes' })).toBeVisible();
	await expect(page.getByTestId('field-title')).toBeVisible();

	const add = async (label: string, type: string) => {
		await page.getByLabel('New field').fill(label);
		await page.getByLabel('Type').selectOption(type);
		await page.getByRole('button', { name: 'Add field' }).click();
	};
	await add('URL name', 'slug');
	await expect(page.getByTestId('field-url_name')).toBeVisible();
	await page.getByRole('button', { name: 'Save content type' }).click();
	await expect(page.getByText('Saved Recipes, version 2')).toBeVisible();
	await add('Serves', 'number');
	await page.getByRole('button', { name: 'Save content type' }).click();
	await expect(page.getByText('Saved Recipes, version 3')).toBeVisible();
	// A second slug field isn't offered.
	await expect(page.getByLabel('Type').locator('option[value="slug"]')).toHaveAttribute('disabled', '');
	await shot(page, 'builder');
	expect(await violations(page)).toEqual([]);

	// An entry's URL name comes from its title in one click.
	await page.goto(`/studio/projects/${project}/recipes`);
	await page.getByRole('button', { name: 'New entry' }).click();
	await page.getByLabel('Title', { exact: true }).fill('Lemon tart');
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	await page.getByRole('button', { name: 'From Title' }).click();
	await expect(page.getByLabel('URL name (optional)')).toHaveValue('lemon-tart');
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	await shot(page, 'editor-slug');
	expect(await violations(page)).toEqual([]);

	// Another entry can't take it: the save is refused, saying so.
	await page.goto(`/studio/projects/${project}/recipes`);
	await page.getByRole('button', { name: 'New entry' }).click();
	await page.getByLabel('Title', { exact: true }).fill('Lemon tart, again');
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	await page.getByLabel('URL name (optional)').fill('lemon-tart');
	await expect(status(page)).toHaveAttribute('data-status', 'refused');
	await expect(status(page)).toContainText('another entry already uses this URL name (“Lemon tart”)');
});

test('a change that would break a saved entry is refused naming it, and goes through once it is fixed', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const base = `/api/v1/projects/${project}/environments/production`;
	const created = await api(page, 'POST', `${base}/content-types`, {
		slug: 'notes',
		definition: {
			name: 'Notes',
			title_field: 'title',
			fields: [
				{ id: 'title', label: 'Title', type: 'text', required: true },
				{ id: 'subtitle', label: 'Subtitle', type: 'text' }
			]
		}
	});
	expect(created.status).toBe(201);
	const note = await api(page, 'POST', `${base}/collections/notes/entries`, { title: 'Kettle', subtitle: 'descaled' });
	expect(note.status).toBe(201);

	await page.goto(`/studio/projects/${project}/settings/types/notes`);
	await page.getByRole('button', { name: 'Remove Subtitle' }).click();
	await page.getByRole('button', { name: 'Save content type' }).click();
	const refusal = page.getByTestId('type-refusal');
	await expect(refusal).toContainText('1 entry is in the way');
	await expect(refusal.getByRole('link', { name: 'Kettle' })).toBeVisible();
	await shot(page, 'builder-refused');
	expect(await violations(page)).toEqual([]);

	// Clear the subtitle, then the same change goes through.
	const id = (note.body['data'] as { id: string }).id;
	const saved = await api(page, 'PUT', `${base}/collections/notes/entries/${id}`, { title: 'Kettle' }, { 'If-Match': note.etag ?? '' });
	expect(saved.status).toBe(200);
	await page.getByRole('button', { name: 'Save content type' }).click();
	await expect(page.getByText('Saved Notes, version 2')).toBeVisible();
	await expect(page.getByTestId('field-subtitle')).toHaveCount(0);
});
