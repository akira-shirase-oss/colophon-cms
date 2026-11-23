// The request page names what built a request (D-097, D-184), against the mocked API: an undo of
// release N also carries `rollback_generation` N - 1, so the page must read `undo_of` first and
// say "Undo of Release N", linking to release N; a rollback says "Rollback to Release N"; an
// ordinary request says neither.

import { expect, test } from '@playwright/test';
import { CHANGESET_URL, PROJECT, serve, world } from './site-preview-world.ts';

for (const [label, overrides, text, target] of [
	['an undo of release 3', { undoOf: 3, rollbackGeneration: 2 }, 'Undo of Release 3', 3],
	['a rollback to release 2', { undoOf: null, rollbackGeneration: 2 }, 'Rollback to Release 2', 2]
] as const) {
	test(`the request page names ${label}, linking to that release`, async ({ page, baseURL }) => {
		const w = world(overrides);
		await serve(page, w, new URL(baseURL ?? '').origin);
		await page.goto(CHANGESET_URL);
		await expect(page.getByRole('heading', { level: 1 })).toHaveText('Spring issue');
		const origin = page.getByTestId('restores-link');
		await expect(origin).toHaveText(text);
		await expect(origin).toHaveAttribute('href', `/studio/projects/${PROJECT}/releases/${target}`);
		expect(w.unanswered).toEqual([]);
	});
}

test('an ordinary request names no origin', async ({ page, baseURL }) => {
	const w = world();
	await serve(page, w, new URL(baseURL ?? '').origin);
	await page.goto(CHANGESET_URL);
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Spring issue');
	await expect(page.getByTestId('restores-link')).toHaveCount(0);
});
