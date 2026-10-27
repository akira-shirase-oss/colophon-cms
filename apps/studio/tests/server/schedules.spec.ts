// Scheduled publishing in the Studio against a real server (D-060): on an approved changeset, an
// owner picks a date, a time and a time zone; a time the clocks show twice brings up both moments
// to choose from; the schedule shows, can be cancelled, and says so. A schedule a few seconds out
// is published by the server's own worker and the Releases page shows it happen.

import { expect, type Page, test } from '@playwright/test';
import { newProject, signIn, violations } from './helpers.ts';

/** An article in an approved changeset, through the API as the signed-in page; (id, hash). */
async function approved(page: Page, project: string, title: string): Promise<{ id: string; hash: string }> {
	return page.evaluate(
		async ({ project, title }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const base = `/api/v1/projects/${project}/environments/production`;
			const send = async (path: string, body: unknown, headers: Record<string, string> = {}) => {
				const response = await fetch(`${base}${path}`, {
					method: 'POST',
					headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, ...headers },
					body: JSON.stringify(body)
				});
				return (await response.json()) as { data: { id: string; hash: string } };
			};
			const key = () => crypto.randomUUID();
			const doc = { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Body' }] }] };
			const entry = await send('/collections/articles/entries', { title, body: { schema: 'colophon/rich-text', version: 1, doc } }, { 'Idempotency-Key': key() });
			const set = await send('/changesets', { title, entries: [entry.data.id] }, { 'Idempotency-Key': key() });
			const ifMatch = { 'If-Match': `"${set.data.hash}"` };
			await send(`/changesets/${set.data.id}/submit`, undefined, ifMatch);
			await send(`/changesets/${set.data.id}/reviews`, { decision: 'approve' }, ifMatch);
			return { id: set.data.id, hash: set.data.hash };
		},
		{ project, title }
	);
}

test('a time the clocks show twice is chosen, scheduled and cancelled', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const set = await approved(page, project, 'Opening hours');
	await page.goto(`/studio/projects/${project}/changesets/${set.id}`);
	const panel = page.getByTestId('schedule-panel');
	await expect(panel).toBeVisible();

	// New York goes back from EDT to EST at 02:00 on 1 November 2026: 01:30 happens twice.
	await panel.getByLabel('Date').fill('2026-11-01');
	await panel.getByLabel('Time', { exact: true }).fill('01:30');
	await panel.getByLabel('Time zone').selectOption('America/New_York');
	await panel.getByRole('button', { name: 'Schedule', exact: true }).click();
	const choice = page.getByTestId('schedule-choice');
	await expect(choice).toContainText('happens twice');
	await expect(choice.getByRole('radio')).toHaveCount(2);
	await expect(choice).toContainText('EDT');
	await expect(choice).toContainText('EST');
	expect(await violations(page)).toEqual([]);
	await choice.getByRole('radio').nth(1).check();
	await panel.getByRole('button', { name: 'Schedule at this moment' }).click();
	const pending = page.getByTestId('schedule-pending');
	await expect(pending).toContainText('EST');
	await expect(pending).toContainText('America/New_York');

	await pending.getByRole('button', { name: 'Cancel the schedule' }).click();
	await expect(page.getByTestId('schedule-history')).toContainText('Cancelled');
	await expect(panel.getByRole('button', { name: 'Schedule', exact: true })).toBeVisible();
	expect(await violations(page)).toEqual([]);
});

test('the worker publishes a schedule when its time comes, and Releases shows it', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const set = await approved(page, project, 'Spring menu');
	// Four seconds ahead, in UTC, to the second: through the API, as the form only takes minutes.
	const status = await page.evaluate(
		async ({ project, set }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const at = new Date(Date.now() + 4000).toISOString().slice(0, 19);
			const response = await fetch(`/api/v1/projects/${project}/environments/production/changesets/${set.id}/schedule`, {
				method: 'POST',
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, 'If-Match': `"${set.hash}"` },
				body: JSON.stringify({ local_time: at, time_zone: 'UTC' })
			});
			return response.status;
		},
		{ project, set }
	);
	expect(status).toBe(201);

	await page.goto(`/studio/projects/${project}/releases`);
	const row = page.getByTestId('schedules').getByRole('listitem').first();
	await expect(row).toContainText('Spring menu');
	await expect(row).toHaveAttribute('data-state', 'published', { timeout: 20_000 });
	await expect(row).toContainText('published · No. 1');
	await expect(page.getByTestId('releases').locator('[data-generation="1"]')).toBeVisible();
	expect(await violations(page)).toEqual([]);
});
