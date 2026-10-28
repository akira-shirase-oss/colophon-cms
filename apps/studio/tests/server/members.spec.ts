// Members and invitations in the Studio against a real server (D-042): an owner invites with a
// link shown once, the invitee signs in and joins, roles change what each sees, the last owner
// can't step down, and a member can leave. Inviting reaches Roles (D-082): Manage roles, and Make a
// new role…, which comes back to the invitation with the new role chosen.

import { expect, test } from '@playwright/test';
import { keycloak, newProject, signIn, violations } from './helpers.ts';

test('an owner invites with a link, the invitee signs in and joins, and roles decide who manages members', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}`);
	await page.getByTestId('members-link').click();
	await expect(page.getByTestId('member')).toHaveCount(1);
	await expect(page.getByLabel('Role of Dev Owner')).toHaveValue('owner');

	// The link is shown once; the list of pending invitations never shows it again.
	await page.getByLabel('Who is it for?').fill('A guest editor');
	await page.getByLabel('Role', { exact: true }).selectOption('editor');
	await page.getByRole('button', { name: 'Create invitation link' }).click();
	const link = await page.getByRole('textbox', { name: 'Invitation link' }).inputValue();
	expect(new URL(link).pathname).toBe('/studio/invite');
	expect(new URL(link).hash).toMatch(/^#[A-Za-z0-9_-]{43}$/);
	await expect(page.getByTestId('invitations')).toContainText('A guest editor');
	expect(await violations(page)).toEqual([]);
	await page.reload();
	await expect(page.getByTestId('invitation-link')).toHaveCount(0);
	await expect(page.getByTestId('invitations')).toContainText('A guest editor');

	// The guest opens the link signed out, signs in, and is back at the invitation, with the
	// token out of the address bar.
	const guest = await (await browser.newContext()).newPage();
	await guest.goto(new URL(link).pathname + new URL(link).hash);
	await expect(guest.getByTestId('signed-out')).toBeVisible();
	expect(new URL(guest.url()).hash).toBe('');
	await guest.getByRole('link', { name: 'Sign in' }).click();
	await keycloak(guest, 'dev-editor');
	await expect(guest.getByTestId('invite-ask')).toContainText('Dev Owner invited you to join Studio test as Editor: Writes and edits content.');
	expect(new URL(guest.url()).hash).toBe('');
	expect(await violations(guest)).toEqual([]);
	await guest.getByRole('button', { name: 'Join', exact: true }).click();
	await expect(guest.getByTestId('invite-outcome')).toContainText('You joined Studio test as Editor.');

	// As an editor: the members are visible, but there's nothing to manage.
	await guest.getByRole('link', { name: 'Studio test' }).click();
	await guest.getByTestId('members-link').click();
	await expect(guest.getByTestId('member')).toHaveCount(2);
	await expect(guest.getByRole('combobox')).toHaveCount(0);
	await expect(guest.getByRole('button', { name: 'Create invitation link' })).toHaveCount(0);
	expect(await violations(guest)).toEqual([]);

	// The owner sees them join, and can't step down while they're the only owner.
	await page.reload();
	await expect(page.getByTestId('member')).toHaveCount(2);
	await expect(page.getByTestId('invitations')).toContainText('None.');
	await page.getByLabel('Role of Dev Owner').selectOption('editor');
	await expect(page.getByTestId('members-error')).toContainText('at least one owner');
	await expect(page.getByLabel('Role of Dev Owner')).toHaveValue('owner');
	await page.getByLabel('Role of Dev Editor').selectOption('developer');
	await expect(page.getByLabel('Role of Dev Editor')).toHaveValue('developer');
	await expect(page.getByTestId('members-error')).toHaveCount(0);

	// A revoked invitation stops working at once.
	await page.getByLabel('Who is it for?').fill('Changed my mind');
	await page.getByRole('button', { name: 'Create invitation link' }).click();
	const revokedLink = await page.getByRole('textbox', { name: 'Invitation link' }).inputValue();
	await page.getByRole('button', { name: 'Revoke the invitation for Changed my mind' }).click();
	await expect(page.getByTestId('invitations')).toContainText('None.');
	await guest.goto(new URL(revokedLink).pathname + new URL(revokedLink).hash);
	await expect(guest.getByTestId('invite-outcome')).toContainText('expired, was revoked, or was already used');

	// The guest leaves, and the project is gone from their list.
	await guest.goto(`/studio/projects/${project}/members`);
	await guest.getByRole('button', { name: 'Leave project' }).click();
	await expect(guest.getByTestId('signed-in')).toBeVisible();
	await expect(guest.locator(`a[href="/studio/projects/${project}"]`)).toHaveCount(0);
	await page.reload();
	await expect(page.getByTestId('member')).toHaveCount(1);
});

test('Make a new role… from the invitation makes one in Roles and comes back with it chosen', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/members`);

	// Manage roles sits beside the Role select.
	await expect(page.getByTestId('manage-roles')).toHaveAttribute('href', `/studio/projects/${project}/settings/roles`);
	const select = page.getByLabel('Role', { exact: true });
	await expect(select.locator('option').last()).toHaveText('Make a new role…');
	expect(await violations(page)).toEqual([]);

	await page.getByLabel('Who is it for?').fill('Sam, copy editor');
	await select.selectOption({ label: 'Make a new role…' });
	await expect(page).toHaveURL(new RegExp(`/settings/roles\\?new=1&for=invite$`));
	const dialog = page.getByRole('dialog');
	await expect(dialog).toContainText('then go back to your invitation');
	await page.locator('#role-name').fill('Copy desk');
	await page.getByRole('button', { name: 'Create and choose what it may do' }).click();
	await expect(page.getByRole('heading', { level: 1, name: 'Copy desk' })).toBeVisible();
	await expect(page.getByTestId('role-for-invite')).toContainText('Choose what Copy desk may do, then save.');
	expect(await violations(page)).toEqual([]);
	await page.getByLabel('Edit: Articles').check();
	await page.getByRole('button', { name: 'Save role' }).click();

	// Back at the invitation, with the new role chosen and what was typed kept.
	await expect(page).toHaveURL(new RegExp(`/projects/${project}/members\\?role=copy-desk#invite$`));
	await expect(page.getByLabel('Role', { exact: true })).toHaveValue('copy-desk');
	await expect(page.getByLabel('Who is it for?')).toHaveValue('Sam, copy editor');
	await expect(page.getByLabel('Who is it for?')).toBeFocused();
	await page.getByRole('button', { name: 'Create invitation link' }).click();
	await expect(page.getByTestId('invitations')).toContainText('Copy desk');
	expect(await violations(page)).toEqual([]);

	// Back to the invitation without saving works too, and Manage roles opens Roles.
	await page.goto(`/studio/projects/${project}/settings/roles/copy-desk?for=invite`);
	await page.getByRole('link', { name: 'Back to the invitation' }).click();
	await expect(page.getByLabel('Role', { exact: true })).toHaveValue('copy-desk');
	await page.getByTestId('manage-roles').click();
	await expect(page.getByRole('heading', { level: 1, name: 'Roles' })).toBeVisible();
	await expect(page.getByTestId('roles')).toContainText('Copy desk');
});
