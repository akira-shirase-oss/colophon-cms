// Publishing through the testbed, in a real browser (D-045): release an approved changeset on
// generation 0, see a second one on the same stale base refused, release it on the right base,
// and read /content/v1 with no cookie, at the latest release and pinned to the first.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

/** A project with two approved changesets of one article each; the project id. */
async function setup(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const json = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
		const created = await fetch('/server/api/v1/projects', { method: 'POST', headers: json, body: JSON.stringify({ slug, name: 'Releases testbed' }) });
		const project = ((await created.json()) as { data: { id: string } }).data.id;
		const base = `/server/api/v1/projects/${project}/environments/production`;
		for (const title of ['First story', 'Second story']) {
			const body = { title, body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } };
			const entry = await fetch(`${base}/collections/articles/entries`, { method: 'POST', headers: { ...json, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify(body) });
			const entryId = ((await entry.json()) as { data: { id: string } }).data.id;
			const made = await fetch(`${base}/changesets`, { method: 'POST', headers: { ...json, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify({ title, entries: [entryId] }) });
			const set = ((await made.json()) as { data: { id: string; hash: string } }).data;
			const ifMatch = { ...json, 'If-Match': `"${set.hash}"` };
			await fetch(`${base}/changesets/${set.id}/submit`, { method: 'POST', headers: ifMatch });
			await fetch(`${base}/changesets/${set.id}/reviews`, { method: 'POST', headers: ifMatch, body: JSON.stringify({ decision: 'approve' }) });
		}
		return project;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('release on a base, refuse a stale base, and read releases with no cookie', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const project = await setup(page);
	await page.goto('/releases');
	await page.getByLabel('Project').selectOption(project);
	await expect(page.getByTestId('generation')).toHaveText('0');

	await page.locator('select[name="changeset"]').selectOption({ label: 'First story (approved)' });
	await page.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('published as generation 1');
	await expect(page.getByTestId('generation')).toHaveText('1');

	await page.locator('select[name="changeset"]').selectOption({ label: 'Second story (approved)' });
	await page.getByLabel('Base generation').fill('0');
	await page.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('409 conflict (generation_moved)');
	await page.locator('select[name="changeset"]').selectOption({ label: 'Second story (approved)' });
	await page.getByLabel('Base generation').fill('1');
	await page.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('published as generation 2');
	await expect(page.getByTestId('releases').getByRole('listitem')).toHaveCount(2);

	await page.getByRole('button', { name: 'Read published articles' }).click();
	await expect(page.getByTestId('delivered')).toHaveText('HTTP 200, generation 2: Second story, First story');
	await page.getByLabel('Read at generation').fill('1');
	await page.getByRole('button', { name: 'Read published articles' }).click();
	await expect(page.getByTestId('delivered')).toHaveText('HTTP 200, generation 1: First story');
});
