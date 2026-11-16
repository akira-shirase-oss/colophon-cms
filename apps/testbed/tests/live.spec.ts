// Live lists through the testbed, in a real browser (E3, D-108): the page holds a
// {P}/events stream through the /server proxy, and an ordinary write's commit arrives as a
// `change` hint in the visible event log, timed against the refetch it caused.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function setup(page: Page): Promise<{ project: string }> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const json = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
		const created = await fetch('/server/api/v1/projects', { method: 'POST', headers: json, body: JSON.stringify({ slug, name: 'Live testbed' }) });
		const project = ((await created.json()) as { data: { id: string } }).data.id;
		const body = { title: 'Note under review', body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } };
		await fetch(`/server/api/v1/projects/${project}/environments/production/collections/articles/entries`, {
			method: 'POST',
			headers: { ...json, 'Idempotency-Key': crypto.randomUUID() },
			body: JSON.stringify(body)
		});
		return { project };
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('a committed write arrives as a change hint, and the log times the refetch it caused', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const { project } = await setup(page);
	await page.goto('/live');
	await page.getByLabel('Project').selectOption(project);
	await expect(page.getByTestId('stream-state')).toHaveText('stream: live');

	// An entry write the page itself makes commits; the stream says so within moments, the
	// log row times the refetch the hint caused, and the list it refetched names the entry.
	await page.getByTestId('create-entry').click();
	await expect(page.getByTestId('message')).toHaveText('entry 1 created');
	const entryHint = page.locator('[data-testid="event"][data-event="change"]').filter({ hasText: '"k":"entries"' }).first();
	await expect(entryHint).toBeVisible();
	await expect(entryHint.getByTestId('latency')).toContainText('refreshed in');
	await expect(page.getByTestId('entries')).toContainText('Live note 1');

	// A comment commits under the entry's scope; its hint refetches the comment count.
	await page.getByTestId('add-comment').click();
	await expect(page.getByTestId('message')).toHaveText('comment 2 added');
	const commentHint = page.locator('[data-testid="event"][data-event="change"]').filter({ hasText: '"k":"comments"' }).first();
	await expect(commentHint).toBeVisible();
	await expect(commentHint.getByTestId('latency')).toContainText('refreshed in');
	await expect(page.getByTestId('comments-count')).toContainText('1 comment');
});
