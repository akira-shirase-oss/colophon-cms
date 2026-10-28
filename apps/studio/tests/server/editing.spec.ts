// The Studio's editor against a real server: autosave, the conflict screen when two people
// edit one article (F04), going offline, and edits kept on the device across a reload.

import { type Browser, expect, type Page, test } from '@playwright/test';
import { newProject, signIn, violations } from './helpers.ts';

/** Creates an article through the Studio; returns the editor's path. */
async function newArticle(page: Page, project: string, title: string): Promise<string> {
	await page.goto(`/studio/projects/${project}/articles`);
	await page.getByRole('button', { name: 'New entry' }).click();
	await page.getByLabel('Title', { exact: true }).fill(title);
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	return new URL(page.url()).pathname;
}

async function typeInBody(page: Page, text: string) {
	await page.getByRole('textbox', { name: 'Body' }).click();
	await page.keyboard.press('End');
	await page.keyboard.type(text);
}

const status = (page: Page) => page.getByTestId('save-status');

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
	await expect(page.getByRole('textbox', { name: 'Body' })).toContainText('Written in the Studio.');
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
	const body = second.getByRole('textbox', { name: 'Body' });
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
	await expect(page.getByRole('textbox', { name: 'Body' })).toContainText('Typed without a network.');
});

test('edits made just before a reload come back from this device and are saved', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const path = await newArticle(page, project, 'Reloaded');

	// Type, then leave before the autosave fires.
	await typeInBody(page, 'Not yet saved.');
	await page.goto(path);
	await expect(page.getByTestId('save-message')).toHaveText('Restored unsaved changes from this device.');
	await expect(page.getByRole('textbox', { name: 'Body' })).toContainText('Not yet saved.');
	await expect(status(page)).toHaveAttribute('data-status', 'saved');

	await page.goto(path);
	await expect(page.getByRole('textbox', { name: 'Body' })).toContainText('Not yet saved.');
	await expect(page.getByTestId('save-message')).toHaveCount(0);
});

test('the home page is accessible signed out and signed in, and lists your projects', async ({ page }) => {
	const asked = page.waitForResponse((response) => response.url().endsWith('/api/v1/installation'));
	await page.goto('/studio/');
	await expect(page.getByTestId('signed-out')).toBeVisible();
	// An installation someone runs greets visitors as usual, not as a new one (D-082): checked once
	// the Studio has its answer.
	expect(await (await asked).json()).toEqual({ data: { has_administrator: true } });
	await expect(page.getByTestId('new-installation')).toHaveCount(0);
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
			localStorage.setItem(key ?? '', JSON.stringify({ base: etag, at, content: { title: 'Expired copy', body } }));
		},
		[`colophon:unsaved:${project}:${entry}`, etag]
	);
	await page.goto(path);
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	await expect(page.getByRole('textbox', { name: 'Body' })).not.toContainText('Too old to restore.');
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

test('an entry of a content type you define: every kind of field, saved and read back', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const created = await page.evaluate(async (project) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch(`/api/v1/projects/${project}/environments/production/content-types`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, 'Idempotency-Key': crypto.randomUUID() },
			body: JSON.stringify({
				slug: 'events',
				definition: {
					name: 'Events',
					title_field: 'name',
					fields: [
						{ id: 'name', label: 'Name', type: 'text', required: true },
						{ id: 'starts_at', label: 'Starts at', type: 'datetime', required: true },
						{ id: 'kind', label: 'Kind', type: 'enum', required: true, values: ['Talk', 'Workshop'] },
						{ id: 'capacity', label: 'Capacity', type: 'number', integer: true, min: 1 },
						{ id: 'online', label: 'Online', type: 'boolean' },
						{ id: 'notes', label: 'Notes', type: 'text', multiline: true },
						{ id: 'description', label: 'Description', type: 'rich_text' }
					]
				}
			})
		});
		return response.status;
	}, project);
	expect(created).toBe(201);

	await page.goto(`/studio/projects/${project}`);
	await page.getByTestId('nav-collections').getByRole('link', { name: 'Events' }).click();
	await page.getByRole('button', { name: 'New entry' }).click();
	await page.getByLabel('Name', { exact: true }).fill('Launch night');
	await page.getByLabel('Starts at').fill('2026-11-20T18:30');
	await page.getByLabel('Kind').selectOption('Workshop');
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	const path = new URL(page.url()).pathname;

	await page.getByLabel('Capacity (optional)').fill('120');
	await page.getByLabel('Online').check();
	await page.getByLabel('Notes (optional)').fill('Doors at six.\nBring a laptop.');
	await page.getByRole('textbox', { name: 'Description' }).click();
	await page.keyboard.type('Hands-on.');
	await expect(status(page)).toHaveAttribute('data-status', 'saved');

	await page.goto(path);
	await expect(page.getByLabel('Name', { exact: true })).toHaveValue('Launch night');
	await expect(page.getByLabel('Starts at')).toHaveValue('2026-11-20T18:30');
	await expect(page.getByLabel('Kind')).toHaveValue('Workshop');
	await expect(page.getByLabel('Capacity (optional)')).toHaveValue('120');
	await expect(page.getByLabel('Online')).toBeChecked();
	await expect(page.getByRole('textbox', { name: 'Description' })).toContainText('Hands-on.');
	expect(await violations(page)).toEqual([]);

	// What the server stored: canonical order, a whole number, a UTC time.
	const entry = path.split('/').at(-1) ?? '';
	const stored = await page.evaluate(
		async ([project, entry]) =>
			(await (await fetch(`/api/v1/projects/${project}/environments/production/collections/events/entries/${entry}`)).json()).data.content,
		[project, entry]
	);
	expect(Object.keys(stored)).toEqual(['name', 'starts_at', 'kind', 'capacity', 'online', 'notes', 'description']);
	expect(stored.capacity).toBe(120);
	expect(stored.starts_at).toMatch(/^2026-11-2\dT\d\d:30:00Z$/);
	expect(stored.notes).toBe('Doors at six.\nBring a laptop.');
});

test('a terminal is approved on the device page and gets a working token', async ({ page }) => {
	await signIn(page);
	const started = await page.evaluate(async () => {
		const response = await fetch('/auth/device', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ client_name: 'Playwright terminal' })
		});
		return ((await response.json()) as { data: { user_code: string; device_secret: string; verification_uri: string } }).data;
	});
	expect(new URL(started.verification_uri).pathname).toBe('/studio/device');

	await page.goto(new URL(started.verification_uri).pathname + new URL(started.verification_uri).search);
	await expect(page.getByTestId('device-ask')).toContainText('Playwright terminal');
	await expect(page.getByTestId('device-code')).toHaveText(started.user_code);
	expect(await violations(page)).toEqual([]);
	await page.getByRole('button', { name: 'Allow', exact: true }).click();
	await expect(page.getByTestId('device-outcome')).toContainText('return to it');

	// The terminal's next poll gets a token, and the token works.
	const status = await page.evaluate(async (secret) => {
		const polled = await fetch('/auth/device/token', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ device_secret: secret })
		});
		const token = ((await polled.json()) as { data: { access_token: string } }).data.access_token;
		const me = await fetch('/me', { headers: { Authorization: `Bearer ${token}` }, credentials: 'omit' });
		return me.status;
	}, started.device_secret);
	expect(status).toBe(200);

	// The same link afterwards: nothing left to approve.
	await page.reload();
	await expect(page.getByTestId('device-outcome')).toContainText('expired or was already used');
});
