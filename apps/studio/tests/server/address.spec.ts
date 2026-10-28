// An entry's public address (gap audit, 2026-10-28): once published, the entry's page says where
// readers get it, the delivery read anyone can make without signing in, with a way to open and
// copy it, in Technical details (D-081); while there are newer changes it says readers get the
// version live in a release, not them; a draft that was never published shows no address. A
// visible Copy public link copies it without opening the drawer (D-082).

import { expect, test } from '@playwright/test';
import { internalTerms, newEntry, newProject, publishThroughApi, signIn, violations } from './helpers.ts';

test('a published entry says where readers get it, and the address answers without signing in', async ({ page, context, request, baseURL }) => {
	await context.grantPermissions(['clipboard-read', 'clipboard-write']);
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Out in the world');
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	// Never published: no address, and nothing to copy.
	await expect(page.getByTestId('public-address')).toHaveCount(0);
	await expect(page.getByTestId('copy-public-link')).toHaveCount(0);

	const generation = await publishThroughApi(page, project, 'Out it goes', [entry]);
	await page.reload();
	const address = `${baseURL}/content/v1/${project}/production/articles/${entry}`;
	const shown = page.getByTestId('public-address');
	// Copying the public link is one visible click, above Technical details (D-082).
	const copyLink = page.getByTestId('copy-public-link');
	await expect(copyLink).toBeVisible();
	await expect(page.getByTestId('entry-details')).toContainText('Anyone can read it.');
	const linkBox = await copyLink.boundingBox();
	const drawerBox = await page.getByTestId('entry-details').getByTestId('technical-details').boundingBox();
	expect(linkBox?.y ?? Infinity).toBeLessThan(drawerBox?.y ?? 0);
	await copyLink.click();
	await expect(page.getByText('Copied')).toBeVisible();
	expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(`${baseURL}/content/v1/${project}/production/articles/${entry}`);
	// The address itself is for developers, so it waits in Technical details (D-081).
	await expect(shown).toBeHidden();
	await page.getByTestId('entry-details').getByTestId('technical-details').locator('summary').click();
	await expect(shown.getByRole('link')).toHaveAttribute('href', address);
	// Even the drawer says how readers get an entry without the internal word (D-082).
	await expect(page.getByTestId('entry-details').getByTestId('technical-details')).toContainText('Readers get an entry only once a release with it is published.');
	await expect(page.getByTestId('entry-details').getByTestId('technical-details')).not.toContainText(/changeset/i);
	await expect(shown).toContainText('Anyone can read the published version at this address, without signing in.');
	expect(await internalTerms(page)).toEqual([]);
	expect(await violations(page)).toEqual([]);

	// It answers anyone: a request with no session.
	const read = await request.get(address);
	expect(read.status()).toBe(200);
	expect(JSON.stringify(await read.json())).toContain('Out in the world');

	await shown.getByRole('button', { name: 'Copy the public address' }).click();
	await expect(page.getByText('Copied').last()).toBeVisible();
	expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(address);

	// Newer changes: readers still get the released version, and it says so.
	await page.getByLabel('Title', { exact: true }).fill('Out in the world, revised');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await expect(shown).toContainText(`the one live in Release ${generation}, not your newer changes.`);
	await expect(page.getByTestId('entry-details')).toContainText('Anyone can read the published version.');

	// On a phone, the address fits without scrolling sideways.
	await page.setViewportSize({ width: 390, height: 844 });
	await page.waitForLoadState('networkidle');
	await expect(shown.getByRole('link')).toBeVisible();
	expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'the editor scrolls sideways on a phone').toBeLessThanOrEqual(0);
	expect(await violations(page)).toEqual([]);
});
