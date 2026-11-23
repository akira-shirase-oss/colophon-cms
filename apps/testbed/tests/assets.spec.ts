// Uploads through the testbed, in a real browser, against the real server and its object store:
// a real PNG is quarantined, verified and becomes usable with its proved type and dimensions;
// HTML disguised as an image is rejected. Deleting is staged: 202 requests it, and the row
// stays, reading `deleted`.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('__Host-colophon_csrf='))?.slice('__Host-colophon_csrf='.length) ?? '';
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

	// M.3: a wide upload earns a w320 rendition, served under its own name.
	await page.getByTestId('upload-wide').click();
	await expect(page.getByTestId('message')).toHaveText('settled', { timeout: 20_000 });
	await expect(page.getByTestId('steps')).toContainText('rendition w320: 200 image/png');
});

test('a usable upload can be deleted, and its tombstone stays', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/assets');
	await page.getByTestId('project').selectOption(projectId);

	await page.getByTestId('upload-real').click();
	await expect(page.getByTestId('message')).toHaveText('settled', { timeout: 20_000 });
	const row = page.locator('tr', { hasText: 'dot.png' });
	await expect(row).toContainText('usable');

	// 202 is "requested", not gone: the row's state lands `deleted` and the row stays.
	await row.getByTestId('asset-delete').click();
	await expect(page.getByTestId('removed')).toHaveText('dot.png: deleted', { timeout: 20_000 });
	await expect(page.getByTestId('steps')).toContainText('delete dot.png: 202 requested');
	await expect(row).toContainText('deleted');
	await expect(row.getByTestId('asset-delete')).toHaveCount(0);
});

// The library (D2): keyset paging over a cursor, search by name and by alt text, metadata changed
// under If-Match — a stale version refused, never a lost update — and a tag as a filter.
test('the library pages, searches, and keeps metadata edits under If-Match', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/assets');
	await page.getByTestId('project').selectOption(projectId);

	await page.getByTestId('library-run').click();
	const steps = page.getByTestId('library-steps');
	await expect(steps).toContainText('read back:', { timeout: 60_000 });
	await expect(steps).toContainText('uploaded: 3');
	// Newest first, two at a time; the cursor carries the rest.
	await expect(steps).toContainText('page 1: 200, last-dot.png, wide-strip.png, more');
	await expect(steps).toContainText('page 2: 200, first-dot.png, end');
	await expect(steps).toContainText('search "WIDE": wide-strip.png');
	await expect(steps).toContainText('metadata: 200, version moved on, tags testbed dots');
	await expect(steps.getByText(/^stale metadata:/)).toContainText('409');
	await expect(steps.getByText(/^stale metadata:/)).toContainText('stale_version');
	await expect(steps.getByText(/^bad tag:/)).toContainText('422');
	await expect(steps.getByText(/^bad tag:/)).toContainText('/tags/0');
	await expect(steps).toContainText('search "dot of ink": last-dot.png');
	await expect(steps).toContainText('tag "dots": last-dot.png');
	await expect(steps).toContainText('read back: Dot / A single dot of ink / The testbed');
});
