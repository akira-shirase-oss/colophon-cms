// Collection transfer (D-086) through the testbed, in a real browser, against the real
// server: drafts export as JSONL, a file imports back in one transaction, a Strapi export
// entity unwraps to its `data`, and a bad file is refused where it fails, landing nothing.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch('/server/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'Transfer testbed' })
		});
		if (!response.ok) throw new Error(`setup: project: HTTP ${response.status} ${await response.text()}`);
		return ((await response.json()) as { data: { id: string } }).data.id;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('a collection exports its drafts and imports every file shape', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/transfer');
	await page.getByLabel('Project').selectOption(projectId);

	// Two entries through the import; the export already answers them back.
	await page.getByRole('button', { name: 'Set up notes and import two' }).click();
	await expect(page.getByTestId('message')).toHaveText('set up');
	await expect(page.getByTestId('imported')).toHaveText('imported 2');
	await expect(page.getByTestId('export-count')).toHaveText('2');
	await expect(page.getByTestId('export-body')).toContainText('"title":"First"');
	await expect(page.getByTestId('export-body')).toContainText('"title":"Second"');

	// A Strapi export entity line unwraps to its `data` and imports as a third entry.
	await page.getByRole('button', { name: 'Import a Strapi entity' }).click();
	await expect(page.getByTestId('strapi-result')).toContainText('201');
	await expect(page.getByTestId('export-count')).toHaveText('3');
	await expect(page.getByTestId('export-body')).toContainText('"title":"From Strapi"');

	// A file whose second line fails is refused where it fails — and lands nothing.
	await page.getByRole('button', { name: 'Import a bad file' }).click();
	await expect(page.getByTestId('refused')).toContainText('422');
	await expect(page.getByTestId('refused')).toContainText('missing_field at /line/2/title');
	await expect(page.getByTestId('export-count')).toHaveText('3');
});
