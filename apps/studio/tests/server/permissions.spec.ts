// Roles owners define, in the Studio against a real server (D-058, step R.2): an owner makes a
// role in Settings and ticks what it may do, someone invited with it sees exactly that role,
// a second tab's save is refused instead of overwriting the first, a role in use can't be
// deleted, and the owner role is shown but never edited.

import { expect, test } from '@playwright/test';
import { keycloak, newProject, signIn, violations } from './helpers.ts';

test('an owner makes a role, chooses what it may do, and someone invited with it holds exactly that', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/settings`);
	await expect(page.getByTestId('roles')).toContainText('Owner');
	await expect(page.getByTestId('roles')).toContainText('Publisher');

	// Make it; its short name follows the name until it's edited.
	await page.locator('#role-name').fill('Copy desk');
	await expect(page.getByLabel('Short name')).toHaveValue('copy-desk');
	await page.getByRole('button', { name: 'Create and choose what it may do' }).click();
	await expect(page.getByRole('heading', { level: 1, name: 'Copy desk' })).toBeVisible();
	await expect(page.getByRole('button', { name: 'Save role' })).toBeDisabled();
	expect(await violations(page)).toEqual([]);

	// "Everything" covers each content type; one content type is just that one.
	await page.getByLabel('See: Everything').check();
	await expect(page.getByLabel('See: Articles')).toBeChecked();
	await expect(page.getByLabel('See: Articles')).toBeDisabled();
	await page.getByLabel('Edit: Articles').check();
	await page.getByLabel('Create: Articles').check();
	await page.getByLabel("What it's for").fill('Edits articles before they go out.');
	await page.getByRole('button', { name: 'Save role' }).click();
	await expect(page.getByText('Version 2')).toBeVisible();
	await expect(page.getByLabel('Edit: Articles')).toBeChecked();
	await expect(page.getByLabel('Publish: Articles')).not.toBeChecked();

	// A second tab still on version 2 loses to the first tab's save, and says so. Opening it cold
	// also covers a page whose three queries arrive in any order (a real bug, fixed in the page).
	const other = await page.context().newPage();
	await other.goto(page.url());
	await other.getByLabel('Approve: Articles').check();
	await page.getByLabel('Publish: Articles').check();
	await page.getByRole('button', { name: 'Save role' }).click();
	await expect(page.getByText('Version 3')).toBeVisible();
	await other.getByRole('button', { name: 'Save role' }).click();
	await expect(other.getByTestId('role-error')).toContainText('Someone changed this role while you were editing it');
	await other.getByRole('button', { name: 'Load the latest version' }).click();
	await expect(other.getByText('Version 3')).toBeVisible();
	await expect(other.getByLabel('Publish: Articles')).toBeChecked();
	await expect(other.getByLabel('Approve: Articles')).not.toBeChecked();
	await other.close();

	// Invite someone with it; the invitation names it and says what it's for.
	await page.goto(`/studio/projects/${project}/members`);
	await page.getByLabel('Who is it for?').fill('A copy editor');
	await page.getByLabel('Role', { exact: true }).selectOption('copy-desk');
	await expect(page.getByText('Edits articles before they go out.')).toBeVisible();
	await page.getByRole('button', { name: 'Create invitation link' }).click();
	const link = await page.getByRole('textbox', { name: 'Invitation link' }).inputValue();
	const guest = await (await browser.newContext()).newPage();
	await guest.goto(new URL(link).pathname + new URL(link).hash);
	await guest.getByRole('link', { name: 'Sign in' }).click();
	await keycloak(guest, 'dev-editor');
	await expect(guest.getByTestId('invite-ask')).toContainText('as Copy desk: Edits articles before they go out.');
	await guest.getByRole('button', { name: 'Join', exact: true }).click();
	await expect(guest.getByTestId('invite-outcome')).toContainText('You joined Studio test as Copy desk.');

	// They can read what their role means, but not change it, nor manage people.
	await guest.goto(`/studio/projects/${project}/settings/roles/copy-desk`);
	await expect(guest.getByTestId('role-read-only')).toBeVisible();
	await expect(guest.getByLabel('Edit: Articles')).toBeChecked();
	await expect(guest.getByLabel('Edit: Articles')).toBeDisabled();
	await expect(guest.getByRole('button', { name: 'Save role' })).toHaveCount(0);
	expect(await violations(guest)).toEqual([]);
	await guest.goto(`/studio/projects/${project}/members`);
	await expect(guest.getByTestId('member')).toHaveCount(2);
	await expect(guest.getByRole('button', { name: 'Create invitation link' })).toHaveCount(0);

	// Held, the role can't be deleted; the owner role is never edited.
	await page.goto(`/studio/projects/${project}/settings/roles/copy-desk`);
	await expect(page.getByTestId('role-in-use')).toContainText('1 person holds this role');
	await expect(page.getByRole('button', { name: /Delete role/ })).toHaveCount(0);
	await page.goto(`/studio/projects/${project}/settings/roles/owner`);
	await expect(page.getByTestId('role-fixed')).toBeVisible();
	await expect(page.getByLabel('Publish: Everything')).toBeChecked();
	await expect(page.getByLabel('Publish: Everything')).toBeDisabled();
	await expect(page.getByRole('button', { name: 'Save role' })).toHaveCount(0);
	expect(await violations(page)).toEqual([]);

	// Given another role, nobody holds it, and it goes.
	await page.goto(`/studio/projects/${project}/members`);
	// The select shows the new value at once; wait for the server's answer before leaving.
	const changed = page.waitForResponse((response) => response.url().includes('/members/') && response.request().method() === 'PUT');
	await page.getByLabel('Role of Dev Editor').selectOption('editor');
	expect((await changed).status()).toBe(204);
	await page.goto(`/studio/projects/${project}/settings/roles/copy-desk`);
	await page.getByRole('button', { name: 'Delete role…' }).click();
	await page.getByRole('button', { name: 'Delete Copy desk' }).click();
	await expect(page).toHaveURL(new RegExp(`/studio/projects/${project}/settings$`));
	await expect(page.getByTestId('roles')).not.toContainText('Copy desk');
});

/** Runs `fetch` calls as the signed-in page: JSON in, JSON out. */
async function api(page: import('@playwright/test').Page, method: string, path: string, body?: unknown, headers: Record<string, string> = {}) {
	return page.evaluate(
		async ({ method, path, body, headers }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const response = await fetch(path, {
				method,
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, ...headers },
				...(body === undefined ? {} : { body: JSON.stringify(body) })
			});
			return { status: response.status, body: (await response.json().catch(() => null)) as Record<string, any> | null };
		},
		{ method, path, body, headers }
	);
}

test('an author retitles someone else’s article without seeing or erasing its body, and the role keeps its limits', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	const base = `/api/v1/projects/${project}`;
	const role = await api(page, 'POST', `${base}/roles`, {
		key: 'author',
		name: 'Author',
		permissions: [
			{ action: 'content.read', subject: 'articles', fields: ['title'] },
			{ action: 'content.read', subject: 'articles', condition: 'own' },
			{ action: 'content.create', subject: 'articles' },
			{ action: 'content.update', subject: 'articles', fields: ['title'] },
			{ action: 'content.update', subject: 'articles', condition: 'own' }
		]
	});
	expect(role.status).toBe(201);
	const doc = { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'The owner’s private body' }] }] };
	const entry = await api(
		page,
		'POST',
		`${base}/environments/production/collections/articles/entries`,
		{ title: 'Owner’s note', body: { schema: 'colophon/rich-text', version: 1, doc } },
		{ 'Idempotency-Key': crypto.randomUUID() }
	);
	expect(entry.status).toBe(201);
	const entryId = entry.body?.data.id as string;
	const invited = await api(page, 'POST', `${base}/invitations`, { role: 'author', label: 'An author' });
	const token = String(invited.body?.data.link).split('#')[1];

	const author = await (await browser.newContext()).newPage();
	await signIn(author, 'dev-editor');
	expect((await api(author, 'POST', '/api/v1/invitations/accept', { token })).status).toBe(200);
	await author.goto(`/studio/projects/${project}/articles/${entryId}`);
	await expect(author.getByTestId('role-limits')).toContainText('doesn’t show body');
	await expect(author.getByRole('textbox', { name: 'Body' })).toHaveCount(0);
	await author.getByLabel('Title', { exact: true }).fill('Owner’s note, retitled');
	await expect(author.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	expect(await violations(author)).toEqual([]);

	// The owner sees the new title and the body exactly as it was.
	const after = await api(page, 'GET', `${base}/environments/production/collections/articles/entries/${entryId}`);
	expect(after.body?.data.content.title).toBe('Owner’s note, retitled');
	expect(after.body?.data.content.body).toEqual(entry.body?.data.content.body);

	// Saving the role from the grid keeps its limited permissions.
	await page.goto(`/studio/projects/${project}/settings/roles/author`);
	const limits = page.getByTestId('limited-permissions');
	await expect(limits).toContainText('Edit: Articles, only title');
	await expect(limits).toContainText('Edit: Articles, only their own entries');
	await page.getByLabel('Publish: Articles').check();
	await page.getByRole('button', { name: 'Save role' }).click();
	await expect(page.getByText('Version 2')).toBeVisible();
	await page.reload();
	await expect(page.getByTestId('limited-permissions')).toContainText('Edit: Articles, only their own entries');
	await expect(page.getByLabel('Publish: Articles')).toBeChecked();
	expect(await violations(page)).toEqual([]);
});

// Step R.4: the activity log's permission, in plain words, and what managing tokens means.
test('the role editor names the activity log in plain words, and says what managing tokens means', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/settings/roles/owner`);
	await expect(page.getByLabel('See the activity log')).toBeChecked();
	await expect(page.getByLabel('See the activity log')).toBeDisabled();
	await expect(page.getByTestId('role-project')).toContainText('Make and revoke keys that let scripts and sites use the API');
	await expect(page.getByTestId('role-project')).not.toContainText('arrive in a later step');
	await page.goto(`/studio/projects/${project}/settings/roles/publisher`);
	await expect(page.getByLabel('See the activity log')).not.toBeChecked();
	await page.getByLabel('See the activity log').check();
	await page.getByRole('button', { name: 'Save role' }).click();
	await expect(page.getByText('Version 2')).toBeVisible();
	await expect(page.getByLabel('See the activity log')).toBeChecked();
	expect(await violations(page)).toEqual([]);
});
