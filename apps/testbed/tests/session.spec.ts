// Sign-in and project isolation through the testbed, in a real browser, against the server's
// real OpenID Connect flow and the stack's Keycloak.

import { expect, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

test('sign in, create a project, and it stays invisible to someone else', async ({ browser }) => {
	const editorContext = await browser.newContext();
	const editor = await editorContext.newPage();
	await signIn(editor, 'dev-owner');
	await expect(editor.getByTestId('signed-in')).toContainText('Dev Owner');

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
	const otherContext = await browser.newContext();
	const other = await otherContext.newPage();
	await signIn(other, 'dev-editor');
	await expect(other.locator(`[data-slug="${slug}"]`)).toHaveCount(0);
	const status = await other.evaluate(async (id) => (await fetch(`/server/api/v1/projects/${id}`)).status, projectId);
	expect(status).toBe(404);

	// Signing out ends the session.
	await editor.getByRole('button', { name: 'Sign out' }).click();
	await expect(editor.getByTestId('signed-out')).toContainText('HTTP 401');
	await editorContext.close();
	await otherContext.close();
});

test('a write without the CSRF token is refused', async ({ page }) => {
	await signIn(page, 'dev-owner');
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
