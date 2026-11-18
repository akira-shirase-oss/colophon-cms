// The editor's side panel in three blocks (D-184), and what moved out of it: Unpublish asks first
// and can be undone for ten seconds, putting back the version readers had; restoring the live
// version from History reads Live with nothing to publish (F3); ⋯ More works from the keyboard;
// History says who changed what in each sitting; the title is the page's heading; comments open
// in a drawer beside the content. Each surface is checked with axe in light and dark, and at 390px.

import { expect, type Page, test } from '@playwright/test';
import { api, env, key, newEntry, newProject, openTechnicalDetails, publishThroughApi, shot, signIn, toast, violations } from './helpers.ts';

function body(...paragraphs: string[]) {
	return {
		schema: 'colophon/rich-text',
		version: 1,
		doc: { type: 'doc', content: paragraphs.map((text) => ({ type: 'paragraph', content: [{ type: 'text', text }] })) }
	};
}

/** What readers get for one entry, without signing in: its status and, when live, its version. */
async function readers(page: Page, project: string, entry: string): Promise<{ status: number; title?: string; hash?: string }> {
	return page.evaluate(async (path) => {
		const response = await fetch(path, { credentials: 'omit', cache: 'no-store' });
		if (!response.ok) return { status: response.status };
		const { data } = (await response.json()) as { data: { title: string; content_hash: string } };
		return { status: response.status, title: data.title, hash: data.content_hash };
	}, `/content/v1/${project}/production/articles/${entry}`);
}

/** Saves `title` as the entry's new draft through the API, as another tab would. */
async function retitle(page: Page, project: string, entry: string, title: string) {
	const path = `${env(project)}/collections/articles/entries/${entry}`;
	const current = await page.evaluate(async (path) => {
		const response = await fetch(path);
		return { etag: response.headers.get('etag') ?? '', content: ((await response.json()) as { data: { content: Record<string, unknown> } }).data.content };
	}, path);
	const saved = await api(page, 'PUT', path, { ...current.content, title }, { 'If-Match': current.etag, 'Idempotency-Key': key() });
	expect(saved.status, JSON.stringify(saved.body)).toBe(200);
}

/** axe in light and dark. */
async function bothSchemes(page: Page, what: string) {
	for (const scheme of ['light', 'dark'] as const) {
		await page.emulateMedia({ colorScheme: scheme });
		expect(await violations(page), `${what}, ${scheme}`).toEqual([]);
	}
	await page.emulateMedia({ colorScheme: 'light' });
}

test('Unpublish asks first; Keep it live keeps it, and Undo puts back the version readers had', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, '', 'articles', { title: 'Tide tables for March', body: body('High water at 06:12 and 18:40 on the first.') });
	const generation = await publishThroughApi(page, project, 'Tides', [entry]);
	const before = await readers(page, project, entry);
	expect(before.status).toBe(200);

	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'live');
	await page.getByTestId('more-menu').click();
	await page.getByTestId('unpublish-button').click();
	const dialog = page.getByRole('alertdialog');
	await expect(dialog.getByRole('heading', { name: 'Take “Tide tables for March” off your site?' })).toBeVisible();
	await expect(dialog).toContainText(`Readers stop seeing it as soon as Release ${generation + 1} goes out.`);
	// Keeping it live is the default: it has the focus, and the last step is the red one.
	await expect(dialog.getByRole('button', { name: 'Keep it live' })).toBeFocused();
	await expect(dialog.getByRole('button', { name: 'Unpublish' })).toHaveClass(/bg-destructive/);
	await bothSchemes(page, 'unpublish confirmation');
	await shot(page, 'editor-unpublish-confirm-1440-light');
	await page.keyboard.press('Enter');
	await expect(dialog).toBeHidden();
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'live');
	expect((await readers(page, project, entry)).status).toBe(200);

	// Unpublish, then Undo from the toast: the same version is back, as a new release.
	await page.getByTestId('more-menu').click();
	await page.getByTestId('unpublish-button').click();
	await dialog.getByRole('button', { name: 'Unpublish' }).click();
	const unpublished = toast(page, `Unpublished in Release ${generation + 1}`);
	await expect(unpublished).toBeVisible();
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'taken_down');
	expect((await readers(page, project, entry)).status).toBe(410);
	await unpublished.getByRole('button', { name: 'Undo' }).click();
	await expect(toast(page, `Back on your site in Release ${generation + 2}`)).toBeVisible();
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'live');
	const after = await readers(page, project, entry);
	expect(after).toEqual(before);
});

