// The site's own visitors through the testbed (D-137): staff name an open audience role and a
// closed one, make a member account, publish one public and one `role:member` article, and the
// public arm signs a visitor up, proves Postgres gates delivery for it (anonymous sees the
// public row only), revokes its session, and sees a disabled account refused. All through the
// real API — the visitor arm holds the session as a bearer credential, like a site does.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch('/server/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'Audience testbed' })
		});
		return ((await response.json()) as { data: { id: string } }).data.id;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

/** A public article and a `role:member` one, published as release 1 through a changeset. */
async function publishGated(page: Page, project: string): Promise<number> {
	return page.evaluate(async (project) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const headers = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
		const base = `/server/api/v1/projects/${project}/environments/production`;
		const doc = { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } };
		const entries: string[] = [];
		for (const [title, gate] of [['For everyone', ''], ['For members', '?visibility=role%3Amember']] as const) {
			const made = await fetch(`${base}/collections/articles/entries${gate}`, {
				method: 'POST',
				headers: { ...headers, 'Idempotency-Key': crypto.randomUUID() },
				body: JSON.stringify({ title, body: doc })
			});
			entries.push(((await made.json()) as { data: { id: string } }).data.id);
		}
		const made = await fetch(`${base}/changesets`, {
			method: 'POST',
			headers: { ...headers, 'Idempotency-Key': crypto.randomUUID() },
			body: JSON.stringify({ title: 'Gated', entries })
		});
		const set = ((await made.json()) as { data: { id: string; hash: string } }).data;
		const ifMatch = { ...headers, 'If-Match': `"${set.hash}"` };
		await fetch(`${base}/changesets/${set.id}/submit`, { method: 'POST', headers: ifMatch });
		await fetch(`${base}/changesets/${set.id}/reviews`, { method: 'POST', headers: ifMatch, body: JSON.stringify({ decision: 'approve' }) });
		const published = await fetch(`${base}/changesets/${set.id}/publish`, {
			method: 'POST',
			headers: { ...ifMatch, 'Idempotency-Key': crypto.randomUUID() },
			body: JSON.stringify({ base_generation: 0 })
		});
		return published.status;
	}, project);
}

