// Scheduled publishing through the testbed (D-060): an approved changeset scheduled a few seconds
// ahead is published by the worker as release 1; a New York time the clocks show twice is refused
// with both candidates, then scheduled with a choice and cancelled; a past time is refused.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

/** A project with an approved changeset of one article: (project, changeset, hash). */
async function setup(page: Page, title: string): Promise<{ project: string; changeset: string; hash: string }> {
	return page.evaluate(
		async ({ slug, title }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const json = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
			const post = async (path: string, body: unknown, headers: Record<string, string> = {}) => {
				const reply = await fetch(`/server${path}`, { method: 'POST', headers: { ...json, ...headers }, body: JSON.stringify(body) });
				return (await reply.json()) as { data: { id: string; hash: string } };
			};
			const project = (await post('/api/v1/projects', { slug, name: 'Schedules testbed' })).data.id;
			const base = `/api/v1/projects/${project}/environments/production`;
			const doc = { type: 'doc', content: [{ type: 'paragraph' }] };
			const entry = await post(`${base}/collections/articles/entries`, { title, body: { schema: 'colophon/rich-text', version: 1, doc } }, { 'Idempotency-Key': crypto.randomUUID() });
			const set = await post(`${base}/changesets`, { title, entries: [entry.data.id] }, { 'Idempotency-Key': crypto.randomUUID() });
			const ifMatch = { 'If-Match': `"${set.data.hash}"` };
			await post(`${base}/changesets/${set.data.id}/submit`, undefined, ifMatch);
			await post(`${base}/changesets/${set.data.id}/reviews`, { decision: 'approve' }, ifMatch);
			return { project, changeset: set.data.id, hash: set.data.hash };
		},
		{ slug: `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`, title }
	);
}

test('schedule, publish on time, choose a doubled time, cancel, refuse the past', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const { project, changeset, hash } = await setup(page, 'Scheduled note');
	await page.goto('/schedules');
	await page.getByLabel('Project').selectOption(project);
	await page.getByLabel('Changeset id').fill(changeset);
	await page.getByLabel('If-Match hash').fill(hash);

	// Four seconds ahead in UTC: the worker publishes it.
	await page.getByLabel('Local time').fill(new Date(Date.now() + 4000).toISOString().slice(0, 19));
	await page.getByRole('button', { name: 'Schedule', exact: true }).click();
	await expect(page.getByTestId('message')).toContainText('scheduled for');
	await expect(async () => {
		await page.getByRole('button', { name: 'Refresh' }).click();
		await expect(page.getByTestId('schedules').locator('li').first()).toHaveAttribute('data-state', 'published', { timeout: 1000 });
	}).toPass({ timeout: 20_000 });
	await expect(page.getByTestId('schedules').locator('li').first()).toContainText('as release 1');

	// A second approved changeset, for New York's doubled 01:30 on 1 November 2026.
	const second = await setup(page, 'Doubled note');
	await page.reload();
	await page.getByLabel('Project').selectOption(second.project);
	await page.getByLabel('Changeset id').fill(second.changeset);
	await page.getByLabel('If-Match hash').fill(second.hash);
	await page.getByLabel('Local time').fill('2026-11-01T01:30');
	await page.getByLabel('Time zone').fill('America/New_York');
	await page.getByRole('button', { name: 'Schedule', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('422 validation_failed (ambiguous_local_time)');
	await expect(page.getByTestId('candidates')).toHaveText('2026-11-01T05:30:00Z or 2026-11-01T06:30:00Z');
	await page.getByLabel('Choice').selectOption('earlier');
	await page.getByRole('button', { name: 'Schedule', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('scheduled for 2026-11-01T05:30:00Z');
	await page.getByRole('button', { name: 'Cancel Doubled note' }).click();
	await expect(page.getByTestId('message')).toHaveText('cancelled');

	await page.getByLabel('Choice').selectOption('');
	await page.getByLabel('Time zone').fill('UTC');
	await page.getByLabel('Local time').fill('2020-01-01T00:00');
	await page.getByRole('button', { name: 'Schedule', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('422 validation_failed (in_the_past)');
});
