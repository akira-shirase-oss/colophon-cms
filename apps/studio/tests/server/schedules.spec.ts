// Scheduled publishing in the Studio against a real server (D-060): on an approved changeset, an
// owner picks a date, a time and a time zone; a time the clocks show twice brings up both moments
// to choose from; the schedule shows, can be cancelled, and says so. A schedule a few seconds out
// is published by the server's own worker and the Releases page shows it happen.

import { expect, type Page, test } from '@playwright/test';
import { newProject, nextAmbiguous, signIn, violations } from './helpers.ts';

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

	// A local time the clocks show twice brings up both moments to choose from. The window is the
	// approval's week and no clock change may fall inside it; when none does that part is skipped,
	// and a plain pending schedule takes the cancel.
	const found = nextAmbiguous(Date.now(), 6);
	if (found) {
		await panel.getByLabel('Date').fill(found.date);
		await panel.getByLabel('Time', { exact: true }).fill(found.time);
		await panel.getByLabel('Time zone').selectOption(found.zone);
		await panel.getByRole('button', { name: 'Schedule', exact: true }).click();
		const choice = page.getByTestId('schedule-choice');
		await expect(choice).toContainText('happens twice');
		await expect(choice.getByRole('radio')).toHaveCount(2);
		await expect(choice).toContainText(found.abbreviations[0]);
		await expect(choice).toContainText(found.abbreviations[1]);
		expect(await violations(page)).toEqual([]);
		await choice.getByRole('radio').nth(1).check();
		await panel.getByRole('button', { name: 'Schedule at this moment' }).click();
	} else {
		await panel.getByLabel('Date').fill(new Date(Date.now() + 5 * 24 * 60 * 60 * 1000).toISOString().slice(0, 10));
		await panel.getByLabel('Time', { exact: true }).fill('12:00');
		await panel.getByLabel('Time zone').selectOption('UTC');
		await panel.getByRole('button', { name: 'Schedule', exact: true }).click();
	}
	const pending = page.getByTestId('schedule-pending');
	if (found) await expect(pending).toContainText(found.abbreviations[1]);
	await expect(pending).toContainText(found ? found.zone : 'UTC');

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
