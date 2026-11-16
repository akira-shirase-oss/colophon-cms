// A content-schema migration through the testbed, in a real browser (step M.5c; D-100, D-101):
// an entry in the way of a type change is named by the plan's dry run, resolved with a
// replacement document, and the hash-bound approve and apply start a run the worker drives to
// done — the page's Refresh follows it.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch('/server/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'Migrations testbed' })
		});
		return ((await response.json()) as { data: { id: string } }).data.id;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

/** The `articles` type's current definition (the JSON a plan's target is written from). */
async function currentDefinition(page: Page, projectId: string): Promise<Record<string, unknown>> {
	return page.evaluate(async (path) => {
		const reply = await fetch(path);
		return ((await reply.json()) as { data: { definition: Record<string, unknown> } }).data.definition;
	}, `/server/api/v1/projects/${projectId}/environments/production/content-types/articles`);
}

/** One `GET` of a JSON API route as the page; `data`. */
async function get(page: Page, path: string): Promise<Record<string, any>> {
	return page.evaluate(async (path) => ((await (await fetch(path)).json()) as { data: Record<string, any> }).data, path);
}

test('plan, resolve, approve and apply a migration the worker drives to done', async ({ page }) => {
	test.setTimeout(90_000);
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	const env = `/server/api/v1/projects/${projectId}/environments/production`;

	// An entry the target can't accept: the change adds a required `subtitle` it lacks.
	await page.evaluate(async ({ env }) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch(`${env}/collections/articles/entries`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, 'Idempotency-Key': crypto.randomUUID() },
			body: JSON.stringify({ title: 'Launch post', body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } })
		});
		if (response.status !== 201) throw new Error(`entry: ${response.status}`);
	}, { env });

	const target = await currentDefinition(page, projectId);
	(target.fields as unknown[]).push({ id: 'subtitle', label: 'Subtitle', type: 'text', required: true, max_chars: 100, multiline: false });

	await page.goto('/migrations');
	await page.getByLabel('Project').selectOption(projectId);
	await page.getByLabel('Collection').fill('articles');
	await page.getByLabel('Target definition').fill(JSON.stringify(target));
	await page.getByRole('button', { name: 'Plan migration' }).click();
	await expect(page.getByTestId('message')).toContainText('1 of 1 in the way');
	const planSection = page.getByTestId('plan');
	await expect(planSection).toHaveAttribute('data-state', 'proposed');
	const hash = await planSection.getAttribute('data-hash');
	expect(hash).toMatch(/^sha256:[0-9a-f]{64}$/);

	// The dry run names the entry; a document that fits resolves it.
	const items = page.getByTestId('items');
	await expect(items.locator('li')).toHaveCount(1);
	await expect(items.locator('li').first()).toHaveAttribute('data-outcome', 'needs_input');
	await expect(items.locator('li').first()).toContainText('missing_field');
	await items.getByRole('button', { name: 'resolve' }).click();
	await page.getByLabel(/Resolution for/).fill(
		JSON.stringify({
			title: 'Launch post',
			subtitle: 'the short version',
			body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } }
		})
	);
	await page.getByRole('button', { name: 'Save resolution' }).click();
	await expect(page.getByTestId('message')).toContainText('resolved ');
	await expect(items.locator('li').first()).toHaveAttribute('data-outcome', 'resolved');

	// Approve and apply bind the plan's hash; apply starts a run a worker drives (D-101).
	await page.getByRole('button', { name: 'Approve' }).click();
	await expect(page.getByTestId('message')).toContainText('approve: approved');
	await page.getByRole('button', { name: 'Apply' }).click();
	await expect(page.getByTestId('message')).toContainText('apply: running');
	const id = (await page.getByTestId('plans').locator('li').first().getAttribute('data-id')) ?? '';
	await expect
		.poll(async () => (await get(page, `${env}/migrations/${id}`))['state'], { timeout: 30_000 })
		.toBe('done');
	await page.getByRole('button', { name: 'Refresh' }).click();
	await expect(planSection).toHaveAttribute('data-state', 'done');
	await expect(items.locator('li').first()).toHaveAttribute('data-outcome', 'applied');

	// The entry's draft is the resolved document, stamped on the run's target version.
	const list = await get(page, `${env}/migrations/${id}/items?limit=10`);
	expect(list[0]['new_revision_id']).toBeTruthy();
});
