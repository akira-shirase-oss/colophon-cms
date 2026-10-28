// Settings laid out like the content-type builder (D-081): a left panel with one page each for
// General, Roles, Website connections, API tokens and Activity; making something (a role, a
// connection, a token) is a button that opens a dialog, never a form left open; the project id and
// public API address wait in Technical details; old addresses still open the right page; each page
// shows in the panel only to roles that can use it.

import { expect, test } from '@playwright/test';
import { api, internalTerms, member, newProject, shot, signIn, violations } from './helpers.ts';

test('an owner finds each setting on a page of its own, from a left panel, with nothing open until asked', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const base = `/studio/projects/${project}`;
	await page.goto(`${base}/settings`);

	// The panel: every page, General open.
	const nav = page.getByTestId('settings-nav');
	await expect(nav.getByRole('link')).toHaveText(['General', 'Roles', 'Website connections', 'API tokens', 'Activity']);
	await expect(nav.getByTestId('settings-general-link')).toHaveAttribute('aria-current', 'page');
	await expect(page.getByRole('heading', { level: 1, name: 'General' })).toBeVisible();
	await expect(page.getByRole('navigation', { name: 'breadcrumb' })).toContainText('Settings');

	// General: plain things first; the developer details are closed until asked for.
	await expect(page.getByText('Studio test', { exact: true }).last()).toBeVisible();
	await expect(page.getByTestId('project-id')).toBeHidden();
	await expect(page.getByTestId('public-api')).toBeHidden();
	await page.getByTestId('technical-details').getByText('Technical details').click();
	await expect(page.getByTestId('project-id')).toHaveText(project);
	await expect(page.getByTestId('public-api')).toContainText(`/content/v1/${project}/production/articles`);
	// The count reads as words with a space each side of the dot (it once said "1 content type· 0").
	await expect(page.getByTestId('types-count')).toHaveText('1 content type · 0 components');
	// Nothing to fill in on General: making things is on their own pages.
	await expect(page.locator('main input, main select, main textarea')).toHaveCount(0);
	expect(await violations(page)).toEqual([]);
	await shot(page, 'settings-general');

	// Roles: the list, and New role opens a dialog; Cancel makes nothing.
	await nav.getByTestId('roles-link').click();
	await expect(page).toHaveURL(new RegExp(`/settings/roles$`));
	await expect(nav.getByTestId('roles-link')).toHaveAttribute('aria-current', 'page');
	await expect(page.getByTestId('roles')).toContainText('Owner');
	await expect(page.locator('#role-name')).toHaveCount(0);
	await page.getByRole('button', { name: 'New role' }).click();
	const roleDialog = page.getByRole('dialog', { name: 'New role' });
	await expect(roleDialog.locator('#role-name')).toBeFocused();
	expect(await violations(page)).toEqual([]);
	await roleDialog.getByRole('button', { name: 'Cancel' }).click();
	await expect(roleDialog).toBeHidden();
	await expect(page.getByTestId('roles').getByRole('listitem')).toHaveCount(4);
	await expect(page.getByRole('navigation', { name: 'breadcrumb' })).toContainText('Roles');

	// Website connections: empty, with one way to add one (it used to offer two), in a dialog.
	await nav.getByTestId('connections-link').click();
	await expect(page).toHaveURL(new RegExp(`/settings/connections$`));
	await expect(page.getByTestId('connections-empty')).toBeVisible();
	await expect(page.getByRole('button', { name: /add .*connection/i })).toHaveCount(1);
	await expect(page.getByLabel('Connection name')).toHaveCount(0);
	await page.getByRole('button', { name: 'Add a connection' }).click();
	const connection = page.getByRole('dialog', { name: 'Add a website connection' });
	await expect(connection.getByRole('radio', { name: /Site rebuild/ })).toBeChecked();
	await connection.getByRole('radio', { name: /Site check/ }).check();
	await expect(connection.getByLabel('URL to check (https)')).toBeVisible();
	await expect(connection.getByLabel(/Location/)).toBeVisible();
	await connection.getByRole('radio', { name: /Site rebuild/ }).check();
	await connection.getByLabel('Connection name').fill('Site build');
	await connection.getByLabel('Webhook URL (https)').fill('https://example.com/hooks/colophon');
	expect(await violations(page)).toEqual([]);
	await connection.getByRole('button', { name: 'Add website connection' }).click();
	await expect(connection).toBeHidden();
	await expect(page.getByTestId('destinations')).toContainText('Site build');
	await expect(page.getByTestId('destinations')).toContainText('Announce releases');
	// Now the list, and still one way to add another, which starts afresh.
	await expect(page.getByTestId('connections-empty')).toHaveCount(0);
	await expect(page.getByRole('button', { name: /add .*connection/i })).toHaveCount(1);
	await page.getByRole('button', { name: 'Add a connection' }).click();
	await expect(connection.getByLabel('Connection name')).toHaveValue('');
	await page.keyboard.press('Escape');
	await expect(connection).toBeHidden();
	await expect(page.getByTestId('destinations').getByRole('listitem')).toHaveCount(1);
	await shot(page, 'settings-connections');

	// API tokens and Activity are pages of the same panel.
	await nav.getByTestId('tokens-link').click();
	await expect(page.getByRole('heading', { level: 1, name: 'API tokens' })).toBeVisible();
	await expect(page.getByLabel('Name', { exact: true })).toHaveCount(0);
	await page.getByTestId('tokens-empty').getByRole('button', { name: 'New token' }).click();
	await expect(page.getByRole('dialog', { name: 'New token' }).getByLabel('Name', { exact: true })).toBeFocused();
	await page.keyboard.press('Escape');
	await nav.getByTestId('activity-link').click();
	await expect(page.getByRole('heading', { level: 1, name: 'Activity' })).toBeVisible();
	await expect(nav.getByTestId('activity-link')).toHaveAttribute('aria-current', 'page');

	// On a phone the panel is a row of links above the page, with no sideways scrolling.
	await page.setViewportSize({ width: 390, height: 844 });
	for (const path of ['settings', 'settings/roles', 'settings/connections', 'settings/tokens', 'settings/activity']) {
		await page.goto(`${base}/${path}`);
		await expect(nav.getByTestId('settings-general-link')).toBeInViewport();
		await page.waitForLoadState('networkidle');
		expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), `${path} scrolls sideways on a phone`).toBeLessThanOrEqual(0);
	}
	expect(await violations(page)).toEqual([]);
});

