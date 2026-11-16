// Publishing through the testbed, in a real browser (D-045): release an approved changeset on
// generation 0, see a second one on the same stale base refused, release it on the right base,
// and read /content/v1 with no cookie, at the latest release and pinned to the first.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

/** A project with two approved changesets of one article each; the project id. */
async function setup(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const json = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
		// Every setup request must succeed: a refusal here is reported with the server's answer,
		// not found a minute later as a choice missing from a list.
		async function ok(response: Promise<Response>, what: string): Promise<Response> {
			const answer = await response;
			if (!answer.ok) throw new Error(`setup: ${what}: HTTP ${answer.status} ${await answer.text()}`);
			return answer;
		}
		const created = await ok(fetch('/server/api/v1/projects', { method: 'POST', headers: json, body: JSON.stringify({ slug, name: 'Releases testbed' }) }), 'project');
		const project = ((await created.json()) as { data: { id: string } }).data.id;
		const base = `/server/api/v1/projects/${project}/environments/production`;
		for (const title of ['First story', 'Second story']) {
			const body = { title, body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } };
			const entry = await ok(fetch(`${base}/collections/articles/entries`, { method: 'POST', headers: { ...json, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify(body) }), `${title}: entry`);
			const entryId = ((await entry.json()) as { data: { id: string } }).data.id;
			const made = await ok(fetch(`${base}/changesets`, { method: 'POST', headers: { ...json, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify({ title, entries: [entryId] }) }), `${title}: changeset`);
			const set = ((await made.json()) as { data: { id: string; hash: string } }).data;
			const ifMatch = { ...json, 'If-Match': `"${set.hash}"` };
			await ok(fetch(`${base}/changesets/${set.id}/submit`, { method: 'POST', headers: ifMatch }), `${title}: submit`);
			await ok(fetch(`${base}/changesets/${set.id}/reviews`, { method: 'POST', headers: ifMatch, body: JSON.stringify({ decision: 'approve' }) }), `${title}: approve`);
		}
		return project;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('release on a base, refuse a stale base, and read releases with no cookie', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const project = await setup(page);
	await page.goto('/releases');
	await page.getByLabel('Project').selectOption(project);
	await expect(page.getByTestId('generation')).toHaveText('0');

	await page.locator('select[name="changeset"]').selectOption({ label: 'First story (approved)' });
	await page.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('published as generation 1');
	await expect(page.getByTestId('generation')).toHaveText('1');

	await page.locator('select[name="changeset"]').selectOption({ label: 'Second story (approved)' });
	await page.getByLabel('Base generation').fill('0');
	await page.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('409 conflict (generation_moved)');
	await page.locator('select[name="changeset"]').selectOption({ label: 'Second story (approved)' });
	await page.getByLabel('Base generation').fill('1');
	await page.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('published as generation 2');
	await expect(page.getByTestId('releases').getByRole('listitem')).toHaveCount(2);

	await page.getByRole('button', { name: 'Read published articles' }).click();
	await expect(page.getByTestId('delivered')).toHaveText('HTTP 200, generation 2: Second story, First story');
	await page.getByLabel('Read at generation').fill('1');
	await page.getByRole('button', { name: 'Read published articles' }).click();
	await expect(page.getByTestId('delivered')).toHaveText('HTTP 200, generation 1: First story');
});

/** A project with "Time traveller" approved as "First release"; the project and article ids. */
async function rollbackSetup(page: Page): Promise<{ project: string; first: string }> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const json = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
		async function ok(response: Promise<Response>, what: string): Promise<Response> {
			const answer = await response;
			if (!answer.ok) throw new Error(`setup: ${what}: HTTP ${answer.status} ${await answer.text()}`);
			return answer;
		}
		const article = (title: string) => ({ title, body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } });
		const created = await ok(fetch('/server/api/v1/projects', { method: 'POST', headers: json, body: JSON.stringify({ slug, name: 'Rollback testbed' }) }), 'project');
		const project = ((await created.json()) as { data: { id: string } }).data.id;
		const base = `/server/api/v1/projects/${project}/environments/production`;
		const entry = await ok(fetch(`${base}/collections/articles/entries`, { method: 'POST', headers: { ...json, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify(article('Time traveller')) }), 'entry');
		const first = ((await entry.json()) as { data: { id: string } }).data.id;
		const made = await ok(fetch(`${base}/changesets`, { method: 'POST', headers: { ...json, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify({ title: 'First release', entries: [first] }) }), 'changeset');
		const set = ((await made.json()) as { data: { id: string; hash: string } }).data;
		const ifMatch = { ...json, 'If-Match': `"${set.hash}"` };
		await ok(fetch(`${base}/changesets/${set.id}/submit`, { method: 'POST', headers: ifMatch }), 'submit');
		await ok(fetch(`${base}/changesets/${set.id}/reviews`, { method: 'POST', headers: ifMatch, body: JSON.stringify({ decision: 'approve' }) }), 'approve');
		return { project, first };
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

/**
 * What the second release changes, made after the first is out: the article is retitled (a new
 * draft to pin) and a second article exists — both approved as "Second release". The second
 * article's id, for the reads that must find it gone.
 */
async function secondRelease(page: Page, project: string, first: string): Promise<string> {
	return page.evaluate(
		async ({ project, first }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const json = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
			async function ok(response: Promise<Response>, what: string): Promise<Response> {
				const answer = await response;
				if (!answer.ok) throw new Error(`setup: ${what}: HTTP ${answer.status} ${await answer.text()}`);
				return answer;
			}
			const article = (title: string) => ({ title, body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } });
			const base = `/server/api/v1/projects/${project}/environments/production`;
			const current = await ok(fetch(`${base}/collections/articles/entries/${first}`), 'read draft');
			const saved = await ok(
				fetch(`${base}/collections/articles/entries/${first}`, {
					method: 'PUT',
					headers: { ...json, 'If-Match': current.headers.get('etag') ?? '', 'Idempotency-Key': crypto.randomUUID() },
					body: JSON.stringify(article('Time traveller returns'))
				}),
				'retitle'
			);
			await saved.text();
			const entry = await ok(fetch(`${base}/collections/articles/entries`, { method: 'POST', headers: { ...json, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify(article('Farewell')) }), 'second entry');
			const gone = ((await entry.json()) as { data: { id: string } }).data.id;
			const made = await ok(fetch(`${base}/changesets`, { method: 'POST', headers: { ...json, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify({ title: 'Second release', entries: [first, gone] }) }), 'changeset');
			const set = ((await made.json()) as { data: { id: string; hash: string } }).data;
			const ifMatch = { ...json, 'If-Match': `"${set.hash}"` };
			await ok(fetch(`${base}/changesets/${set.id}/submit`, { method: 'POST', headers: ifMatch }), 'submit');
			await ok(fetch(`${base}/changesets/${set.id}/reviews`, { method: 'POST', headers: ifMatch, body: JSON.stringify({ decision: 'approve' }) }), 'approve');
			return gone;
		},
		{ project, first }
	);
}

// Rolling back (D-097): POST releases/{n}/rollback builds an ordinary changeset restoring what
// that release published, refused for the current or a missing generation and when nothing would
// change. The restore is submitted, approved and published like any other changeset, and the
// generation still moves forward. Pinned reads keep serving what is still deliverable at their
// generation: the retitled article's earlier and later revisions both stay readable — but what
// the restore took down is revoked, and a revoked article is gone at every generation (D-049).
test('roll back to an earlier release through an ordinary changeset', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const { project, first } = await rollbackSetup(page);
	await page.goto('/releases');
	await page.getByLabel('Project').selectOption(project);

	// Before the first release there is nothing to restore.
	await page.getByLabel('Roll back to generation').fill('1');
	await page.getByLabel('Restore changeset title').fill('Too soon');
	await page.getByRole('button', { name: 'Roll back to this release' }).click();
	await expect(page.getByTestId('restored')).toHaveText('422 validation_failed (invalid_generation)');

	await page.locator('select[name="changeset"]').selectOption({ label: 'First release (approved)' });
	await page.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('published as generation 1');
	const gone = await secondRelease(page, project, first);
	// The page's changeset list predates "Second release"; reload to see it.
	await page.goto('/releases');
	await page.getByLabel('Project').selectOption(project);
	await page.locator('select[name="changeset"]').selectOption({ label: 'Second release (approved)' });
	await page.getByLabel('Base generation').fill('1');
	await page.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('published as generation 2');

	// Generation 0 and the live release cannot be restored.
	await page.getByLabel('Restore changeset title').fill('Nope');
	for (const at of ['0', '2']) {
		await page.getByLabel('Roll back to generation').fill(at);
		await page.getByRole('button', { name: 'Roll back to this release' }).click();
		await expect(page.getByTestId('restored')).toHaveText('422 validation_failed (invalid_generation)');
	}

	// Rolling back to 1 builds an open changeset; the Changesets page opens it like any other:
	// "Time traveller" back at its first revision, "Farewell" taken down.
	await page.getByLabel('Roll back to generation').fill('1');
	await page.getByLabel('Restore changeset title').fill('Back to one');
	await page.getByRole('button', { name: 'Roll back to this release' }).click();
	await expect(page.getByTestId('restored')).toContainText('(open)');

	await page.goto('/changesets');
	await page.getByLabel('Project').selectOption(project);
	await expect(page.locator('select[name="open-changeset"] option')).toHaveCount(4);
	await page.locator('select[name="open-changeset"]').selectOption({ label: 'Back to one (open)' });
	await expect(page.getByTestId('state')).toHaveText('open');
	await expect(page.getByTestId('changeset')).toContainText('Farewell (unpublish)');
	await expect(page.getByTestId('changeset')).toContainText('Time traveller');
	await expect(page.getByTestId('changeset').getByRole('listitem')).toHaveCount(2);
	await page.getByRole('button', { name: 'Submit', exact: true }).click();
	await page.getByRole('button', { name: 'Approve', exact: true }).click();
	await expect(page.getByTestId('state')).toHaveText('approved');

	await page.goto('/releases');
	await page.getByLabel('Project').selectOption(project);
	await page.locator('select[name="changeset"]').selectOption({ label: 'Back to one (approved)' });
	await page.getByLabel('Base generation').fill('2');
	await page.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('published as generation 3');

	// Generation 3 serves what generation 1 served; at generation 2 the article's newer revision
	// is still delivered — but "Farewell" is gone even there: a revocation is for all history.
	await page.getByRole('button', { name: 'Read published articles' }).click();
	await expect(page.getByTestId('delivered')).toHaveText('HTTP 200, generation 3: Time traveller');
	await page.getByLabel('Read at generation').fill('1');
	await page.getByRole('button', { name: 'Read published articles' }).click();
	await expect(page.getByTestId('delivered')).toHaveText('HTTP 200, generation 1: Time traveller');
	await page.getByLabel('Read at generation').fill('2');
	await page.getByRole('button', { name: 'Read published articles' }).click();
	await expect(page.getByTestId('delivered')).toHaveText('HTTP 200, generation 2: Time traveller returns');
	await page.getByLabel("One article's id (empty for the list)").fill(gone);
	for (const at of ['', '1', '2']) {
		await page.getByLabel('Read at generation').fill(at);
		await page.getByRole('button', { name: 'Read published articles' }).click();
		await expect(page.getByTestId('delivered')).toHaveText('HTTP 410, gone');
	}

	// Restoring what is already live builds nothing.
	await page.getByLabel("One article's id (empty for the list)").fill('');
	await page.getByLabel('Restore changeset title').fill('Again');
	await page.getByLabel('Roll back to generation').fill('1');
	await page.getByRole('button', { name: 'Roll back to this release' }).click();
	await expect(page.getByTestId('restored')).toHaveText('422 validation_failed (already_at)');
});

/** A project with one approved changeset of a published-to-be article and one draft; ids. */
async function unpublishSetup(page: Page): Promise<{ project: string; live: string }> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const json = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
		const created = await fetch('/server/api/v1/projects', { method: 'POST', headers: json, body: JSON.stringify({ slug, name: 'Unpublish testbed' }) });
		const project = ((await created.json()) as { data: { id: string } }).data.id;
		const base = `/server/api/v1/projects/${project}/environments/production`;
		const ids: string[] = [];
		for (const title of ['Gone soon', 'Only a draft']) {
			const body = { title, body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } };
			const entry = await fetch(`${base}/collections/articles/entries`, { method: 'POST', headers: { ...json, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify(body) });
			ids.push(((await entry.json()) as { data: { id: string } }).data.id);
		}
		const made = await fetch(`${base}/changesets`, { method: 'POST', headers: { ...json, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify({ title: 'Launch', entries: [ids[0]] }) });
		const set = ((await made.json()) as { data: { id: string; hash: string } }).data;
		const ifMatch = { ...json, 'If-Match': `"${set.hash}"` };
		await fetch(`${base}/changesets/${set.id}/submit`, { method: 'POST', headers: ifMatch });
		await fetch(`${base}/changesets/${set.id}/reviews`, { method: 'POST', headers: ifMatch, body: JSON.stringify({ decision: 'approve' }) });
		return { project, live: ids[0] ?? '' };
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('refuse to unpublish a draft, take a published article down, and read it as gone at every release', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const { project, live } = await unpublishSetup(page);
	await page.goto('/releases');
	await page.getByLabel('Project').selectOption(project);
	await page.locator('select[name="changeset"]').selectOption({ label: 'Launch (approved)' });
	await page.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('published as generation 1');

	await page.goto('/changesets');
	await page.getByLabel('Project').selectOption(project);
	await page.getByLabel('Changeset title').fill('Take it down');
	await page.getByLabel('Unpublish Only a draft').check();
	await page.getByRole('button', { name: 'Create changeset' }).click();
	await expect(page.getByTestId('message')).toHaveText('422 validation_failed (not_published)');
	await page.getByLabel('Unpublish Only a draft').uncheck();
	await page.getByLabel('Gone soon', { exact: true }).check();
	await page.getByLabel('Unpublish Gone soon').check();
	await page.getByRole('button', { name: 'Create changeset' }).click();
	await expect(page.getByTestId('message')).toHaveText('422 validation_failed (invalid_entries)');
	await page.getByLabel('Gone soon', { exact: true }).uncheck();
	await page.getByRole('button', { name: 'Create changeset' }).click();
	await expect(page.getByTestId('state')).toHaveText('open');
	await expect(page.getByTestId('changeset')).toContainText('Gone soon (unpublish)');
	await page.getByRole('button', { name: 'Submit', exact: true }).click();
	await page.getByRole('button', { name: 'Approve', exact: true }).click();
	await expect(page.getByTestId('state')).toHaveText('approved');

	await page.goto('/releases');
	await page.getByLabel('Project').selectOption(project);
	await page.locator('select[name="changeset"]').selectOption({ label: 'Take it down (approved)' });
	await page.getByLabel('Base generation').fill('1');
	await page.getByRole('button', { name: 'Publish', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('published as generation 2');
	await expect(page.getByTestId('releases')).toContainText('generation 2: Take it down (0 changed, 1 unpublished)');
	await page.getByRole('button', { name: 'Read published articles' }).click();
	await expect(page.getByTestId('delivered')).toHaveText('HTTP 200, generation 2: ');
	await page.getByLabel("One article's id (empty for the list)").fill(live);
	for (const at of ['', '1']) {
		await page.getByLabel('Read at generation').fill(at);
		await page.getByRole('button', { name: 'Read published articles' }).click();
		await expect(page.getByTestId('delivered')).toHaveText('HTTP 410, gone');
	}
});
