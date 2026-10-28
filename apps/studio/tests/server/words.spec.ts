// Plain words (D-059, rule 1): outside Settings and the Technical details drawer, no screen shows
// or reads out an internal term (changeset, revision, slug, destination, webhook, probe,
// generation, pinned, hash…; `INTERNAL_TERMS` in src/lib/words.ts). The drawer still has them, for
// developers. The first-run journey checks its own screens the same way (journey.spec.ts).

import { expect, type Page, test } from '@playwright/test';
import { api, env, internalTerms, key, member, newEntry, newProject, signIn, violations } from './helpers.ts';

async function plain(page: Page, screen: string) {
	await page.waitForLoadState('networkidle');
	expect(await internalTerms(page), screen).toEqual([]);
}

test('every everyday screen speaks plainly; the Technical details drawer keeps the internal names', async ({ page }) => {
	await signIn(page);
	await plain(page, 'projects');
	const project = await newProject(page);
	const base = `/studio/projects/${project}`;

	await page.goto(base);
	await expect(page.getByTestId('first-run')).toBeVisible();
	await plain(page, 'overview, empty');
	await page.goto(`${base}/articles`);
	await expect(page.getByTestId('entries-empty')).toBeVisible();
	await plain(page, 'articles, empty');
	await page.goto(`${base}/changesets`);
	await expect(page.getByTestId('requests-empty')).toBeVisible();
	await plain(page, 'publish requests, empty');
	await page.goto(`${base}/releases`);
	await expect(page.getByTestId('releases-empty')).toBeVisible();
	await plain(page, 'releases, empty');
	await page.goto(`${base}/members`);
	await expect(page.getByTestId('members-alone')).toBeVisible();
	await plain(page, 'members, only you');

	await page.goto(`${base}/articles?new=1`);
	await page.getByLabel('Title', { exact: true }).fill('Plain words');
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await plain(page, 'editor');
	await page.getByRole('tab', { name: /History/ }).click();
	await plain(page, 'editor, history');
	// The drawer has the internal names, closed until asked for.
	const drawer = page.getByTestId('technical-details');
	await drawer.getByText('Technical details').click();
	await expect(drawer).toContainText('Revision');
	await expect(drawer).toContainText('Content hash');

	await page.getByTestId('publish-button').click();
	await expect(page.getByTestId('publish-outcome')).toContainText('Live as Release');
	await plain(page, 'editor, published');
	await page.getByTestId('publish-outcome').getByRole('link').click();
	await expect(page.getByTestId('receipt-label')).toBeVisible();
	await plain(page, 'receipt');
	await page.getByRole('link', { name: 'The publish request' }).click();
	await expect(page.getByTestId('changeset-status')).toHaveAttribute('data-state', 'published');
	// One click approved it as its sender: allowed (D-044), and said.
	await expect(page.getByTestId('self-review-note')).toContainText('their own request');
	await plain(page, 'publish request');
	await page.getByTestId('changesets-link').click();
	await expect(page.getByTestId('changesets')).toContainText('Plain words');
	await plain(page, 'publish requests');
	await page.getByTestId('releases-link').click();
	await expect(page.getByTestId('releases')).toContainText('Plain words');
	await plain(page, 'releases');
	await page.goto(base);
	await expect(page.getByTestId('first-run-count')).toContainText('done');
	await plain(page, 'overview');
	await page.keyboard.press('ControlOrMeta+k');
	await expect(page.getByRole('dialog')).toBeVisible();
	await plain(page, 'search');
	await page.keyboard.press('Escape');

	// Settings is the developer area, where the internal names stay next to the plain ones; the
	// check sees them there, so it would see them anywhere.
	await page.goto(`${base}/settings`);
	await expect(page.getByRole('heading', { name: 'Website connections' })).toBeVisible();
	expect((await internalTerms(page)).join('\n')).toMatch(/webhook/i);
});

// Refusals in plain words (D-059, rule 3). The server's messages for these two say "changeset"
// and "pinned"; the Studio once showed them as they came.

test('a review refused because an entry was edited meanwhile says what happened and what to do, plainly', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Edited meanwhile');
	const made = await api(page, 'POST', `${env(project)}/changesets`, { title: 'Edited meanwhile', entries: [entry] }, { 'Idempotency-Key': key() });
	const set = made.body?.['data'] as { id: string; hash: string };
	expect((await api(page, 'POST', `${env(project)}/changesets/${set.id}/submit`, undefined, { 'If-Match': `"${set.hash}"` })).status).toBe(200);
	await page.goto(`/studio/projects/${project}/changesets/${set.id}`);
	await expect(page.getByTestId('changeset-status')).toHaveAttribute('data-state', 'awaiting_review');

	// Someone edits the entry while the reviewer has the request open.
	const saved = await page.evaluate(
		async ({ path }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const current = await fetch(path);
			const { data } = (await current.json()) as { data: { content: Record<string, unknown> } };
			const response = await fetch(path, {
				method: 'PUT',
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, 'If-Match': current.headers.get('etag') ?? '', 'Idempotency-Key': crypto.randomUUID().replaceAll('-', '') },
				body: JSON.stringify({ ...data.content, title: 'Edited meanwhile, again' })
			});
			return response.status;
		},
		{ path: `${env(project)}/collections/articles/entries/${entry}` }
	);
	expect(saved).toBe(200);

	await page.getByRole('button', { name: 'Approve this version' }).click();
	const said = page.getByRole('alert').filter({ hasText: 'edited after' });
	await expect(said).toContainText('An entry in it was edited after the publish request was made. Update it to the latest versions, then review it again.');
	await plain(page, 'publish request, review refused');
	expect(await violations(page)).toEqual([]);
});

test('a request refused because the role changed meanwhile says so, plainly', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	const roles = `/api/v1/projects/${project}/roles`;
	const read = { action: 'content.read', subject: 'articles' };
	expect((await api(page, 'POST', roles, { key: 'writer', name: 'Writer', permissions: [read, { action: 'content.create', subject: 'articles' }, { action: 'content.update', subject: 'articles' }] })).status).toBe(201);
	expect((await api(page, 'POST', roles, { key: 'reader', name: 'Reader', permissions: [read] })).status).toBe(201);

	const writer = await member(page, browser, project, 'writer');
	await writer.goto(`/studio/projects/${project}/articles?new=1`);
	await writer.getByLabel('Title', { exact: true }).fill('Asked too late');
	await writer.getByRole('button', { name: 'Create', exact: true }).click();
	await expect(writer.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await expect(writer.getByTestId('publish-button')).toHaveText('Ask for review');

	// An owner narrows their role while the editor is open.
	const people = await api(page, 'GET', `/api/v1/projects/${project}/members`);
	const them = (people.body?.['data'] as { identity_id: string; role: string }[]).find((person) => person.role === 'writer');
	expect((await api(page, 'PUT', `/api/v1/projects/${project}/members/${them?.identity_id}`, { role: 'reader' })).status).toBe(204);

	await writer.getByTestId('publish-button').click();
	await expect(writer.getByTestId('publish-outcome')).toContainText("Your role in this project can't ask for review here");
	await plain(writer, 'editor, request refused');
	await writer.context().close();
});
