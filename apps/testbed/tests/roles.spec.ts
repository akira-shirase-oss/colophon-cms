// Roles owners define, through the testbed (D-058, step R.2): create a role with permissions,
// change it at its version, lose to a stale version, have a field list where it can't apply and
// the owner role refused, and delete it once nobody holds it.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch('/server/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'Roles testbed' })
		});
		return ((await response.json()) as { data: { id: string } }).data.id;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('create a role, change it at its version, refuse what is not enforced, delete it', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/roles');
	await page.getByLabel('Project').selectOption(projectId);
	await expect(page.getByTestId('roles').locator('li')).toHaveCount(4);
	await expect(page.locator('li[data-key="owner"]')).toContainText('fixed');

	await page.getByLabel('Key').fill('desk');
	await page.getByLabel('Name').fill('Desk');
	await page.getByRole('button', { name: 'Create' }).click();
	await expect(page.getByTestId('message')).toHaveText('created desk v1');
	await expect(page.locator('li[data-key="desk"]')).toContainText('content.read on articles');

	await page.getByLabel('Permissions (JSON)').fill('[{"action": "content.read", "subject": "*"}, {"action": "content.update", "subject": "articles"}]');
	await page.getByRole('button', { name: 'Save' }).click();
	await expect(page.getByTestId('message')).toHaveText('saved desk v2');
	await expect(page.locator('li[data-key="desk"]')).toContainText('content.update on articles');
	await page.getByRole('button', { name: 'Save' }).click();
	await expect(page.getByTestId('message')).toHaveText('409 conflict (stale_role)');

	await page.getByLabel('Version (If-Match)').fill('2');
	await page.getByLabel('Permissions (JSON)').fill('[{"action": "content.publish", "subject": "articles", "fields": ["title"]}]');
	await page.getByRole('button', { name: 'Save' }).click();
	await expect(page.getByTestId('message')).toHaveText('422 validation_failed (fields_not_applicable)');

	await page.getByLabel('Key').fill('owner');
	await page.getByLabel('Permissions (JSON)').fill('[]');
	await page.getByRole('button', { name: 'Save' }).click();
	await expect(page.getByTestId('message')).toHaveText('409 conflict');

	await page.getByLabel('Key').fill('desk');
	await page.getByRole('button', { name: 'Delete' }).click();
	await expect(page.getByTestId('message')).toHaveText('deleted desk');
	await expect(page.getByTestId('roles').locator('li')).toHaveCount(4);
});
