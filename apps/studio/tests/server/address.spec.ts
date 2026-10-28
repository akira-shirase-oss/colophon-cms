// An entry's public address (gap audit, 2026-10-28): once published, the entry's page says where
// readers get it, the delivery read anyone can make without signing in, with a way to open and
// copy it; while there are newer changes it says readers get the version live in a release, not
// them; a draft that was never published shows no address.

import { expect, test } from '@playwright/test';
import { internalTerms, newEntry, newProject, publishThroughApi, signIn, violations } from './helpers.ts';

test('a published entry says where readers get it, and the address answers without signing in', async ({ page, context, request, baseURL }) => {
	await context.grantPermissions(['clipboard-read', 'clipboard-write']);
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Out in the world');
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	// Never published: no address.
	await expect(page.getByTestId('public-address')).toHaveCount(0);

	const generation = await publishThroughApi(page, project, 'Out it goes', [entry]);
	await page.reload();
	const address = `${baseURL}/content/v1/${project}/production/articles/${entry}`;
	const shown = page.getByTestId('public-address');
	await expect(shown.getByRole('link')).toHaveAttribute('href', address);
	await expect(shown).toContainText('Anyone can read the published version at this address, without signing in.');
	expect(await internalTerms(page)).toEqual([]);
	expect(await violations(page)).toEqual([]);

	// It answers anyone: a request with no session.
	const read = await request.get(address);
	expect(read.status()).toBe(200);
	expect(JSON.stringify(await read.json())).toContain('Out in the world');

	await shown.getByRole('button', { name: 'Copy the public address' }).click();
	await expect(page.getByText('Copied')).toBeVisible();
	expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(address);

	// Newer changes: readers still get the released version, and it says so.
	await page.getByLabel('Title', { exact: true }).fill('Out in the world, revised');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await expect(shown).toContainText(`the one live in Release ${generation}, not your newer changes.`);

	// On a phone, the address fits without scrolling sideways.
	await page.setViewportSize({ width: 390, height: 844 });
	await page.waitForLoadState('networkidle');
	expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'the editor scrolls sideways on a phone').toBeLessThanOrEqual(0);
	expect(await violations(page)).toEqual([]);
});
