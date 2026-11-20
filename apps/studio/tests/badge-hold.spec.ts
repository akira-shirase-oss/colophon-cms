// The editor's state badge says where the entry stands only once it knows: an entry waiting in a
// publish request reads "In review", not "Live", and that depends on the request's own read. Until
// the requests, the schedules and each waiting request's read have settled, the badge holds in the
// kit's loading treatment. A slow read once showed "Live" for an entry that was in fact waiting for
// review, which is false, however briefly.

import { expect, test, type Page } from '@playwright/test';
import { CHANGESET, EDITOR_URL, serve, world } from './site-preview-world.ts';

declare global {
	interface Window {
		badgeStates: string[];
	}
}

/** Records every state the entry badge takes, from the first frame on, so a flash can't slip past. */
async function recordBadge(page: Page): Promise<void> {
	await page.addInitScript(() => {
		window.badgeStates = [];
		const note = () => {
			const badge = document.querySelector('[data-testid="entry-state"]');
			const state = badge?.getAttribute('data-state');
			if (state && window.badgeStates.at(-1) !== state) window.badgeStates.push(state);
		};
		new MutationObserver(note).observe(document, { subtree: true, childList: true, attributes: true, attributeFilter: ['data-state'] });
	});
}

test('the badge never says live while the request the entry waits in is still being read', async ({ page, baseURL }) => {
	let release = () => {};
	const w = world({ live: true, listed: true, request: 'awaiting_review', requestRead: new Promise<void>((done) => (release = done)) });
	await recordBadge(page);
	await serve(page, w, new URL(baseURL ?? '').origin);
	const read = page.waitForRequest((r) => r.method() === 'GET' && r.url().endsWith(`/changesets/${CHANGESET}`));
	await page.goto(EDITOR_URL);
	await read;

	// The request's read is held: the badge waits, and says it does to assistive technology.
	const badge = page.getByTestId('entry-state');
	await expect(badge).toHaveAttribute('data-state', 'checking');
	await expect(badge).toHaveAttribute('aria-busy', 'true');
	await expect(page.getByTestId('publish-state')).toContainText('Live in');

	release();
	await expect(badge).toHaveAttribute('data-state', 'review');
	expect(await page.evaluate(() => window.badgeStates)).not.toContain('live');
	expect(w.unanswered).toEqual([]);
});

test('with no request waiting, the badge settles on live', async ({ page, baseURL }) => {
	const w = world({ live: true, listed: false });
	await recordBadge(page);
	await serve(page, w, new URL(baseURL ?? '').origin);
	await page.goto(EDITOR_URL);
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'live');
	await expect(page.getByTestId('entry-state')).not.toHaveAttribute('aria-busy', 'true');
	expect(w.unanswered).toEqual([]);
});
