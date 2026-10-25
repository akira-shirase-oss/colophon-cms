// Sign-in and project isolation through the testbed, in a real browser, against the server's
// real OpenID Connect flow and the stack's Keycloak.

import { expect, type Page, test } from '@playwright/test';

async function signIn(page: Page, user: string) {
	await page.goto('/session');
	await expect(page.getByTestId('signed-out')).toBeVisible();
	await page.getByTestId('sign-in').click();
	// Keycloak's own login form.
	await page.locator('#username').fill(user);
	await page.locator('#password').fill('colophon-dev');
	await page.locator('#kc-login').click();
	// The server finishes the sign-in and sends the browser to /me on its own origin.
	await page.waitForURL(/\/me$/);
	await page.goto('/session');
	await expect(page.getByTestId('signed-in')).toBeVisible();
}

test('sign in, create a project, and it stays invisible to someone else', async ({ browser }) => {
	const editorContext = await browser.newContext();
	const editor = await editorContext.newPage();
	await signIn(editor, 'dev-editor');
	await expect(editor.getByTestId('signed-in')).toContainText('Dev Editor');

	const slug = `tb-${Date.now().toString(36)}`;
	await editor.getByLabel('Slug').fill(slug);
	await editor.getByLabel('Name').fill('Testbed project');
	await editor.getByRole('button', { name: 'Create project' }).click();
	await expect(editor.getByTestId('message')).toHaveText(`created ${slug}`);
	const item = editor.locator(`[data-slug="${slug}"]`);
	await expect(item).toContainText('owner');
	const projectId = await item.getAttribute('data-id');

	// The same slug again: a conflict, explained.
	await editor.getByRole('button', { name: 'Create project' }).click();
	await expect(editor.getByTestId('message')).toContainText('409 conflict');

	// Another person, in another browser, can't see it: not listed, and 404 by id.
	const ownerContext = await browser.newContext();
	const owner = await ownerContext.newPage();
	await signIn(owner, 'dev-owner');
	await expect(owner.locator(`[data-slug="${slug}"]`)).toHaveCount(0);
	const status = await owner.evaluate(async (id) => (await fetch(`/server/api/v1/projects/${id}`)).status, projectId);
	expect(status).toBe(404);

	// Signing out ends the session.
	await editor.getByRole('button', { name: 'Sign out' }).click();
	await expect(editor.getByTestId('signed-out')).toContainText('HTTP 401');
	await editorContext.close();
	await ownerContext.close();
});

test('a write without the CSRF token is refused', async ({ page }) => {
	await signIn(page, 'dev-editor');
	const status = await page.evaluate(async () => {
		const response = await fetch('/server/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ slug: 'never-created', name: 'x' })
		});
		return response.status;
	});
	expect(status).toBe(403);
});
