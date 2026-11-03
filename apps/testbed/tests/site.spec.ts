// The website starter (W.0) through the testbed, in a real browser, against the real server:
// installing writes the component, the three content types and the starter entries; the menu's
// first item names the Home page's id and the theme holds the default palette; a second run
// reports everything already there.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch('/server/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'Site testbed' })
		});
		if (!response.ok) throw new Error(`setup: project: HTTP ${response.status} ${await response.text()}`);
		return ((await response.json()) as { data: { id: string } }).data.id;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('the website starter installs once and reruns harmlessly', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/site');
	await page.getByLabel('Project').selectOption(projectId);

	await page.getByTestId('install').click();
	await expect(page.getByTestId('message')).toHaveText('installed');
	const steps = page.getByTestId('steps');
	await expect(steps).toContainText('section-style: added');
	await expect(steps).toContainText('pages: added');
	await expect(steps).toContainText('navigation: added');
	await expect(steps).toContainText('theme: added');
	await expect(steps).toContainText('home page: written');
	await expect(steps).toContainText('navigation entry: written');
	await expect(steps).toContainText('theme entry: written');

	const homeId = await page.getByTestId('home-id').textContent();
	expect(homeId).toMatch(/^[0-9a-f-]{36}$/);
	await expect(page.getByTestId('nav-item')).toHaveText(`Home → ${homeId}`);
	await expect(page.getByTestId('palette')).toHaveText('primary #6d28d9, text #18181b, radius-md 8px');

	// A second run keeps everything it finds.
	await page.getByTestId('install').click();
	await expect(page.getByTestId('message')).toHaveText('installed');
	const again = page.getByTestId('steps');
	await expect(again).toContainText('section-style: already there');
	await expect(again).toContainText('pages: already there');
	await expect(again).toContainText('home page: already there');
	await expect(again).toContainText('navigation entry: already there');
	await expect(again).toContainText('theme entry: already there');
});
