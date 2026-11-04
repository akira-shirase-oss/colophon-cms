// Named review stages in the Studio (D-104, E2): the owner orders the sign-offs a publish
// request needs in Settings, the changeset names each stage and where it stands, a review
// names the stage it decides, the sender's own approval never counts at a someone-else
// stage, and a role that can't change the workflow reads the list without the controls.

import { expect, test } from '@playwright/test';
import { api, env, key, member, newEntry, newProject, signIn, violations } from './helpers.ts';

test('stages are ordered in Settings, and publishing waits on every one', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);

	// Settings → Review stages: two named sign-offs; Legal asks for someone other than the sender.
	await page.goto(`/studio/projects/${project}/settings`);
	await page.getByTestId('stages-link').click();
	await expect(page.getByTestId('stages-empty')).toBeVisible();
	await page.getByTestId('add-stage').click();
	await page.getByTestId('stage-name').first().fill('Editorial');
	await expect(page.getByTestId('stage-key').first()).toHaveValue('editorial');
	await page.getByTestId('add-stage').click();
	await page.getByTestId('stage-name').nth(1).fill('Legal');
	await page.getByTestId('stage-different').nth(1).check();
	expect(await violations(page)).toEqual([]);
	await page.getByTestId('save-stages').click();
	await expect(page.getByTestId('save-stages')).toBeDisabled();

	// The publish request names each stage and where it stands; the selector starts at the
	// first one still waiting.
	await newEntry(page, project, 'Spring launch');
	await page.goto(`/studio/projects/${project}/changesets`);
	await page.getByRole('button', { name: 'New publish request' }).click();
	await page.getByLabel('Title', { exact: true }).fill('Launch week');
	await page.getByLabel('Spring launch', { exact: true }).check();
	await page.getByRole('button', { name: 'Create publish request' }).click();
	await page.getByRole('button', { name: 'Ask for review' }).click();
	const stages = page.getByTestId('review-stages');
	await expect(stages.getByTestId('stage')).toHaveCount(2);
	await expect(stages.getByTestId('stage').nth(0)).toContainText('Editorial');
	await expect(stages.getByTestId('stage').nth(0)).toHaveAttribute('data-state', 'pending');
	await expect(stages.getByTestId('stage').nth(1)).toContainText('Legal');
	await expect(stages).toContainText('needs someone other than you');
	await expect(page.getByTestId('review-stage')).toHaveValue('editorial');

	// Approving names the stage; with Legal still waiting there's no Publish button, and a
	// publish through the API is refused outright.
	await page.getByRole('button', { name: 'Approve this version' }).click();
	await expect(stages.getByTestId('stage').nth(0)).toHaveAttribute('data-state', 'approved');
	await expect(stages).toContainText('by Dev Owner');
	await expect(page.getByTestId('review-stage')).toHaveValue('legal');
	await expect(page.getByRole('button', { name: 'Publish' })).toHaveCount(0);
	const id = new URL(page.url()).pathname.split('/').at(-1) ?? '';
	const shown = await api(page, 'GET', `${env(project)}/changesets/${id}`);
	const hash = String(shown.body?.['data'].hash);
	const generation = await page.evaluate(async (path) => (await fetch(path)).headers.get('colophon-generation'), `${env(project)}/releases`);
	const early = await api(page, 'POST', `${env(project)}/changesets/${id}/publish`, { base_generation: Number(generation ?? '0') }, { 'If-Match': `"${hash}"`, 'Idempotency-Key': key() });
	expect(early.status).toBe(409);
	expect(early.body?.['error'].details.reason).toBe('not_approved');

	// The sender's own approval at a someone-else stage is recorded — a review is never lost —
	// but it counts for nothing: Legal still waits.
	await page.getByRole('button', { name: 'Approve this version' }).click();
	await expect(page.getByTestId('changeset-reviews')).toContainText('for Legal');
	await expect(stages.getByTestId('stage').nth(1)).toHaveAttribute('data-state', 'pending');
	expect(await violations(page)).toEqual([]);

	// A reviewer who isn't the sender decides Legal; both stages now approve.
	const other = await member(page, browser, project, 'publisher', 'dev-editor');
	await other.goto(`/studio/projects/${project}/changesets/${id}`);
	await expect(other.getByTestId('review-stage')).toHaveValue('legal');
	await expect(other.getByTestId('review-stages')).toContainText('needs someone other than whoever sent it');
	await other.getByRole('button', { name: 'Approve this version' }).click();
	await expect(other.getByTestId('changeset-status')).toHaveAttribute('data-state', 'approved');
	await expect(other.getByTestId('review-stages').getByTestId('stage').nth(1)).toContainText('by Dev Editor');
	expect(await violations(other)).toEqual([]);

	// The owner publishes now that every stage approves.
	await page.reload();
	await expect(page.getByTestId('changeset-status')).toHaveAttribute('data-state', 'approved');
	await page.getByRole('button', { name: 'Publish' }).click();
	await expect(page.getByTestId('changeset-status')).toHaveAttribute('data-state', 'published');
});

test('a role that cannot change the workflow reads the stages without the controls', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);

	// One stage through the API; this test is about reading it, not changing it.
	const route = `${env(project)}/review-stages`;
	const etag = await page.evaluate(async (path) => (await fetch(path)).headers.get('etag'), route);
	const saved = await api(page, 'PUT', route, { stages: [{ key: 'legal', name: 'Legal', different_reviewer: true }] }, { 'If-Match': etag ?? '', 'Idempotency-Key': key() });
	expect(saved.status, JSON.stringify(saved.body)).toBe(200);

	// A publisher reviews and publishes but has no schema.change: no Settings link, a read-only
	// list on the page itself, and no editing controls.
	const other = await member(page, browser, project, 'publisher', 'dev-editor');
	await other.goto(`/studio/projects/${project}/settings`);
	await expect(other.getByTestId('stages-link')).toHaveCount(0);
	await other.goto(`/studio/projects/${project}/settings/stages`);
	await expect(other.getByTestId('stages-read-only')).toBeVisible();
	await expect(other.getByTestId('stages')).toContainText('Legal');
	await expect(other.getByTestId('stages')).toContainText('someone other than the sender');
	await expect(other.getByTestId('stage-name')).toHaveCount(0);
	await expect(other.getByTestId('save-stages')).toHaveCount(0);
	expect(await violations(other)).toEqual([]);
});