test('Undo after unpublishing an entry with newer changes puts back what readers had and keeps the newer draft', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, '', 'articles', { title: 'Harbour fees for 2027', body: body('Mooring is 14 a night.') });
	const generation = await publishThroughApi(page, project, 'Fees', [entry]);
	const before = await readers(page, project, entry);
	await retitle(page, project, entry, 'Harbour fees for 2027, revised');

	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'changed');
	await page.getByTestId('more-menu').click();
	await page.getByTestId('unpublish-button').click();
	await page.getByRole('alertdialog').getByRole('button', { name: 'Unpublish' }).click();
	const unpublished = toast(page, `Unpublished in Release ${generation + 1}`);
	await expect(unpublished).toBeVisible();
	await unpublished.getByRole('button', { name: 'Undo' }).click();
	await expect(toast(page, 'Back on your site')).toBeVisible();

	// Readers have exactly the version they had; the draft is the newer one again.
	expect(await readers(page, project, entry)).toEqual(before);
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await expect(page.getByLabel('Title', { exact: true })).toHaveValue('Harbour fees for 2027, revised');
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'changed');
	const draft = await api(page, 'GET', `${env(project)}/collections/articles/entries/${entry}`);
	expect(draft.body?.['data'].content.title).toBe('Harbour fees for 2027, revised');
});

test('restoring the live version from History reads Live, with nothing to publish (F3)', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, '', 'articles', { title: 'Lifeboat open day', body: body('Saturday from ten.') });
	const generation = await publishThroughApi(page, project, 'Open day', [entry]);
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await page.getByLabel('Title', { exact: true }).fill('Lifeboat open day moves to Sunday');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'changed');
	await expect(page.getByTestId('publish-button')).toHaveText('Publish changes');

	// History: the edit is its own sitting (the live version ends one), said in words.
	await page.getByRole('tab', { name: /History/ }).click();
	const rows = page.getByTestId('history-row');
	await expect(rows).toHaveCount(2);
	await expect(rows.first()).toContainText('Current draft');
	await expect(rows.first().getByTestId('history-summary')).toHaveText('Title edited');
	await expect(rows.last()).toHaveAttribute('data-live', 'true');
	await expect(rows.last().getByTestId('history-live')).toHaveText(`Live in Release ${generation}`);
	await expect(rows.last().getByTestId('history-summary')).toHaveText('First version');
	await bothSchemes(page, 'history');
	await shot(page, 'editor-history-1440-light');

	await rows.last().getByTestId('history-restore').click();
	await expect(page.getByTestId('restore-note')).toHaveText('Back to the live version — nothing to publish.');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'live');
	await expect(page.getByTestId('publish-button')).toHaveCount(0);
	await expect(page.getByTestId('nothing-to-publish')).toBeVisible();
	// It is a new version with the live one's content: a reload says the same.
	await page.reload();
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'live');
	await expect(page.getByTestId('publish-state')).toContainText(`Live in Release ${generation}`);
	await expect(page.getByTestId('publish-button')).toHaveCount(0);
});

