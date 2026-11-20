// The entry editor's next step for each role in each state (D-184): an owner (who publishes in one
// click), an editor (who asks for review) and a reader (a custom role that may only read). Each
// sees exactly what their role allows — a role without publish rights never sees Publish or
// Unpublish — and the server refuses the same steps when they are asked for directly, so hiding
// a button is never the only guard. States: draft, live with changes, live, in review,
// scheduled, taken down, a single type's entry and an entry with a translation.

import { expect, type Page, test } from '@playwright/test';
import { api, env, key, member, newEntry, newProject, publishThroughApi, signIn } from './helpers.ts';

function body(text: string) {
	return { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] } };
}

async function retitle(page: Page, project: string, collection: string, entry: string, title: string) {
	const path = `${env(project)}/collections/${collection}/entries/${entry}`;
	const current = await page.evaluate(async (path) => {
		const response = await fetch(path);
		return { etag: response.headers.get('etag') ?? '', content: ((await response.json()) as { data: { content: Record<string, unknown> } }).data.content };
	}, path);
	expect((await api(page, 'PUT', path, { ...current.content, title }, { 'If-Match': current.etag, 'Idempotency-Key': key() })).status).toBe(200);
}

/** A request for `entries`, submitted; approved too when `approve`. Its id and hash. */
async function request(page: Page, project: string, title: string, entries: string[], approve = false) {
	const made = await api(page, 'POST', `${env(project)}/changesets`, { title, entries }, { 'Idempotency-Key': key() });
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	const set = made.body?.['data'] as { id: string; hash: string };
	expect((await api(page, 'POST', `${env(project)}/changesets/${set.id}/submit`, undefined, { 'If-Match': `"${set.hash}"` })).status).toBe(200);
	if (approve) expect((await api(page, 'POST', `${env(project)}/changesets/${set.id}/reviews`, { decision: 'approve' }, { 'If-Match': `"${set.hash}"` })).status).toBe(200);
	return set;
}

interface State {
	name: string;
	collection: string;
	entry: string;
	badge: string;
	/** The one primary per role; null for none. */
	owner: string | null;
	editor: string | null;
	/** Whether Unpublish (or, for an editor, Ask to unpublish) is in ⋯ More. */
	takesDown: boolean;
	language: boolean;
}

