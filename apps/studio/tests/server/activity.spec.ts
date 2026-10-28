// Activity, the audit log in Settings (step R.4): each change to roles, members, invitations and
// API tokens as a sentence, newest first; filters for the kind of change and the person; "Show
// older" pages back. Only roles that may read it see it: an Editor sees neither Activity nor API
// tokens, in the sidebar, in search, or by address.

import { expect, test } from '@playwright/test';
import { api, internalTerms, member, newProject, shot, signIn, violations } from './helpers.ts';

test('the activity log tells each change as a sentence, newest first, and narrows by kind and person', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	const base = `/api/v1/projects/${project}`;
	const read = { action: 'content.read', subject: 'articles' };
	expect((await api(page, 'POST', `${base}/roles`, { key: 'desk', name: 'Desk', permissions: [read] })).status).toBe(201);
	const editor = await member(page, browser, project, 'editor');
	await editor.context().close();
	const people = (await api(page, 'GET', `${base}/members`)).body?.['data'] as { identity_id: string; role: string }[];
	const them = people.find((person) => person.role === 'editor')?.identity_id ?? '';
	expect((await api(page, 'PUT', `${base}/members/${them}`, { role: 'desk' })).status).toBe(204);
	const token = await api(page, 'POST', `${base}/tokens`, { name: 'Importer', kind: 'read-only', expires_in_days: 30 });
	expect(token.status).toBe(201);
	expect((await api(page, 'DELETE', `${base}/tokens/${token.body?.['data'].id}`)).status).toBe(200);
	const desk = (await api(page, 'GET', `${base}/roles/desk`)).body?.['data'] as { version: number };
	const changed = await api(page, 'PUT', `${base}/roles/desk`, { name: 'Desk', description: '', permissions: [read, { action: 'content.update', subject: 'articles' }] }, { 'If-Match': `"v${desk.version}"` });
	expect(changed.status).toBe(200);

	await page.goto(`/studio/projects/${project}/settings`);
	await page.getByTestId('activity-link').click();
	await expect(page.getByRole('heading', { level: 1, name: 'Activity' })).toBeVisible();
	const events = page.getByTestId('activity-event');
	await expect(events).toHaveCount(7);
	const said = await events.allInnerTexts();
	expect(said.map((line) => line.split('\n')[0]?.trim())).toEqual([
		'Dev Owner changed the role Desk: now may edit Articles.',
		'Dev Owner revoked the API token “Importer”.',
		expect.stringMatching(/^Dev Owner made the API token “Importer” \(read only, until .+\)\.$/),
		'Dev Owner changed Dev Editor’s role from Editor to Desk.',
		'Dev Editor joined as Editor, with an invitation.',
		'Dev Owner invited “dev-editor as editor” as Editor.',
		'Dev Owner made the role Desk.'
	]);
	await expect(page.getByTestId('activity').getByRole('heading', { name: 'Today' })).toBeVisible();
	await shot(page, 'activity');
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);

	// One kind of change; then one person; then everything again.
	await page.getByTestId('activity-kind').selectOption({ label: 'Someone’s role changed' });
	await expect(events).toHaveCount(1);
	await expect(events).toContainText('changed Dev Editor’s role from Editor to Desk');
	await page.getByTestId('activity-kind').selectOption({ label: 'Every kind' });
	await page.getByTestId('activity-person').selectOption({ label: 'Dev Editor' });
	await expect(events).toHaveCount(1);
	await expect(events).toContainText('Dev Editor joined as Editor');
	await page.getByRole('button', { name: 'Show everything' }).click();
	await expect(events).toHaveCount(7);

	// On a phone, without sideways scrolling.
	await page.setViewportSize({ width: 390, height: 844 });
	await page.waitForLoadState('networkidle');
	expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'Activity scrolls sideways on a phone').toBeLessThanOrEqual(0);
	expect(await violations(page)).toEqual([]);
});

test('a long log shows older changes on request', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	for (let index = 1; index <= 30; index += 1) {
		const made = await api(page, 'POST', `/api/v1/projects/${project}/invitations`, { role: 'editor', label: `Guest ${index}` });
		expect(made.status).toBe(201);
	}
	await page.goto(`/studio/projects/${project}/settings/activity`);
	const events = page.getByTestId('activity-event');
	await expect(events).toHaveCount(25);
	await expect(events.first()).toContainText('“Guest 30”');
	await page.getByTestId('activity-older').click();
	await expect(events).toHaveCount(30);
	await expect(events.last()).toContainText('“Guest 1”');
	await expect(page.getByTestId('activity-older')).toHaveCount(0);
});

test('an Editor sees neither Activity nor API tokens', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	const editor = await member(page, browser, project, 'editor');
	const asked: string[] = [];
	editor.on('request', (request) => {
		if (/^\/api\/v1\/projects\/[^/]+\/(tokens|audit)\b/.test(new URL(request.url()).pathname)) asked.push(request.url());
	});
	await expect(editor.getByTestId('first-run')).toBeVisible();
	await expect(editor.getByTestId('settings-link')).toHaveCount(0);
	await editor.keyboard.press('ControlOrMeta+k');
	await expect(editor.getByRole('option', { name: 'Members' })).toBeVisible();
	await expect(editor.getByRole('option', { name: 'API tokens' })).toHaveCount(0);
	await expect(editor.getByRole('option', { name: 'Activity' })).toHaveCount(0);
	await editor.keyboard.press('Escape');

	await editor.goto(`/studio/projects/${project}/settings/tokens`);
	await expect(editor.getByTestId('tokens-hidden')).toBeVisible();
	await expect(editor.getByRole('button', { name: 'Make the token' })).toHaveCount(0);
	expect(await internalTerms(editor)).toEqual([]);
	await editor.goto(`/studio/projects/${project}/settings/activity`);
	await expect(editor.getByTestId('activity-hidden')).toBeVisible();
	await expect(editor.getByTestId('activity-event')).toHaveCount(0);
	expect(await violations(editor)).toEqual([]);
	expect(await internalTerms(editor)).toEqual([]);
	// The Studio didn't even ask: the server would refuse, and the page has nothing to show.
	expect(asked).toEqual([]);
	await editor.context().close();
});
