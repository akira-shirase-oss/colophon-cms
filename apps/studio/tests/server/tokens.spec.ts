// API tokens in Settings (step R.4): an owner makes a token, sees its secret once and copies it;
// the token works from outside the browser with `Authorization: Bearer`, with exactly its powers;
// last use shows up; revoking asks first and then the token is refused (401). A custom token is
// chosen in the role editor's grid, without the four powers only people hold.

import { expect, type Page, test } from '@playwright/test';
import { env, internalTerms, key, newEntry, newProject, shot, signIn, violations } from './helpers.ts';

/** A request from outside any browser session, as a script would make it. */
async function bearer(page: Page, secret: string, method: string, path: string, body?: unknown): Promise<{ status: number; body: any }> {
	const response = await fetch(`${new URL(page.url()).origin}${path}`, {
		method,
		headers: {
			Authorization: `Bearer ${secret}`,
			...(body === undefined ? {} : { 'Content-Type': 'application/json', 'Idempotency-Key': key() })
		},
		...(body === undefined ? {} : { body: JSON.stringify(body) })
	});
	return { status: response.status, body: await response.json().catch(() => null) };
}

const article = (title: string) => ({ title, body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } });

test('an owner makes a token, sees its secret once, uses it, and revoking it stops it', async ({ page, context }) => {
	await context.grantPermissions(['clipboard-read', 'clipboard-write']);
	await signIn(page);
	const project = await newProject(page);
	await newEntry(page, project, 'Readable by the token');
	await page.goto(`/studio/projects/${project}/settings`);
	await page.getByTestId('tokens-link').click();
	await expect(page.getByRole('heading', { level: 1, name: 'API tokens' })).toBeVisible();
	await expect(page.getByTestId('tokens-empty')).toBeVisible();
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);

	await page.getByTestId('tokens-empty').getByRole('button', { name: 'New token' }).click();
	await page.getByLabel('Name', { exact: true }).fill('Site build');
	await page.getByLabel('Works for').selectOption('30');
	await page.getByRole('button', { name: 'Make the token' }).click();
	const shown = page.getByTestId('token-secret');
	await expect(shown).toContainText("You won't see it again.");
	const secret = await page.getByTestId('token-secret-value').inputValue();
	expect(secret).toMatch(/^colophon_api_/);
	await shown.getByRole('button', { name: 'Copy' }).click();
	await expect(shown.getByRole('button', { name: 'Copied' })).toBeVisible();
	expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(secret);
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);
	const row = page.getByTestId('token').filter({ hasText: 'Site build' });
	await expect(row.getByTestId('token-state')).toHaveText('Active');
	await expect(row).toContainText('Read only');
	await expect(row.getByTestId('token-used')).toHaveText('Never used');
	await shot(page, 'tokens-secret');

	// From a script: it reads, and it may not write.
	const read = await bearer(page, secret, 'GET', `${env(project)}/collections/articles/entries`);
	expect(read.status).toBe(200);
	expect(read.body.data.map((entry: { revision: { title: string } }) => entry.revision.title)).toContain('Readable by the token');
	const write = await bearer(page, secret, 'POST', `${env(project)}/collections/articles/entries`, article('Not allowed'));
	expect(write.status).toBe(403);
	expect(write.body.error.details.reason).toBe('missing_permission');

	// Its last use is recorded outside the request; once it is, the page says so. The secret is gone.
	await expect
		.poll(async () => (await page.evaluate(async (path) => ((await (await fetch(path)).json()) as { data: { last_used_at: string | null }[] }).data[0]?.last_used_at ?? null, `/api/v1/projects/${project}/tokens`)), { timeout: 20_000 })
		.not.toBeNull();
	await page.reload();
	await expect(page.getByTestId('token-secret')).toHaveCount(0);
	await expect(row.getByTestId('token-used')).toContainText('Last used');

	// Revoking asks first, and can be called off.
	await row.getByRole('button', { name: 'Revoke Site build' }).click();
	await expect(page.getByRole('alertdialog')).toContainText("can't be undone");
	expect(await violations(page)).toEqual([]);
	await page.getByRole('button', { name: 'Keep it' }).click();
	await expect(row.getByTestId('token-state')).toHaveText('Active');
	await row.getByRole('button', { name: 'Revoke Site build' }).click();
	await page.getByRole('button', { name: 'Revoke it' }).click();
	await expect(row.getByTestId('token-state')).toHaveText('Revoked');
	await expect(row.getByRole('button', { name: /Revoke/ })).toHaveCount(0);
	expect((await bearer(page, secret, 'GET', `${env(project)}/collections/articles/entries`)).status).toBe(401);
});

test('a custom token is chosen in the grid, without the powers only people hold, and holds exactly them', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/settings/tokens`);
	await page.getByRole('button', { name: 'New token' }).click();
	await page.getByLabel('Name', { exact: true }).fill('Importer');
	await page.getByRole('radio', { name: /Custom/ }).check();
	const grid = page.getByTestId('token-grid');
	await expect(grid.getByLabel('See: Articles')).toBeVisible();
	await expect(grid.getByText('Manage where releases go')).toBeVisible();
	for (const peopleOnly of ['Invite people and change their roles', 'Make and change roles', 'Manage API tokens', 'See the activity log']) {
		await expect(grid.getByText(peopleOnly)).toHaveCount(0);
	}
	await expect(page.getByRole('button', { name: 'Make the token' })).toBeDisabled();
	await grid.getByLabel('See: Articles').check();
	await grid.getByLabel('Create: Articles').check();
	expect(await violations(page)).toEqual([]);
	await page.getByRole('button', { name: 'Make the token' }).click();
	const secret = await page.getByTestId('token-secret-value').inputValue();
	await expect(page.getByTestId('token').filter({ hasText: 'Importer' })).toContainText('Custom: see Articles, create Articles');

	const made = await bearer(page, secret, 'POST', `${env(project)}/collections/articles/entries`, article('Imported'));
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	const roles = await bearer(page, secret, 'GET', `/api/v1/projects/${project}/roles`);
	expect(roles.status).toBe(403);

	// On a phone, the page reads without sideways scrolling.
	await page.setViewportSize({ width: 390, height: 844 });
	await page.reload();
	await expect(page.getByTestId('tokens')).toContainText('Importer');
	await page.waitForLoadState('networkidle');
	expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'API tokens scroll sideways on a phone').toBeLessThanOrEqual(0);
	expect(await violations(page)).toEqual([]);
});