async function world(page: Page): Promise<{ project: string; states: State[]; approved: { id: string; hash: string } }> {
	const project = await newProject(page, 'Roles');
	const article = (title: string) => ({ title, body: body(title) });
	const draft = await newEntry(page, project, '', 'articles', article('A draft'));
	const changed = await newEntry(page, project, '', 'articles', article('Live, then edited'));
	const live = await newEntry(page, project, '', 'articles', article('Live'));
	const review = await newEntry(page, project, '', 'articles', article('In review'));
	const scheduled = await newEntry(page, project, '', 'articles', article('Scheduled'));
	const down = await newEntry(page, project, '', 'articles', article('Taken down'));
	const made = await api(page, 'POST', `${env(project)}/collections/articles/entries?locale=en`, article('Translated'), { 'Idempotency-Key': key() });
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	const translated = made.body?.['data'].id as string;
	const french = await api(page, 'POST', `${env(project)}/collections/articles/entries/${translated}/localizations?locale=fr`, undefined, { 'Idempotency-Key': key() });
	expect(french.status, JSON.stringify(french.body)).toBe(201);
	const settings = { name: 'Site settings', title_field: 'title', single: true, fields: [{ id: 'title', label: 'Site name', type: 'text', required: true, max_chars: 200, multiline: false }] };
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'site-settings', definition: settings }, { 'Idempotency-Key': key() })).status).toBe(201);
	const single = await newEntry(page, project, '', 'site-settings', { title: 'Harbour Gazette' });

	await publishThroughApi(page, project, 'Everything live', [changed, live, down, translated]);
	await publishThroughApi(page, project, 'Down', [], [down]);
	await retitle(page, project, 'articles', changed, 'Live, then edited — again');
	await request(page, project, 'Waiting', [review]);
	const later = await request(page, project, 'Later', [scheduled], true);
	const tomorrow = new Date(Date.now() + 24 * 3600 * 1000).toISOString().slice(0, 10);
	const set = await api(page, 'POST', `${env(project)}/changesets/${later.id}/schedule`, { local_time: `${tomorrow}T09:00`, time_zone: 'UTC' }, { 'If-Match': `"${later.hash}"` });
	expect(set.status, JSON.stringify(set.body)).toBe(201);
	const approved = await request(page, project, 'Approved, for the refusals', [draft], true);
	return {
		project,
		approved,
		states: [
			{ name: 'draft', collection: 'articles', entry: draft, badge: 'draft', owner: 'Publish', editor: 'Ask for review', takesDown: false, language: true },
			{ name: 'changed', collection: 'articles', entry: changed, badge: 'changed', owner: 'Publish changes', editor: 'Ask for review', takesDown: true, language: true },
			{ name: 'live', collection: 'articles', entry: live, badge: 'live', owner: null, editor: null, takesDown: true, language: true },
			{ name: 'in review', collection: 'articles', entry: review, badge: 'review', owner: 'Review the request', editor: null, takesDown: false, language: true },
			{ name: 'scheduled', collection: 'articles', entry: scheduled, badge: 'scheduled', owner: 'See the schedule', editor: null, takesDown: false, language: true },
			{ name: 'taken down', collection: 'articles', entry: down, badge: 'taken_down', owner: 'Publish again', editor: 'Ask for review', takesDown: false, language: true },
			{ name: 'single type', collection: 'site-settings', entry: single, badge: 'draft', owner: 'Publish', editor: 'Ask for review', takesDown: false, language: false },
			{ name: 'translated', collection: 'articles', entry: translated, badge: 'live', owner: null, editor: null, takesDown: true, language: true }
		]
	};
}

/** The primary buttons a person can see (dialogs aside). */
async function primaries(page: Page): Promise<string[]> {
	return page
		.locator('[data-slot="button"][data-variant="default"]:visible')
		.evaluateAll((buttons) => buttons.filter((button) => button.closest('[role="dialog"]') === null).map((button) => button.textContent?.trim() ?? ''));
}

/** Makes a custom role that may only read, and moves `guest` (signed in on `as`) to it. */
async function makeReader(owner: Page, as: Page, project: string) {
	const role = await api(owner, 'POST', `/api/v1/projects/${project}/roles`, {
		key: 'reader',
		name: 'Reader',
		permissions: [
			{ action: 'content.read', subject: 'articles' },
			{ action: 'content.read', subject: 'site-settings' }
		]
	});
	expect(role.status, JSON.stringify(role.body)).toBe(201);
	const me = (await api(as, 'GET', '/me')).body?.['data'].id as string;
	const moved = await api(owner, 'PUT', `/api/v1/projects/${project}/members/${me}`, { role: 'reader' });
	expect(moved.status, JSON.stringify(moved.body)).toBeLessThan(300);
}

