// Controls follow permissions (D-059, rule 5; the server still decides every request): a role
// that can't edit doesn't get "Ask for review" or "Update to the latest versions"; someone who
// neither set a schedule nor may publish it doesn't get "Cancel the schedule"; and a role with
// nothing to change in Settings (an Editor) doesn't see Settings, in the sidebar or in search.

import { expect, type Page, test } from '@playwright/test';
import { api, env, internalTerms, key, member, newEntry, newProject, signIn, violations } from './helpers.ts';

/** A publish request of `entries`, through the API; submitted unless `open`. Its id and hash. */
async function request(page: Page, project: string, title: string, entries: string[], open = false): Promise<{ id: string; hash: string }> {
	const made = await api(page, 'POST', `${env(project)}/changesets`, { title, entries }, { 'Idempotency-Key': key() });
	const set = made.body?.['data'] as { id: string; hash: string };
	if (!open) expect((await api(page, 'POST', `${env(project)}/changesets/${set.id}/submit`, undefined, { 'If-Match': `"${set.hash}"` })).status).toBe(200);
	return set;
}

/** Saves a new title on an entry, as someone editing it elsewhere. */
async function retitle(page: Page, project: string, entry: string, title: string) {
	const status = await page.evaluate(
		async ({ path, title }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const current = await fetch(path);
			const { data } = (await current.json()) as { data: { content: Record<string, unknown> } };
			const response = await fetch(path, {
				method: 'PUT',
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, 'If-Match': current.headers.get('etag') ?? '', 'Idempotency-Key': crypto.randomUUID().replaceAll('-', '') },
				body: JSON.stringify({ ...data.content, title })
			});
			return response.status;
		},
		{ path: `${env(project)}/collections/articles/entries/${entry}`, title }
	);
	expect(status).toBe(200);
}

test('controls follow the role: a reader can’t send or update, an editor can’t cancel a schedule or open Settings', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	const base = `/studio/projects/${project}`;

	// One request not yet sent, one edited since it was sent (so it needs updating), one approved
	// and scheduled by the owner for tomorrow.
	const open = await request(page, project, 'Not sent yet', [await newEntry(page, project, 'First draft')], true);
	const edited = await newEntry(page, project, 'Edited after sending');
	const stale = await request(page, project, 'Edited since', [edited]);
	await retitle(page, project, edited, 'Edited after sending, again');
	const later = await request(page, project, 'Tomorrow morning', [await newEntry(page, project, 'Scheduled piece')]);
	expect((await api(page, 'POST', `${env(project)}/changesets/${later.id}/reviews`, { decision: 'approve' }, { 'If-Match': `"${later.hash}"` })).status).toBe(200);
	const tomorrow = new Date(Date.now() + 24 * 3600 * 1000).toISOString().slice(0, 10);
	const scheduled = await api(page, 'POST', `${env(project)}/changesets/${later.id}/schedule`, { local_time: `${tomorrow}T09:00`, time_zone: 'UTC' }, { 'If-Match': `"${later.hash}"` });
	expect(scheduled.status, JSON.stringify(scheduled.body)).toBe(201);

	// The owner has every control.
	await page.goto(`${base}/changesets/${later.id}`);
	await expect(page.getByTestId('schedule-pending').getByRole('button', { name: 'Cancel the schedule' })).toBeVisible();
	await expect(page.getByTestId('settings-link')).toBeVisible();

	// A role that may only read.
	expect((await api(page, 'POST', `/api/v1/projects/${project}/roles`, { key: 'reader', name: 'Reader', permissions: [{ action: 'content.read', subject: '*' }] })).status).toBe(201);
	const other = await member(page, browser, project, 'reader');
	await other.goto(`${base}/changesets/${open.id}`);
	await expect(other.getByTestId('changeset-status')).toHaveAttribute('data-state', 'open');
	await expect(other.getByTestId('cannot-send')).toContainText('Someone who can edit these entries sends it for review.');
	await expect(other.getByRole('button', { name: 'Ask for review' })).toHaveCount(0);
	await other.goto(`${base}/changesets/${stale.id}`);
	await expect(other.getByTestId('changeset-status')).toHaveAttribute('data-state', 'stale');
	await expect(other.getByRole('button', { name: 'Update to the latest versions' })).toHaveCount(0);
	await expect(other.getByTestId('settings-link')).toHaveCount(0);
	expect(await violations(other)).toEqual([]);
	expect(await internalTerms(other)).toEqual([]);

	// The same person as an Editor: sends and updates, but neither cancels the owner's schedule nor
	// sees Settings.
	const people = await api(page, 'GET', `/api/v1/projects/${project}/members`);
	const them = (people.body?.['data'] as { identity_id: string; role: string }[]).find((person) => person.role === 'reader');
	expect((await api(page, 'PUT', `/api/v1/projects/${project}/members/${them?.identity_id}`, { role: 'editor' })).status).toBe(204);
	await other.goto(`${base}/changesets/${open.id}`);
	await expect(other.getByRole('button', { name: 'Ask for review' })).toBeVisible();
	await other.goto(`${base}/changesets/${later.id}`);
	await expect(other.getByTestId('schedule-pending')).toContainText('Publishes');
	await expect(other.getByRole('button', { name: 'Cancel the schedule' })).toHaveCount(0);
	await expect(other.getByTestId('cannot-cancel')).toBeVisible();
	await expect(other.getByTestId('settings-link')).toHaveCount(0);
	await expect(other.getByTestId('members-link')).toBeVisible();
	await other.keyboard.press('ControlOrMeta+k');
	await expect(other.getByRole('option', { name: 'Members' })).toBeVisible();
	await expect(other.getByRole('option', { name: 'Settings' })).toHaveCount(0);
	await other.keyboard.press('Escape');
	expect(await violations(other)).toEqual([]);
	expect(await internalTerms(other)).toEqual([]);
	await other.context().close();
});
