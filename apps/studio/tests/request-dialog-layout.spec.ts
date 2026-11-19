// The new-request dialog's rows keep an entry's state and its Publish | Unpublish choice apart: the
// longest state word ("Live with changes", with its icon) once ran into the toggle at 1440, because
// its column was narrower than the badge. Measured on the real layout, at a desktop and a phone width,
// for every state an entry can be in.

import { expect, type Locator, type Page, test } from '@playwright/test';
import { PROJECT, serve, world } from './site-preview-world.ts';

const ENTRIES = `/api/v1/projects/${PROJECT}/environments/production/collections/articles/entries`;

function entry(id: string, title: string, published: { revision_id: string; generation: number } | null) {
	const at = '2026-11-18T10:00:00Z';
	return {
		id,
		created_by: 'u-writer',
		updated_at: at,
		locale: null,
		published,
		revision: { id: `${id}-draft`, base_revision_id: null, author_id: 'u-writer', created_at: at, content_hash: 'sha256:0', title }
	};
}

async function box(locator: Locator) {
	const found = await locator.boundingBox();
	if (found === null) throw new Error('not laid out');
	return found;
}

const apart = (a: { x: number; y: number; width: number; height: number }, b: typeof a) =>
	a.x + a.width <= b.x || b.x + b.width <= a.x || a.y + a.height <= b.y || b.y + b.height <= a.y;

async function openDialog(page: Page, baseURL: string | undefined) {
	await serve(page, world(), new URL(baseURL ?? '').origin);
	await page.route(
		(url) => url.pathname === ENTRIES,
		(route) =>
			route.fulfill({
				json: {
					data: [
						entry('e-changed', 'An entry readers have, edited since', { revision_id: 'e-changed-live', generation: 12 }),
						entry('e-live', 'An entry readers have as drafted', { revision_id: 'e-live-draft', generation: 12 }),
						entry('e-draft', 'A draft nobody has seen', null)
					],
					next_cursor: null
				}
			})
	);
	await page.goto(`/studio/projects/${PROJECT}/changesets`);
	await page.getByRole('button', { name: 'New publish request' }).click();
	const rows = page.getByTestId('new-request').getByTestId('request-entry');
	await expect(rows).toHaveCount(3);
	return rows;
}

for (const [width, height] of [
	[1440, 900],
	[390, 844]
] as const) {
	test(`a row's state badge and its Publish | Unpublish choice never overlap: ${width}px`, async ({ page, baseURL }) => {
		await page.setViewportSize({ width, height });
		await page.emulateMedia({ reducedMotion: 'reduce' });
		const rows = await openDialog(page, baseURL);

		const changed = rows.filter({ has: page.locator('[data-state="changed"]:visible') });
		await expect(changed.locator('[data-state="changed"]:visible')).toHaveText('Live with changes');

		for (const row of await rows.all()) {
			const badge = row.locator('[data-slot="status-badge"]:visible');
			await expect(badge).toHaveCount(1);
			const badgeBox = await box(badge);
			// The badge stays inside the dialog's row, whole.
			const rowBox = await box(row);
			expect(badgeBox.x + badgeBox.width, 'badge inside its row').toBeLessThanOrEqual(rowBox.x + rowBox.width + 0.5);
			const toggle = row.locator('[data-testid="request-entry-kind"]:visible');
			if ((await toggle.count()) === 0) continue; // a draft has nothing to unpublish
			expect(apart(badgeBox, await box(toggle)), `${await row.getAttribute('data-entry')}: badge clear of the toggle`).toBe(true);
		}
	});
}