test('⋯ More is reached and used from the keyboard, and Technical details opens the Details', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Ferry strike notice');
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');

	// Tab goes from the one primary to More, which says it opens a menu.
	await page.getByTestId('publish-button').focus();
	await page.keyboard.press('Tab');
	const more = page.getByTestId('more-menu');
	await expect(more).toBeFocused();
	await expect(more).toHaveAttribute('aria-haspopup', 'menu');
	await expect(more).toHaveAccessibleName('More actions');
	await page.keyboard.press('Enter');
	await expect(page.getByRole('menu')).toBeVisible();
	// A draft: nothing to copy or take down.
	await expect(page.getByRole('menuitem')).toHaveText(['Duplicate', 'Technical details']);
	await page.keyboard.press('ArrowDown');
	await expect(page.getByRole('menuitem', { name: 'Technical details' })).toBeFocused();
	await page.keyboard.press('Enter');
	await expect(page.getByTestId('details')).toHaveAttribute('open', '');
	await expect(page.getByTestId('entry-details').getByTestId('technical-details')).toHaveAttribute('open', '');
	await expect(page.getByTestId('entry-details')).toContainText(entry);

	// Escape closes the menu and gives focus back to More.
	await more.focus();
	await page.keyboard.press('Enter');
	await expect(page.getByRole('menu')).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(page.getByRole('menu')).toBeHidden();
	await expect(more).toBeFocused();

	// Duplicate makes a copy and opens it.
	await more.click();
	await page.getByRole('menuitem', { name: 'Duplicate' }).click();
	await page.waitForURL(new RegExp(`/articles/(?!${entry})[0-9a-f-]+$`));
	await expect(page.getByLabel('Title', { exact: true })).toHaveValue('Ferry strike notice (copy)');
});

test('the title is the heading, edited in place: Enter moves to the body, and the count shows near the limit', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Short');
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	const title = page.getByRole('heading', { level: 1 }).getByRole('textbox', { name: 'Title' });
	await expect(title).toHaveValue('Short');
	await expect(page.getByTestId('title-count')).toHaveCount(0);
	await title.fill('The harbour master retires after forty years');
	await title.press('Enter');
	await expect(page.getByRole('textbox', { name: 'Body' })).toBeFocused();
	await expect(title).toHaveValue('The harbour master retires after forty years');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	// Near the 200-character limit, a count appears; under it, none.
	await title.fill('x'.repeat(170));
	await expect(page.getByTestId('title-count')).toHaveText('170 of 200 characters');
	await title.fill('A title again');
	await expect(page.getByTestId('title-count')).toHaveCount(0);
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await expect(page.getByRole('navigation', { name: 'breadcrumb' })).toContainText('A title again');
});

test('comments open in a drawer beside the content, say who wrote them, and give focus back on Escape', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, '', 'articles', { title: 'Regatta programme', body: body('Races start at eleven from the outer mole.') });
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	const button = page.getByTestId('comments-button');
	await button.click();
	const drawer = page.getByTestId('comments-drawer');
	await expect(drawer).toBeVisible();
	await expect(drawer.getByRole('heading', { name: 'Comments' })).toBeFocused();
	// The text stays in view beside it, and can still be edited.
	const content = await page.locator('#panel-content').boundingBox();
	const side = await drawer.boundingBox();
	expect((content?.x ?? 0) + (content?.width ?? 0)).toBeLessThanOrEqual(side?.x ?? 0);
	await expect(page.getByRole('textbox', { name: 'Body' })).toBeInViewport();

	// Comment with nothing written says so, rather than a grey button that does nothing.
	await drawer.getByTestId('comment-post').click();
	await expect(drawer).toContainText('Write something first.');
	await drawer.getByTestId('comment-input').fill('Is the start time confirmed?');
	await drawer.getByTestId('comment-post').click();
	await expect(drawer.getByTestId('comment')).toContainText('You');
	await expect(page.getByTestId('comments-count')).toHaveText('1');
	await expect(drawer.getByRole('tab', { name: 'Resolved' })).toBeVisible();
	await bothSchemes(page, 'comments drawer');
	await shot(page, 'editor-comments-1440-light');
	await page.keyboard.press('Escape');
	await expect(drawer).toBeHidden();
	await expect(button).toBeFocused();

	// On a phone the drawer takes the screen, and nothing scrolls sideways.
	const phone = await (await browser.newContext({ baseURL: new URL(page.url()).origin, viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true })).newPage();
	await phone.context().addCookies(await page.context().cookies());
	await phone.goto(`/studio/projects/${project}/articles/${entry}`);
	await phone.getByTestId('comments-button').click();
	await expect(phone.getByTestId('comments-drawer').getByTestId('comment')).toContainText('Is the start time confirmed?');
	expect(await phone.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBeLessThanOrEqual(0);
	await bothSchemes(phone, 'comments drawer on a phone');
	await phone.context().close();
});

