// The Studio's editor against a real server: autosave, the conflict screen when two people
// edit one article (F04), going offline, and edits kept on the device across a reload.

import AxeBuilder from '@axe-core/playwright';
import { type Browser, expect, type Page, test } from '@playwright/test';

async function signIn(page: Page) {
	await page.goto('/studio/');
	await page.getByTestId('sign-in').click();
	await page.locator('#username').fill('dev-editor');
	await page.locator('#password').fill('colophon-dev');
	await page.locator('#kc-login').click();
	await expect(page.getByTestId('signed-in')).toBeVisible();
}

/** A fresh project, created through the API as the signed-in page; its id. */
async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch('/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'Studio test' })
		});
		return ((await response.json()) as { data: { id: string } }).data.id;
	}, `st-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

/** Creates an article through the Studio; returns the editor's path. */
async function newArticle(page: Page, project: string, title: string): Promise<string> {
	await page.goto(`/studio/projects/${project}`);
	await page.getByLabel('New article title').fill(title);
	await page.getByRole('button', { name: 'Create article' }).click();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	return new URL(page.url()).pathname;
}

async function typeInBody(page: Page, text: string) {
	await page.getByRole('textbox', { name: 'Article body' }).click();
	await page.keyboard.press('End');
	await page.keyboard.type(text);
}

const status = (page: Page) => page.getByTestId('save-status');

/** Accessibility problems on the page, one line per rule and element, so a failure says what. */
async function violations(page: Page): Promise<string[]> {
	const results = await new AxeBuilder({ page }).analyze();
	return results.violations.flatMap((violation) =>
		violation.nodes.map((node) => `${violation.id}: ${node.target.join(' ')} (${node.failureSummary ?? ''})`)
	);
}

async function editor(browser: Browser): Promise<Page> {
	const page = await (await browser.newContext()).newPage();
	await signIn(page);
	return page;
}

test('an article autosaves as you type, and is there after a reload', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const path = await newArticle(page, project, 'Autosaved');

	await typeInBody(page, 'Written in the Studio.');
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	await expect(page.getByTestId('history').locator('li')).toHaveCount(2);

	await page.goto(path);
	await expect(page.getByRole('textbox', { name: 'Article body' })).toContainText('Written in the Studio.');
	await expect(status(page)).toHaveAttribute('data-status', 'saved');

	expect(await violations(page)).toEqual([]);
});

test('two editors, one article: the later save opens the conflict screen, and keeping mine loses nothing', async ({ browser }) => {
	const first = await editor(browser);
	const project = await newProject(first);
	const path = await newArticle(first, project, 'Shared');
	const second = await editor(browser);
	await second.goto(path);
	await expect(status(second)).toHaveAttribute('data-status', 'saved');

	await typeInBody(first, 'First editor was here.');
	await expect(status(first)).toHaveAttribute('data-status', 'saved');

	await typeInBody(second, 'Second editor too.');
	await expect(status(second)).toHaveAttribute('data-status', 'conflict');
	const conflict = second.getByTestId('conflict');
	await expect(conflict.getByTestId('theirs')).toContainText('First editor was here.');
	await expect(conflict.getByTestId('mine')).toContainText('Second editor too.');
	expect(await violations(second)).toEqual([]);

	await conflict.getByRole('button', { name: 'Keep my version' }).click();
	await expect(status(second)).toHaveAttribute('data-status', 'saved');
	await expect(second.getByTestId('conflict')).toHaveCount(0);
	// Three revisions: the first, the first editor's, and the second's on top of it.
	await expect(second.getByTestId('history').locator('li')).toHaveCount(3);
});

test('choosing their version discards mine and continues from theirs', async ({ browser }) => {
	const first = await editor(browser);
	const project = await newProject(first);
	const path = await newArticle(first, project, 'Theirs wins');
	const second = await editor(browser);
	await second.goto(path);
	await expect(status(second)).toHaveAttribute('data-status', 'saved');

	await typeInBody(first, 'The version to keep.');
	await expect(status(first)).toHaveAttribute('data-status', 'saved');
	await typeInBody(second, 'A change to drop.');
	await expect(status(second)).toHaveAttribute('data-status', 'conflict');

	await second.getByRole('button', { name: 'Use their version' }).click();
	await expect(status(second)).toHaveAttribute('data-status', 'saved');
	const body = second.getByRole('textbox', { name: 'Article body' });
	await expect(body).toContainText('The version to keep.');
	await expect(body).not.toContainText('A change to drop.');
	await expect(second.getByTestId('history').locator('li')).toHaveCount(2);
});

test('offline, edits are kept and saved once the network is back', async ({ page, context }) => {
	await signIn(page);
	const project = await newProject(page);
	const path = await newArticle(page, project, 'Offline');

	await context.setOffline(true);
	await typeInBody(page, 'Typed without a network.');
	await expect(status(page)).toHaveAttribute('data-status', 'retrying');
	await context.setOffline(false);
	await expect(status(page)).toHaveAttribute('data-status', 'saved', { timeout: 15_000 });

	await page.goto(path);
	await expect(page.getByRole('textbox', { name: 'Article body' })).toContainText('Typed without a network.');
});

test('edits made just before a reload come back from this device and are saved', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const path = await newArticle(page, project, 'Reloaded');

	// Type, then leave before the autosave fires.
	await typeInBody(page, 'Not yet saved.');
	await page.goto(path);
	await expect(page.getByTestId('save-message')).toHaveText('Restored unsaved changes from this device.');
	await expect(page.getByRole('textbox', { name: 'Article body' })).toContainText('Not yet saved.');
	await expect(status(page)).toHaveAttribute('data-status', 'saved');

	await page.goto(path);
	await expect(page.getByRole('textbox', { name: 'Article body' })).toContainText('Not yet saved.');
	await expect(page.getByTestId('save-message')).toHaveCount(0);
});

test('the home page is accessible signed out and signed in, and lists your projects', async ({ page }) => {
	await page.goto('/studio/');
	await expect(page.getByTestId('signed-out')).toBeVisible();
	expect(await violations(page)).toEqual([]);
	await signIn(page);
	const project = await newProject(page);
	await page.goto('/studio/');
	await expect(page.getByTestId('projects').getByRole('link')).not.toHaveCount(0);
	await expect(page.locator(`a[href="/studio/projects/${project}"]`)).toBeVisible();
	expect(await violations(page)).toEqual([]);
});

test('a save whose answer is lost is sent again with the same key, and replayed instead of saved twice', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await newArticle(page, project, 'Lost answer');

	const keys: string[] = [];
	const answers: { status: number; replayed: string | undefined }[] = [];
	await page.route('**/collections/articles/entries/*', async (route) => {
		if (route.request().method() !== 'PUT') return route.continue();
		keys.push(route.request().headers()['idempotency-key'] ?? '');
		const response = await route.fetch();
		answers.push({ status: response.status(), replayed: response.headers()['idempotent-replayed'] });
		// The first save reaches the server and is committed; its answer never comes back.
		if (answers.length === 1) return route.abort('connectionreset');
		return route.fulfill({ response });
	});

	await typeInBody(page, 'Saved exactly once.');
	await expect(status(page)).toHaveAttribute('data-status', 'saved', { timeout: 15_000 });
	expect(answers[0]).toEqual({ status: 200, replayed: undefined });
	expect(answers[1]).toEqual({ status: 200, replayed: 'true' });
	expect(new Set(keys).size, 'the retry reused the key').toBe(1);
	await expect(page.getByTestId('history').locator('li')).toHaveCount(2);
});

test('edits kept on this device expire after 30 days', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const path = await newArticle(page, project, 'Expired copy');
	const entry = path.split('/').at(-1) ?? '';
	const etag = await page.evaluate(async ([project, entry]) => {
		const response = await fetch(`/api/v1/projects/${project}/environments/production/collections/articles/entries/${entry}`);
		return response.headers.get('etag');
	}, [project, entry]);
	// A copy kept 31 days ago, on the current revision.
	await page.evaluate(
		([key, etag]) => {
			const at = new Date(Date.now() - 31 * 86_400_000).toISOString();
			const body = { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Too old to restore.' }] }] } };
			localStorage.setItem(key ?? '', JSON.stringify({ base: etag, at, article: { title: 'Expired copy', body } }));
		},
		[`colophon:unsaved:${project}:${entry}`, etag]
	);
	await page.goto(path);
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	await expect(page.getByRole('textbox', { name: 'Article body' })).not.toContainText('Too old to restore.');
	expect(await page.evaluate((key) => localStorage.getItem(key), `colophon:unsaved:${project}:${entry}`)).toBeNull();
});

test('a save that committed just before a reload is resent with its key on reopening, not stored twice', async ({ page }) => {
	// Found by the simulator (new-key-after-crash, D-039): reopening used to compare the kept
	// edit with the draft, and when someone had saved on top in between, offered "keep mine",
	// which stored the already-committed edit a second time.
	await signIn(page);
	const project = await newProject(page);
	const path = await newArticle(page, project, 'Reload mid-save');

	// The first save reaches the server and commits; its answer is dropped, and the page reloads
	// before the editor can retry.
	let committed: () => void = () => {};
	const done = new Promise<void>((resolve) => (committed = resolve));
	await page.route('**/collections/articles/entries/*', async (route) => {
		if (route.request().method() !== 'PUT') return route.continue();
		const response = await route.fetch();
		expect(response.status()).toBe(200);
		await route.abort('connectionreset');
		committed();
	});
	await typeInBody(page, 'Committed, answer lost.');
	await done;
	await page.unrouteAll({ behavior: 'ignoreErrors' });

	// Meanwhile someone else saves on top of the committed edit.
	const entry = path.split('/').at(-1) ?? '';
	const saved = await page.evaluate(
		async ([project, entry]) => {
			const url = `/api/v1/projects/${project}/environments/production/collections/articles/entries/${entry}`;
			const current = await fetch(url);
			const draft = ((await current.json()) as { data: { content: { body: unknown } } }).data;
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const response = await fetch(url, {
				method: 'PUT',
				headers: {
					'Content-Type': 'application/json',
					'X-CSRF-Token': csrf,
					'If-Match': current.headers.get('etag') ?? '',
					'Idempotency-Key': crypto.randomUUID()
				},
				body: JSON.stringify({ title: 'Changed by someone else', body: draft.content.body })
			});
			return response.status;
		},
		[project, entry]
	);
	expect(saved).toBe(200);

	const answers: { key: string | undefined; replayed: string | undefined }[] = [];
	page.on('response', (response) => {
		if (response.request().method() === 'PUT') {
			answers.push({ key: response.request().headers()['idempotency-key'], replayed: response.headers()['idempotent-replayed'] });
		}
	});
	await page.goto(path);
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	await expect(page.getByTestId('conflict')).toHaveCount(0);
	expect(answers[0]?.replayed, 'the reopened page resent the same request and got the recorded answer').toBe('true');
	// The first revision, the committed edit once, and the other person's save: nothing twice.
	await expect(page.getByTestId('history').locator('li')).toHaveCount(3);
});
