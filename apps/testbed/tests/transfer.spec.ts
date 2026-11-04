// Collection transfer (D-086, D-114) through the testbed, in a real browser, against the
// real server: drafts export as JSONL, a file imports back in one transaction, a Strapi
// export entity unwraps to its `data`, a bad file is refused where it fails, landing
// nothing, and `?dry_run=true` reports each item's plan without writing.

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

test('a dry run reports each item, Sanity imports, and a source id updates', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/transfer');
	await page.getByLabel('Project').selectOption(projectId);

	await page.getByRole('button', { name: 'Set up notes and import two' }).click();
	await expect(page.getByTestId('message')).toHaveText('set up');
	await expect(page.getByTestId('export-count')).toHaveText('2');

	// `?dry_run=true` answers the per-item plan — would_create, invalid, skipped — and the
	// export still holds only the two entries: the plan wrote nothing (D-114).
	await page.getByRole('button', { name: 'Dry-run a mixed file' }).click();
	await expect(page.getByTestId('outcome-label')).toHaveText('dry run');
	const rows = page.getByTestId('outcome');
	await expect(rows).toHaveCount(3);
	await expect(rows.nth(0)).toHaveAttribute('data-status', 'would_create');
	await expect(rows.nth(0)).toContainText('/line/1');
	await expect(rows.nth(1)).toHaveAttribute('data-status', 'invalid');
	await expect(rows.nth(1)).toContainText('missing_field at /line/2/title');
	await expect(rows.nth(2)).toHaveAttribute('data-status', 'skipped');
	await expect(page.getByTestId('export-count')).toHaveText('2');

	// A Sanity NDJSON line maps `_id`/`_type` and reports the metadata it set aside.
	await page.getByRole('button', { name: 'Import a Sanity document' }).click();
	await expect(page.getByTestId('outcome-label')).toContainText('sanity: 201');
	await expect(rows.nth(0)).toHaveAttribute('data-status', 'created');
	await expect(rows.nth(0)).toContainText('source drafts.s7');
	await expect(rows.nth(0)).toContainText('unmapped _createdAt, _rev');
	await expect(page.getByTestId('export-count')).toHaveText('3');

	// A source id that names an entry is a save onto its draft — `updated`, not a second
	// entry.
	await page.getByRole('button', { name: 'Update by source id' }).click();
	await expect(page.getByTestId('outcome-label')).toContainText('update: 201');
	await expect(rows.nth(0)).toHaveAttribute('data-status', 'updated');
	await expect(page.getByTestId('export-count')).toHaveText('3');
	await expect(page.getByTestId('export-body')).toContainText('"title":"Renamed"');
});
