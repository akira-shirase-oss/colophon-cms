// What readers get (gap audit, 2026-10-28): a published entry's page opens the public read of it,
// made as a website makes it, without signing in. It names the release in words, says when newer
// changes are left out, shows the answer exactly as the website receives it (and copies it), and
// says plainly when the entry has been taken down meanwhile.

import { expect, test } from '@playwright/test';
import { api, env, internalTerms, key, newEntry, newProject, publishThroughApi, signIn, violations } from './helpers.ts';

test('a published entry shows what readers get, as the website receives it, and says when that is behind or gone', async ({ page, context, request }) => {
	await context.grantPermissions(['clipboard-read', 'clipboard-write']);
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Read by many');
	const generation = await publishThroughApi(page, project, 'Out it goes', [entry]);
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');

	// It waits in Technical details, beside the public address (D-081).
	await expect(page.getByTestId('see-what-readers-get')).toBeHidden();
	await page.getByTestId('entry-details').getByTestId('technical-details').locator('summary').click();
	await page.getByTestId('see-what-readers-get').click();
	const view = page.getByTestId('reader-view');
	await expect(view.getByRole('heading', { name: 'What readers get' })).toBeVisible();
	await expect(view.getByTestId('reader-title')).toHaveText('Read by many');
	await expect(view.getByTestId('reader-release')).toHaveText(`Release ${generation}`);
	await expect(view.getByTestId('reader-changed')).toHaveCount(0);
	// Exactly the public answer: the same data a request with no session gets.
	const shown = JSON.parse((await view.getByTestId('reader-data').textContent()) ?? '') as unknown;
	const publicRead = await request.get(`/content/v1/${project}/production/articles/${entry}`);
	expect(publicRead.status()).toBe(200);
	expect(shown).toEqual(await publicRead.json());
	expect(await internalTerms(page)).toEqual([]);
	expect(await violations(page)).toEqual([]);

	await view.getByTestId('reader-copy').click();
	await expect(page.getByText('Copied', { exact: true })).toBeVisible();
	expect(JSON.parse(await page.evaluate(() => navigator.clipboard.readText()))).toEqual(shown);
	await view.getByRole('button', { name: 'Close' }).click();
	await expect(view).toBeHidden();

	// Newer changes: readers still get the published version, and it says so.
	await page.getByLabel('Title', { exact: true }).fill('Read by many, revised');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await page.getByTestId('see-what-readers-get').click();
	await expect(view.getByTestId('reader-changed')).toContainText("Your newer changes aren't in it");
	await expect(view.getByTestId('reader-title')).toHaveText('Read by many');
	await view.getByRole('button', { name: 'Close' }).click();

	// Taken down meanwhile (by someone else, while this page was open): it says readers get nothing.
	// The page's own read freezes where it stood — a tab the stream's hint never reached — so the
	// button still offers the read; the view itself asks readers' API and finds nothing there.
	const frozen = await page.evaluate(
		async (path) => (await (await fetch(path)).text()),
		`${env(project)}/collections/articles/entries/${entry}`
	);
	await page.route(`**/collections/articles/entries/${entry}`, (route) =>
		route.request().method() === 'GET'
			? route.fulfill({ status: 200, contentType: 'application/json', body: frozen })
			: route.continue()
	);
	await publishThroughApi(page, project, 'Taken down', [], [entry]);
	await page.getByTestId('see-what-readers-get').click();
	await expect(view.getByTestId('reader-outcome')).toContainText('Readers get nothing for it now: it was taken down.');
	await expect(view.getByTestId('reader-copy')).toHaveCount(0);
	expect(await internalTerms(page)).toEqual([]);
	expect(await violations(page)).toEqual([]);
});

test('in one language, it says which translation readers get and when that is a fallback', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	// English is the default; French readers get English where there is no French; German
	// readers get German or nothing (D-178).
	const set = await api(
		page,
		'PUT',
		`${env(project)}/locales`,
		{
			locales: [
				{ tag: 'en', label: 'English', default: true },
				{ tag: 'fr', label: 'Français', fallback: ['en'] },
				{ tag: 'de', label: 'Deutsch' }
			]
		},
		{ 'If-Match': '"locales-0"', 'Idempotency-Key': key() }
	);
	expect(set.status, JSON.stringify(set.body)).toBe(200);
	const made = await api(
		page,
		'POST',
		`${env(project)}/collections/articles/entries?locale=en`,
		{ title: 'Hello', body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } },
		{ 'Idempotency-Key': key() }
	);
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	const entry = made.body?.['data'].id as string;
	await publishThroughApi(page, project, 'Out it goes', [entry]);
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await page.getByTestId('entry-details').getByTestId('technical-details').locator('summary').click();
	await page.getByTestId('see-what-readers-get').click();
	const view = page.getByTestId('reader-view');
	await expect(view.getByTestId('reader-title')).toHaveText('Hello');
	await expect(view.getByTestId('reader-resolved')).toHaveCount(0);

	// French readers get the English, and it says that is a fallback.
	await view.getByTestId('reader-language-select').selectOption('fr');
	await expect(view.getByTestId('reader-resolved')).toContainText('English');
	await expect(view.getByTestId('reader-resolved')).toContainText('falls back from Français');
	expect(await violations(page)).toEqual([]);
	// German readers get nothing, and it says which languages it is published in.
	await view.getByTestId('reader-language-select').selectOption('de');
	await expect(view.getByTestId('reader-outcome')).toContainText('Readers in Deutsch get nothing for this');
	await expect(view.getByTestId('reader-outcome')).toContainText('It is published in English.');
	await expect(view.getByTestId('reader-copy')).toHaveCount(0);
	// English readers get it as it is.
	await view.getByTestId('reader-language-select').selectOption('en');
	await expect(view.getByTestId('reader-resolved')).toHaveText('English');
	expect(await internalTerms(page)).toEqual([]);
	expect(await violations(page)).toEqual([]);
});