test('the panel’s surfaces read on a phone in light and dark: More, the confirmation, Details and Translate', async ({ browser, page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, '', 'articles', { title: 'Lighthouse tours resume', body: body('Tours run on the hour from April.') });
	await publishThroughApi(page, project, 'Tours', [entry]);
	const phone = await (await browser.newContext({ baseURL: new URL(page.url()).origin, viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true })).newPage();
	await phone.context().addCookies(await page.context().cookies());
	await phone.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(phone.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await phone.getByTestId('more-menu').click();
	await expect(phone.getByRole('menuitem')).toHaveText(['Copy public link', 'Duplicate', 'Technical details', 'Unpublish…']);
	await bothSchemes(phone, 'more menu on a phone');
	await phone.getByTestId('unpublish-button').click();
	await expect(phone.getByRole('alertdialog')).toBeVisible();
	await bothSchemes(phone, 'confirmation on a phone');
	await phone.getByRole('button', { name: 'Keep it live' }).click();
	await openTechnicalDetails(phone);
	await expect(phone.getByTestId('public-address')).toBeVisible();
	await phone.getByTestId('translate').click();
	await expect(phone.getByTestId('language-dialog')).toBeVisible();
	await bothSchemes(phone, 'translate on a phone');
	await shot(phone, 'editor-translate-390-light');
	expect(await phone.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBeLessThanOrEqual(0);
	await phone.context().close();
});

test('when saves can’t reach the server, one amber banner says so plainly, and Retry now sends them', async ({ page, context }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, '', 'articles', { title: 'Harbour webcam', body: body('Back online on Monday.') });
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');

	// The server stops answering: every API request is refused at the connection.
	await page.route('**/api/v1/**', (route) => route.abort('connectionrefused'));
	const title = page.getByRole('heading', { level: 1 }).getByRole('textbox', { name: 'Title' });
	await title.fill('Harbour webcam is back');
	const banner = page.getByTestId('save-problem');
	await expect(banner).toBeVisible({ timeout: 30_000 });
	await expect(banner).toHaveText(/Colophon isn’t answering — your changes are saved on this device and will sync\.\s*Retry now/);
	// Said once, in the banner: no raw lowercase line under the save state.
	await expect(page.getByTestId('save-message')).toHaveCount(0);
	await expect(page.getByText('no answer from the server')).toHaveCount(0);
	await bothSchemes(page, 'save banner');
	await shot(page, 'editor-offline-1440-light');

	// The browser loses its network too: the same banner says Offline.
	await context.setOffline(true);
	await expect(banner).toContainText('Offline — your changes are saved on this device and will sync.');

	// Back: Retry now sends the kept change, and the banner goes.
	await context.setOffline(false);
	await page.unroute('**/api/v1/**');
	await banner.getByRole('button', { name: 'Retry now' }).click();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved', { timeout: 30_000 });
	await expect(banner).toBeHidden();
	const draft = await api(page, 'GET', `${env(project)}/collections/articles/entries/${entry}`);
	expect(draft.body?.['data'].content.title).toBe('Harbour webcam is back');
});
