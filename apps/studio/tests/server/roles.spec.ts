// Regression (found by the local check, 2026-10-26): pages decided what to show from the role in
// the signed-in person's project list, which /me caps at 1,000 projects. Someone in more projects
// than that lost every owner control in the rest. Pages now ask the project itself.

import { expect, test } from '@playwright/test';
import { newProject, signIn } from './helpers.ts';

test('a page reads your role from the project, not from your list of projects', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	// As if they belonged to more projects than /me lists: this one isn't in their list.
	await page.route('**/me', async (route) => {
		const response = await route.fetch();
		const body = await response.json();
		body.data.projects = [];
		await route.fulfill({ response, json: body });
	});

	await page.goto(`/studio/projects/${project}/members`);
	await expect(page.getByLabel('Role of Dev Owner')).toHaveValue('owner');
	await page.goto(`/studio/projects/${project}/settings/connections`);
	await page.getByRole('button', { name: 'Add a connection' }).click();
	await expect(page.getByLabel('Connection name')).toBeVisible();
	await page.keyboard.press('Escape');
	await page.goto(`/studio/projects/${project}/settings/roles`);
	await expect(page.getByRole('button', { name: 'New role' })).toBeVisible();
});
