// Members and invitations through the testbed, in two real browsers: an owner invites, someone
// else looks the invitation up and accepts it once, the last owner can't step down, a promoted
// member can, and a revoked invitation is gone.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch('/server/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'Members testbed' })
		});
		return ((await response.json()) as { data: { id: string } }).data.id;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('invite, look up, accept once, keep an owner, and revoke', async ({ page, browser }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/members');
	await page.getByLabel('Project').selectOption(projectId);
	await expect(page.getByTestId('members').locator('li')).toHaveCount(1);

	await page.getByLabel('Label').fill('Guest publisher');
	await page.getByLabel('Invitation role').selectOption('publisher');
	await page.getByRole('button', { name: 'Create invitation' }).click();
	await expect(page.getByTestId('message')).toHaveText('invited');
	const link = (await page.getByTestId('link').textContent()) ?? '';
	expect(link).toMatch(/\/studio\/invite#[A-Za-z0-9_-]{43}$/);
	await expect(page.getByTestId('invitations')).toContainText('Guest publisher');

	// Someone else, in their own browser.
	const guest = await (await browser.newContext()).newPage();
	await signIn(guest, 'dev-editor');
	await guest.goto('/members');
	await guest.getByLabel('Invitation link or token').fill(link);
	await guest.getByRole('button', { name: 'Look up' }).click();
	await expect(guest.getByTestId('offer')).toHaveText('Members testbed as publisher, from Dev Owner');
	await guest.getByRole('button', { name: 'Accept' }).click();
	await expect(guest.getByTestId('message')).toHaveText('joined as publisher');
	await guest.getByRole('button', { name: 'Accept' }).click();
	await expect(guest.getByTestId('message')).toHaveText('404 not_found');

	// The owner can't step down alone, can after promoting the guest, and can remove them.
	await page.reload();
	await page.getByLabel('Project').selectOption(projectId);
	await expect(page.getByTestId('members').locator('li')).toHaveCount(2);
	await expect(page.getByTestId('invitations').locator('li')).toHaveCount(0);
	await page.getByLabel('Role of Dev Owner').selectOption('editor');
	await expect(page.getByTestId('message')).toHaveText('409 conflict');
	await page.getByLabel('Role of Dev Editor').selectOption('owner');
	await expect(page.getByTestId('message')).toHaveText('role changed to owner');
	await page.getByRole('button', { name: 'Remove Dev Editor' }).click();
	await expect(page.getByTestId('message')).toHaveText('removed');
	await expect(page.getByTestId('members').locator('li')).toHaveCount(1);

	// A revoked invitation is gone at once.
	await page.getByLabel('Label').fill('Never mind');
	await page.getByRole('button', { name: 'Create invitation' }).click();
	const revoked = (await page.getByTestId('link').textContent()) ?? '';
	await page.getByRole('button', { name: 'Revoke Never mind' }).click();
	await expect(page.getByTestId('message')).toHaveText('revoked');
	await guest.getByLabel('Invitation link or token').fill(revoked);
	await guest.getByRole('button', { name: 'Look up' }).click();
	await expect(guest.getByTestId('message')).toHaveText('404 not_found');
});
