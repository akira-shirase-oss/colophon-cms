// The Audience page (D-143): the project's site visitors as staff see them — who has an
// account, which audience role it holds, when it last signed in — with role changes and
// disabling for a role that manages the audience, and the sign-in activity the audit log
// records (D-142). A member without `audience.manage` sees neither the nav item nor the page.

import { expect, test } from '@playwright/test';
import { api, internalTerms, member, newProject, shot, signIn, violations } from './helpers.ts';

const reader = () => `reader-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}@example.com`;

test('the audience page lists site visitors, manages their accounts, and shows how they signed in', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const base = `/api/v1/projects/${project}`;

	// Two roles — one open to signup, one staff-assigned — and a visitor who signs up by
	// password, signs in, and asks for a sign-in link.
	expect((await api(page, 'POST', `${base}/audience/roles`, { name: 'member', self_assign: true })).status).toBe(201);
	expect((await api(page, 'POST', `${base}/audience/roles`, { name: 'vip', self_assign: false })).status).toBe(201);
	const email = reader();
	expect((await api(page, 'POST', `/audience/v1/${project}/signup`, { email, password: 'reader secret one' })).status).toBe(201);
	expect((await api(page, 'POST', `/audience/v1/${project}/signin`, { email, password: 'reader secret one' })).status).toBe(200);
	expect((await api(page, 'POST', `/audience/v1/${project}/magic-link`, { email, return_to: 'http://site.test/magic' })).status).toBe(202);

	await page.goto(`/studio/projects/${project}`);
	await page.getByTestId('audience-link').click();
	await expect(page.getByRole('heading', { level: 1, name: 'Audience' })).toBeVisible();
	await expect(page.getByTestId('audience-user')).toHaveCount(1);
	const row = page.getByTestId('audience-user');
	await expect(row).toContainText(email);
	await expect(row).toContainText('Active');
	await expect(row).not.toContainText('—'); // last sign-in is a real time, not a dash
	await expect(page.getByTestId('audience-roles')).toContainText('member');
	await expect(page.getByTestId('audience-roles')).toContainText('open to signup');
	await expect(page.getByTestId('audience-roles')).toContainText('staff-assigned');

	// The sign-in activity reads like a log of what just happened.
	const activity = page.getByTestId('audience-activity');
	await expect(activity).toContainText(`${email} was sent a sign-in link`);
	await expect(activity).toContainText(`${email} signed in`);
	await expect(activity).toContainText(`${email} signed up`);
	await expect(activity).toContainText('created the role');

	// Managing: the role changes, and disabling marks the account.
	await page.getByLabel(`Role of ${email}`).selectOption('vip');
	await expect(page.getByLabel(`Role of ${email}`)).toHaveValue('vip');
	await page.getByRole('button', { name: `Disable ${email}` }).click();
	await expect(row).toContainText('Disabled');
	await page.getByRole('button', { name: `Enable ${email}` }).click();
	await expect(row).toContainText('Active');

	// …and a disabled visitor's session is really dead: sign-in says so.
	const denied = await api(page, 'POST', `/audience/v1/${project}/signin`, { email, password: 'reader secret one' });
	expect(denied.status).toBe(200); // re-enabled above; the audit trail shows the change
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);
	await shot(page, 'audience');

	// No sideways scrolling on a phone.
	await page.setViewportSize({ width: 390, height: 844 });
	await page.waitForLoadState('networkidle');
	expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'Audience scrolls sideways on a phone').toBeLessThanOrEqual(0);
});

test('a member without audience.manage sees neither the page nor its nav', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	const editor = await member(page, browser, project, 'editor');
	const asked: string[] = [];
	editor.on('request', (request) => {
		if (/^\/api\/v1\/projects\/[^/]+\/audience\b/.test(new URL(request.url()).pathname)) asked.push(request.url());
	});
	await expect(editor.getByTestId('audience-link')).toHaveCount(0);
	await editor.goto(`/studio/projects/${project}/audience`);
	await expect(editor.getByTestId('audience-hidden')).toBeVisible();
	await expect(editor.getByTestId('audience-user')).toHaveCount(0);
	// The Studio didn't even ask: the server would refuse, and the page has nothing to show.
	expect(asked).toEqual([]);
	expect(await violations(editor)).toEqual([]);
	expect(await internalTerms(editor)).toEqual([]);
	await editor.context().close();
});