test('each role sees only what it may do, in every state, and the server refuses the rest', async ({ page, browser }) => {
	test.setTimeout(300_000);
	await signIn(page);
	const made = await world(page);
	const editor = await member(page, browser, made.project, 'editor', 'dev-editor');
	const people: [string, Page][] = [['owner', page], ['editor', editor]];

	for (const [role, viewer] of people) {
		for (const state of made.states) {
			await viewer.goto(`/studio/projects/${made.project}/${state.collection}/${state.entry}`);
			await expect(viewer.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
			await viewer.waitForLoadState('networkidle');
			const where = `${role}, ${state.name}`;
			await expect.soft(viewer.getByTestId('entry-state'), where).toHaveAttribute('data-state', state.badge);
			const expected = role === 'owner' ? state.owner : state.editor;
			// The role read has arrived (the page no longer says it is checking): before it, no primary
			// shows for anyone, and an expected "none" would pass on that frame.
			await expect(viewer.getByTestId('publish-entry')).not.toContainText('Checking what your role may do');
			expect.soft(await primaries(viewer), where).toEqual(expected === null ? [] : [expected]);
			await viewer.getByTestId('more-menu').click();
			// The menu is open (its always-there item shows) before its takedown item is judged, so an
			// absent item means absent, not "not drawn yet".
			await expect(viewer.getByTestId('technical-menu')).toBeVisible();
			const takedown = viewer.getByRole('menuitem').filter({ hasText: /unpublish/i });
			await expect.soft(takedown, where).toHaveText(state.takesDown ? [role === 'owner' ? 'Unpublish…' : 'Ask to unpublish…'] : []);
			await viewer.keyboard.press('Escape');
			await expect.soft(viewer.getByTestId('entry-language'), where).toHaveCount(state.language ? 1 : 0);
		}
	}
	// The server refuses what the editor's buttons don't offer: they may ask, but not approve,
	// publish or undo.
	const live = made.states.find((state) => state.name === 'live')?.entry ?? '';
	const generation = Number(await editor.evaluate(async (path) => (await fetch(path)).headers.get('colophon-generation'), `${env(made.project)}/releases`));
	const published = await api(editor, 'POST', `${env(made.project)}/changesets/${made.approved.id}/publish`, { base_generation: generation }, { 'If-Match': `"${made.approved.hash}"`, 'Idempotency-Key': key() });
	expect(published.status, JSON.stringify(published.body)).toBe(403);
	const takedown = await api(editor, 'POST', `${env(made.project)}/changesets`, { title: 'Down', unpublish: [live] }, { 'Idempotency-Key': key() });
	expect(takedown.status, JSON.stringify(takedown.body)).toBe(201);
	const set = takedown.body?.['data'] as { id: string; hash: string };
	expect((await api(editor, 'POST', `${env(made.project)}/changesets/${set.id}/submit`, undefined, { 'If-Match': `"${set.hash}"` })).status).toBe(200);
	const approval = await api(editor, 'POST', `${env(made.project)}/changesets/${set.id}/reviews`, { decision: 'approve' }, { 'If-Match': `"${set.hash}"` });
	expect(approval.status, JSON.stringify(approval.body)).toBe(403);
	const undo = await api(editor, 'POST', `${env(made.project)}/releases/${generation}/undo`, undefined, { 'Idempotency-Key': key() });
	expect(undo.status, JSON.stringify(undo.body)).toBe(403);

	// A reader: no next step at all, nothing to take down, and not even a request allowed.
	// The editor's request to take `live` down (above) now waits for review, so a reader sees `live`
	// as in review: the badge is read once the page's reads have settled, never from the frame drawn
	// before the waiting requests arrive (which showed `live` and let a stale expectation pass).
	await makeReader(page, editor, made.project);
	for (const state of made.states) {
		await editor.goto(`/studio/projects/${made.project}/${state.collection}/${state.entry}`);
		await editor.waitForLoadState('networkidle');
		const badge = state.entry === live ? 'review' : state.badge;
		await expect(editor.getByTestId('entry-state'), `reader, ${state.name}`).toHaveAttribute('data-state', badge);
		// Before the role read arrives the page offers nothing to anyone, so "no next step" proves
		// something only once it says why: the reader's sentence comes from the role read itself.
		await expect(editor.getByTestId('publish-entry')).toContainText('Your role can’t publish or ask for review here.');
		expect.soft(await primaries(editor), `reader, ${state.name}`).toEqual([]);
		await expect.soft(editor.getByTestId('publish-button')).toHaveCount(0);
		await editor.getByTestId('more-menu').click();
		await expect(editor.getByTestId('technical-menu')).toBeVisible();
		await expect.soft(editor.getByTestId('unpublish-button'), `reader, ${state.name}`).toHaveCount(0);
		await editor.keyboard.press('Escape');
	}
	await expect(editor.getByTestId('publish-entry')).toContainText('Your role can’t publish or ask for review here.');
	const asked = await api(editor, 'POST', `${env(made.project)}/changesets`, { title: 'Sneak', unpublish: [live] }, { 'Idempotency-Key': key() });
	expect(asked.status, JSON.stringify(asked.body)).toBe(403);
	await editor.context().close();
});
