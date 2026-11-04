// A bulk job through the testbed, in a real browser (step P5; D-116): a manifest of exact
// entries and base revisions submitted sealed, the worker applying it per item, the page's
// job and item views telling the truth — applied with the revision it made, never "all or
// nothing".

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

const key = () => `tb-${crypto.randomUUID()}`;

async function api(page: Page, method: string, path: string, body?: unknown, extra: Record<string, string> = {}) {
	return page.evaluate(
		async ({ method, path, body, extra }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const response = await fetch(`/server${path}`, {
				method,
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, 'Idempotency-Key': `tb-${crypto.randomUUID()}`, ...extra },
				body: body === undefined ? undefined : JSON.stringify(body)
			});
			return { status: response.status, body: (await response.json().catch(() => null)) as unknown, etag: response.headers.get('etag') };
		},
		{ method, path, body, extra }
	);
}

async function newProject(page: Page): Promise<string> {
	const made = await api(page, 'POST', '/api/v1/projects', { slug: `tb-${Date.now().toString(36)}`, name: 'Bulk testbed' });
	return (made.body as { data: { id: string } }).data.id;
}

/** A draft article: `{id, revision}`. */
async function draft(page: Page, projectId: string, title: string): Promise<{ id: string; revision: string }> {
	const made = await api(page, 'POST', `/api/v1/projects/${projectId}/environments/production/collections/articles/entries`, {
		title,
		body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } }
	});
	expect(made.status).toBe(201);
	const data = (made.body as { data: { id: string; revision: { id: string } } }).data;
	return { id: data.id, revision: data.revision.id };
}

test('a sealed bulk job runs its items and the page reports them', async ({ page }) => {
	await signIn(page);
	const projectId = await newProject(page);
	const first = await draft(page, projectId, 'bulk one');
	const second = await draft(page, projectId, 'bulk two');

	await page.goto('/bulk');
	await page.getByLabel('Project').selectOption(projectId);
	await page.getByLabel('Manifest (JSON array of items)').fill(
		JSON.stringify([
			{ key: 'one', entry: first.id, expected_revision: first.revision, op: { op: 'set', path: 'title', value: 'bulk one (done)' } },
			{ key: 'two', entry: second.id, expected_revision: second.revision, op: { op: 'set', path: 'title', value: 'bulk two (done)' } }
		], null, 2)
	);
	await page.getByRole('button', { name: 'Submit job' }).click();
	await expect(page.getByTestId('message')).toContainText('submitted');

	// The worker drives the run; the page's Refresh follows it to done.
	for (let attempt = 0; attempt < 60; attempt += 1) {
		await page.getByRole('button', { name: 'Refresh' }).click();
		const job = await api(page, 'GET', `/api/v1/projects/${projectId}/environments/production/bulk`);
		const state = (job.body as { data: { state: string }[] }).data[0]?.state;
		if (state === 'done') break;
		await page.waitForTimeout(500);
	}
	const jobs = await api(page, 'GET', `/api/v1/projects/${projectId}/environments/production/bulk`);
	expect((jobs.body as { data: { state: string }[] }).data[0]?.state).toBe('done');

	// Open the job: both items applied, each naming its revision.
	await page.getByTestId('jobs').getByRole('button').first().click();
	await expect(page.getByTestId('items')).toContainText('one: applied');
	await expect(page.getByTestId('items')).toContainText('two: applied');
});
