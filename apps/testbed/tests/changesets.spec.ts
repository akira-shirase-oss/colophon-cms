// Changesets through the testbed, in a real browser (D-044): pin two articles, submit, see a stale
// hash refused, approve the current one, then edit an article through the API and see the
// changeset go stale until it is refreshed to a new hash.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function setup(page: Page): Promise<{ project: string; first: string }> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const json = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
		const created = await fetch('/server/api/v1/projects', { method: 'POST', headers: json, body: JSON.stringify({ slug, name: 'Changesets testbed' }) });
		const project = ((await created.json()) as { data: { id: string } }).data.id;
		let first = '';
		for (const title of ['Alpha note', 'Beta note']) {
			const body = { title, body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } };
			const reply = await fetch(`/server/api/v1/projects/${project}/environments/production/collections/articles/entries`, {
				method: 'POST',
				headers: { ...json, 'Idempotency-Key': crypto.randomUUID() },
				body: JSON.stringify(body)
			});
			first ||= ((await reply.json()) as { data: { id: string } }).data.id;
		}
		return { project, first };
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('pin, submit, refuse a stale hash, approve, and go stale after an edit', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const { project, first } = await setup(page);
	await page.goto('/changesets');
	await page.getByLabel('Project').selectOption(project);
	await page.getByLabel('Changeset title').fill('Notes');
	await page.getByLabel('Alpha note').check();
	await page.getByLabel('Beta note').check();
	await page.getByRole('button', { name: 'Create changeset' }).click();
	await expect(page.getByTestId('state')).toHaveText('open');
	const pinned = await page.getByLabel('If-Match hash').inputValue();
	expect(pinned).toMatch(/^sha256:[0-9a-f]{64}$/);

	await page.getByRole('button', { name: 'Submit' }).click();
	await expect(page.getByTestId('state')).toHaveText('awaiting_review');
	await page.getByLabel('If-Match hash').fill(`sha256:${'0'.repeat(64)}`);
	await page.getByRole('button', { name: 'Approve' }).click();
	await expect(page.getByTestId('message')).toHaveText('409 conflict (stale_changeset)');
	await page.getByLabel('If-Match hash').fill(pinned);
	await page.getByRole('button', { name: 'Approve' }).click();
	await expect(page.getByTestId('state')).toHaveText('approved');

	// An edit through the API makes it stale; a refresh gives it a new hash and awaits review again.
	const saved = await page.evaluate(
		async ([project, entry]) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const path = `/server/api/v1/projects/${project}/environments/production/collections/articles/entries/${entry}`;
			const current = await fetch(path);
			const draft = ((await current.json()) as { data: { content: Record<string, unknown> } }).data;
			const reply = await fetch(path, {
				method: 'PUT',
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, 'If-Match': current.headers.get('etag') ?? '', 'Idempotency-Key': crypto.randomUUID() },
				body: JSON.stringify({ ...draft.content, title: 'Alpha note, edited' })
			});
			return reply.status;
		},
		[project, first]
	);
	expect(saved).toBe(200);
	await page.getByRole('button', { name: 'Reload' }).click();
	await expect(page.getByTestId('state')).toHaveText('stale');
	await page.getByRole('button', { name: 'Refresh' }).click();
	await expect(page.getByTestId('state')).toHaveText('awaiting_review');
	expect(await page.getByLabel('If-Match hash').inputValue()).not.toBe(pinned);
	await expect(page.getByTestId('reviews')).toContainText('approve by Dev Owner (earlier hash)');
});