test('old addresses still open the right page, and a role is named in the trail by its name', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const base = `/studio/projects/${project}`;

	// Website connections were a section of the one Settings page; links to it still work.
	await page.goto(`${base}/settings#website-connections`);
	await expect(page).toHaveURL(new RegExp(`/settings/connections$`));
	await expect(page.getByRole('heading', { level: 1, name: 'Website connections' })).toBeVisible();

	// The first-run checklist's "Connect" opens the page.
	await page.goto(base);
	await page.getByTestId('first-run').getByRole('link', { name: /^Connect:/ }).click();
	await expect(page).toHaveURL(new RegExp(`/settings/connections$`));

	// A role's page sits under Roles, named in words, not by its short name.
	await page.goto(`${base}/settings/roles/publisher`);
	const trail = page.getByRole('navigation', { name: 'breadcrumb' });
	await expect(trail).toContainText('Settings');
	await expect(trail.getByRole('link', { name: 'Roles' })).toHaveAttribute('href', `${base}/settings/roles`);
	await expect(trail).toContainText('Publisher');
	await expect(trail).not.toContainText('Role publisher');
	await expect(page.getByTestId('roles-link')).toHaveAttribute('aria-current', 'page');

	// Search finds the new pages too.
	await page.keyboard.press('ControlOrMeta+k');
	await expect(page.getByRole('option', { name: 'Roles' })).toBeVisible();
	await expect(page.getByRole('option', { name: 'Website connections' })).toBeVisible();
	await page.getByRole('option', { name: 'Website connections' }).click();
	await expect(page).toHaveURL(new RegExp(`/settings/connections$`));
});

test('a Developer sees only the pages their role can use, and an Editor still has no Settings', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	const base = `/studio/projects/${project}`;

	const developer = await member(page, browser, project, 'developer');
	await developer.getByTestId('settings-link').click();
	const nav = developer.getByTestId('settings-nav');
	await expect(nav.getByRole('link')).toHaveText(['General', 'Roles', 'Website connections']);
	await developer.getByTestId('roles-link').click();
	await expect(developer.getByTestId('roles')).toContainText('Developer');
	await expect(developer.getByRole('button', { name: 'New role' })).toHaveCount(0);
	await developer.getByTestId('connections-link').click();
	await expect(developer.getByRole('button', { name: 'Add a connection' })).toBeVisible();
	// By address, a page their role can't use says why, and keeps its place in the panel.
	await developer.goto(`${base}/settings/tokens`);
	await expect(developer.getByTestId('tokens-hidden')).toBeVisible();
	await expect(nav.getByTestId('tokens-link')).toHaveAttribute('aria-current', 'page');
	expect(await violations(developer)).toEqual([]);

	// The same person as an Editor: nothing to change in Settings, so no Settings.
	const people = await api(page, 'GET', `/api/v1/projects/${project}/members`);
	const them = (people.body?.['data'] as { identity_id: string; role: string }[]).find((person) => person.role === 'developer');
	expect((await api(page, 'PUT', `/api/v1/projects/${project}/members/${them?.identity_id}`, { role: 'editor' })).status).toBe(204);
	const editor = developer;
	await editor.goto(base);
	await expect(editor.getByTestId('first-run')).toBeVisible();
	await expect(editor.getByTestId('settings-link')).toHaveCount(0);
	await editor.goto(`${base}/settings/connections`);
	await expect(editor.getByTestId('destinations-hidden')).toBeVisible();
	await expect(editor.getByRole('button', { name: 'Add a connection' })).toHaveCount(0);
	await expect(editor.getByTestId('settings-nav').getByRole('link')).toHaveText(['General', 'Roles', 'Website connections']);
	expect(await internalTerms(editor)).toEqual([]);
	await editor.context().close();
});
