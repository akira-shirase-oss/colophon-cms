// Uploads through the testbed, in a real browser, against the real server and its object store:
// a real PNG is quarantined, verified and becomes usable with its proved type and dimensions;
// HTML disguised as an image is rejected.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch('/server/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'Asset testbed' })
		});
		if (!response.ok) throw new Error(`setup: project: HTTP ${response.status} ${await response.text()}`);
		return ((await response.json()) as { data: { id: string } }).data.id;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('uploads are verified, or rejected, and listed', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/assets');
	await page.getByTestId('project').selectOption(projectId);

	await page.getByTestId('upload-real').click();
	await expect(page.getByTestId('message')).toHaveText('settled', { timeout: 20_000 });
	await expect(page.getByTestId('steps')).toContainText('uploaded: 202 quarantined');
	await expect(page.getByTestId('steps')).toContainText('usable: image/png');
	await expect(page.getByTestId('steps')).toContainText('served: 200 image/png');
	await expect(page.getByTestId('assets')).toContainText('dot.png');

	await page.getByTestId('upload-fake').click();
	await expect(page.getByTestId('message')).toHaveText('settled', { timeout: 20_000 });
	await expect(page.getByTestId('steps')).toContainText('rejected');
	await expect(page.getByTestId('steps')).toContainText('served: 404');
	await expect(page.getByTestId('assets')).toContainText('photo.png');
});
