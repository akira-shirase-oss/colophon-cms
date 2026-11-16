// A release receipt's slider and rollback in the Studio against a real server (D-097): the
// receipt walks the environment's releases through the slider, "Roll back to this release"
// builds a publish request that restores what that release published, and — sent for review,
// approved and published like any other — the restore goes out as a new generation. Restoring
// what is already live is refused with `already_at`.

import { type Page, expect, test } from '@playwright/test';
import { api, env, key, newEntry, newProject, publishThroughApi, signIn, violations } from './helpers.ts';

const status = (page: Page) => page.getByTestId('changeset-status');

test('a release receipt walks its neighbours and rolls back to itself as a new generation', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const first = await newEntry(page, project, 'Time travel');
	const second = await newEntry(page, project, 'Paradox');
	expect(await publishThroughApi(page, project, 'First light', [first])).toBe(1);
	expect(await publishThroughApi(page, project, 'Second wave', [second])).toBe(2);

	// Release 1's receipt: the slider sits between an Older button that cannot go lower and a
	// Newer one that walks forward.
	await page.goto(`/studio/projects/${project}/releases/1`);
	await expect(page.getByRole('heading', { level: 1 })).toContainText('Release 1');
	const slider = page.getByTestId('release-slider');
	await expect(slider).toBeVisible();
	await expect(slider).toContainText('Release 1 of 2');
	await expect(page.getByTestId('slider')).toHaveValue('1');
	await expect(page.getByRole('button', { name: 'Older release' })).toBeDisabled();
	expect(await violations(page)).toEqual([]);

	await slider.getByRole('link', { name: 'Newer release' }).click();
	await expect(page).toHaveURL(new RegExp(`/studio/projects/${project}/releases/2$`));
	await expect(slider).toContainText('Release 2 of 2');
	await expect(page.getByRole('heading', { level: 1 })).toContainText('Release 2');
	// The live release is not a rollback target: no button, and nothing newer to walk to.
	await expect(page.getByTestId('rollback')).toHaveCount(0);
	await expect(page.getByRole('button', { name: 'Newer release' })).toBeDisabled();

	// The range input walks too, one step each way.
	await page.getByTestId('slider').press('ArrowLeft');
	await expect(page).toHaveURL(new RegExp(`/studio/projects/${project}/releases/1$`));
	await page.getByTestId('slider').press('ArrowRight');
	await expect(page).toHaveURL(new RegExp(`/studio/projects/${project}/releases/2$`));
	await slider.getByRole('link', { name: 'Older release' }).click();
	await expect(page).toHaveURL(new RegExp(`/studio/projects/${project}/releases/1$`));

	// Rolling back to release 1 builds a publish request: it takes down what release 2 added.
	await page.getByTestId('rollback').click();
	await expect(page).toHaveURL(new RegExp(`/studio/projects/${project}/changesets/[0-9a-f-]+$`));
	await expect(page.getByRole('heading', { level: 1 })).toContainText('Rollback to release 1');
	await expect(status(page)).toHaveAttribute('data-state', 'open');
	const items = page.getByTestId('changeset-items').getByRole('listitem');
	await expect(items).toHaveCount(1);
	await expect(items).toContainText('Paradox');
	await expect(page.getByTestId('unpublish')).toBeVisible();
	expect(await violations(page)).toEqual([]);
	const changeset = new URL(page.url()).pathname.split('/').pop() ?? '';

	// The restore is a changeset like any other: sent for review, approved, published on the
	// release it follows. The generation moves forward, never back.
	const got = await api(page, 'GET', `${env(project)}/changesets/${changeset}`);
	expect(got.status, JSON.stringify(got.body)).toBe(200);
	const ifMatch = { 'If-Match': `"${String(got.body?.['data'].hash)}"` };
	expect((await api(page, 'POST', `${env(project)}/changesets/${changeset}/submit`, undefined, ifMatch)).status).toBe(200);
	expect((await api(page, 'POST', `${env(project)}/changesets/${changeset}/reviews`, { decision: 'approve' }, ifMatch)).status).toBe(200);
	const published = await api(
		page,
		'POST',
		`${env(project)}/changesets/${changeset}/publish`,
		{ base_generation: 2 },
		{ ...ifMatch, 'Idempotency-Key': key() }
	);
	expect(published.status, JSON.stringify(published.body)).toBe(201);
	expect(published.body?.['data'].generation).toBe(3);

	// Release 3's receipt spans all three, and readers get exactly what release 1 gave them.
	await page.goto(`/studio/projects/${project}/releases/3`);
	await expect(page.getByRole('heading', { level: 1 })).toContainText('Release 3');
	await expect(page.getByTestId('release-slider')).toContainText('Release 3 of 3');
	const delivered = await page.evaluate(async (path) => {
		const response = await fetch(path, { credentials: 'omit' });
		return {
			generation: response.headers.get('colophon-generation'),
			titles: ((await response.json()) as { data: { title: string }[] }).data.map((entry) => entry.title)
		};
	}, `/content/v1/${project}/production/articles`);
	expect(delivered).toEqual({ generation: '3', titles: ['Time travel'] });

	// Release 1's set is live again: restoring it once more builds nothing.
	const again = await api(
		page,
		'POST',
		`${env(project)}/releases/1/rollback`,
		{ title: 'Back to one, again' },
		{ 'Idempotency-Key': key() }
	);
	expect(again.status, JSON.stringify(again.body)).toBe(422);
	expect(again.body?.['error']?.['details']?.['reason']).toBe('already_at');
});