test('audience roles, a member account, gated delivery, revocation and a disabled account', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/audience');
	await page.getByLabel('Project').selectOption(projectId);

	// An open role visitors take at signup, and a closed one staff alone grants.
	await page.getByLabel('Name', { exact: true }).fill('member');
	await page.getByRole('button', { name: 'Create role' }).click();
	await expect(page.getByTestId('message')).toHaveText('created role member');
	await expect(page.locator('li[data-name="member"]')).toContainText('open to signup');
	await page.getByLabel('Name', { exact: true }).fill('vip');
	await page.getByLabel('open to signup').uncheck();
	await page.getByRole('button', { name: 'Create role' }).click();
	await expect(page.getByTestId('message')).toHaveText('created role vip');
	await expect(page.locator('li[data-name="vip"]')).toContainText('staff-assigned');

	// Staff make an account straight into the closed role; public signup never could.
	await page.getByLabel('Email', { exact: true }).first().fill('alice@example.test');
	await page.getByLabel('Password', { exact: true }).first().fill('alice-member-pass');
	await page.getByLabel('Role', { exact: true }).fill('vip');
	await page.getByRole('button', { name: 'Create user' }).click();
	await expect(page.getByTestId('message')).toHaveText('created user alice@example.test');
	await expect(page.locator('li[data-email="alice@example.test"]')).toContainText('role vip');

	// One public row, one role:member row — before the visitor exists.
	expect(await publishGated(page, projectId)).toBe(201);

	// The visitor arm: signup takes the single open role, and the session gates delivery.
	await page.getByLabel('Email', { exact: true }).nth(1).fill('bob@example.test');
	await page.getByLabel('Password', { exact: true }).nth(1).fill('reader-of-the-draft');
	await page.getByRole('button', { name: 'Sign up' }).click();
	await expect(page.getByTestId('message')).toHaveText('signed up as member');
	await expect(page.getByTestId('visitor')).toContainText('bob@example.test');
	await page.getByRole('button', { name: 'Who am I' }).click();
	await expect(page.getByTestId('message')).toHaveText('me: bob@example.test');
	await page.getByRole('button', { name: 'Check delivery' }).click();
	await expect(page.getByTestId('delivery')).toHaveText('anonymous sees 1, this session sees 2');

	// Staff see the live session and end it; the visitor's next read says so.
	await page.locator('li[data-email="bob@example.test"]').getByRole('button', { name: 'Sessions' }).click();
	await expect(page.getByTestId('sessions').getByRole('listitem')).toHaveCount(1);
	await page.getByRole('button', { name: 'Revoke' }).click();
	await expect(page.getByTestId('message')).toContainText('revoked session');
	await page.getByRole('button', { name: 'Who am I' }).click();
	await expect(page.getByTestId('message')).toHaveText('401 unauthorized');

	// Signed out, the same delivery read is the anonymous one again: a dead credential
	// degrades to anonymous — delivery never 401s a public collection.
	await page.getByRole('button', { name: 'Sign out' }).click();
	await expect(page.getByTestId('visitor')).toContainText('Signed out');
	await page.getByRole('button', { name: 'Check delivery' }).click();
	await expect(page.getByTestId('delivery')).toHaveText('anonymous sees 1, this session sees 1');

	// A disabled account is refused at sign-in, named honestly.
	await page.locator('li[data-email="bob@example.test"]').getByRole('button', { name: 'Disable' }).click();
	await expect(page.getByTestId('message')).toHaveText('bob@example.test disabled');
	await page.getByRole('button', { name: 'Sign in' }).click();
	await expect(page.getByTestId('message')).toHaveText('403 forbidden (account_disabled)');

	// The role gate is the role, not the account: vip sees the public row only.
	await page.getByLabel('Email', { exact: true }).nth(1).fill('alice@example.test');
	await page.getByLabel('Password', { exact: true }).nth(1).fill('alice-member-pass');
	await page.getByRole('button', { name: 'Sign in' }).click();
	await expect(page.getByTestId('message')).toHaveText('signed in as vip');
	await page.getByRole('button', { name: 'Check delivery' }).click();
	await expect(page.getByTestId('delivery')).toHaveText('anonymous sees 1, this session sees 1');
});

// Passwordless (D-140): a magic link is requested and spent through the same public API a
// site uses — the testbed's server runs COLOPHON_DEV_MAIL, so the answer carries the link a
// production server would mail.
test('a magic link signs a visitor in without a password, once', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/audience');
	await page.getByLabel('Project').selectOption(projectId);

	await page.getByLabel('Name', { exact: true }).fill('member');
	await page.getByRole('button', { name: 'Create role' }).click();
	await expect(page.getByTestId('message')).toHaveText('created role member');
	expect(await publishGated(page, projectId)).toBe(201);

	// An email with no account: the link is the signup — spending it makes a member.
	await page.getByLabel('Email', { exact: true }).nth(2).fill('carol@example.test');
	await page.getByRole('button', { name: 'Send the link' }).click();
	await expect(page.getByTestId('dev-link')).toContainText('token=');
	await page.getByRole('button', { name: 'Use the link' }).click();
	await expect(page.getByTestId('message')).toHaveText('signed in by link as member');
	await expect(page.getByTestId('visitor')).toContainText('carol@example.test');
	await page.getByRole('button', { name: 'Check delivery' }).click();
	await expect(page.getByTestId('delivery')).toHaveText('anonymous sees 1, this session sees 2');

	// The account the link made signs in by link again — and the spent link is dead.
	await page.getByLabel('Email', { exact: true }).nth(2).fill('carol@example.test');
	await page.getByRole('button', { name: 'Send the link' }).click();
	await expect(page.getByTestId('dev-link')).toContainText('token=');
	const link = await page.getByTestId('dev-link').textContent();
	const token = link?.split('token=').at(-1) ?? '';
	await page.getByRole('button', { name: 'Use the link' }).click();
	await expect(page.getByTestId('message')).toHaveText('signed in by link as member');
	// Spending the same token a second time names it invalid.
	const spent = await page.evaluate(
		async ([project, token]) =>
			(await fetch(`/server/audience/v1/${project}/magic-link/${token}`, { method: 'POST' })).status,
		[projectId, token]
	);
	expect(spent).toBe(403);
});
