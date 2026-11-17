// The environment's languages (gap/locales): Settings → Languages lists them with a default and,
// for each, the languages readers get where it is missing, saved whole under the list's ETag; a
// list someone changed meanwhile loads instead of being saved over; a list that leaves out a tag
// entries are written in names them, offering to add the tag (written before any list) or keep
// the language (just removed); the editor chooses an entry's language from the list, marks a
// tag the list lacks, and says a refusal of the language beside it; the collection list names
// each entry's language and filters by one; a role that can't change the list reads it. axe finds
// nothing on any of these screens at 360 and 1440 px, light and dark; with STUDIO_SHOTS_DIR set,
// each is photographed (`languages-<screen>-<width>-<scheme>.png`).

import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { expect, type Page, test } from '@playwright/test';
import { api, env, internalTerms, key, member, newProject, signIn, violations } from './helpers.ts';

const NOTES = {
	slug: 'notes',
	definition: {
		name: 'Notes',
		title_field: 'title',
		fields: [
			{ id: 'title', label: 'Title', type: 'text', required: true },
			{ id: 'note', label: 'Note', type: 'text' }
		]
	}
};

const SHOTS = process.env['STUDIO_SHOTS_DIR'];
const DESKTOP = { width: 1440, height: 900 };
const PHONE = { width: 360, height: 780 };

/** One screen at a phone's width and a desktop's, light and dark: no sideways scroll, nothing
 *  axe finds, and a photograph of each when STUDIO_SHOTS_DIR is set. Leaves the desktop size. */
async function screen(page: Page, name: string) {
	for (const size of [PHONE, DESKTOP]) {
		await page.setViewportSize(size);
		await page.waitForLoadState('networkidle');
		expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), `${name} at ${size.width}px scrolls sideways`).toBe(true);
		for (const scheme of ['light', 'dark'] as const) {
			await page.emulateMedia({ colorScheme: scheme });
			await expect(page.locator('html')).toHaveClass(scheme === 'dark' ? /\bdark\b/ : /^(?!.*\bdark\b)/);
			expect(await violations(page), `${name}, ${size.width}px, ${scheme}`).toEqual([]);
			if (SHOTS !== undefined && SHOTS !== '') {
				mkdirSync(SHOTS, { recursive: true });
				await page.screenshot({ path: join(SHOTS, `languages-${name}-${size.width}-${scheme}.png`), fullPage: true });
			}
		}
		await page.emulateMedia({ colorScheme: 'light' });
	}
}

/** One language's row on the Languages page. */
const row = (page: Page, tag: string) => page.locator(`[data-testid="language"][data-tag="${tag}"]`);

/** The languages a row falls back to, in order. */
const chain = (page: Page, tag: string) =>
	page.locator(`[data-testid="language"][data-tag="${tag}"] [data-testid="fallback"]`).evaluateAll((items) => items.map((item) => item.getAttribute('data-tag')));

async function addLanguage(page: Page, tag: string, suggested: string) {
	await page.getByTestId('add-language').click();
	const dialog = page.getByRole('dialog', { name: 'Add language' });
	await expect(dialog.getByLabel('Language code')).toBeFocused();
	await dialog.getByLabel('Language code').fill(tag);
	// The name is suggested from the code, in the language itself.
	await expect(dialog.getByLabel('Name')).toHaveValue(suggested);
	await dialog.getByTestId('add-language-submit').click();
	await expect(dialog).toBeHidden();
	await expect(row(page, tag)).toBeVisible();
}

/** Picks `label` in the "If missing, show" menu of `tag`'s row, by keyboard. */
async function addFallback(page: Page, tag: string, label: string) {
	await page.locator(`[data-add-fallback="${tag}"]`).focus();
	await page.keyboard.press('Enter');
	const item = page.getByRole('menuitem', { name: new RegExp(`^${label}`) });
	await expect(item).toBeVisible();
	await item.focus();
	await page.keyboard.press('Enter');
	await expect(page.getByRole('menu')).toBeHidden();
}

async function savedList(page: Page, project: string) {
	const read = await api(page, 'GET', `${env(project)}/locales`);
	expect(read.status, JSON.stringify(read.body)).toBe(200);
	return read.body?.['data'] as { version: number; locales: { tag: string; label: string; default: boolean; fallback: string[] }[] };
}

test('languages are listed, ordered and saved in Settings, and the editor and the list use them', async ({ page, browser }) => {
	await page.setViewportSize(DESKTOP);
	await signIn(page);
	const project = await newProject(page);
	const base = env(project);
	expect((await api(page, 'POST', `${base}/content-types`, NOTES, { 'Idempotency-Key': key() })).status).toBe(201);
	// Before any list, any tag goes: one entry with none, one in pt-BR, one in fr.
	const made = async (title: string, locale?: string) => {
		const query = locale === undefined ? '' : `?locale=${locale}`;
		const answer = await api(page, 'POST', `${base}/collections/notes/entries${query}`, { title }, { 'Idempotency-Key': key() });
		expect(answer.status, JSON.stringify(answer.body)).toBe(201);
		return (answer.body?.['data'] as { id: string }).id;
	};
	const welcome = await made('Welcome');
	const ola = await made('Olá', 'pt-BR');
	const bonjour = await made('Bonjour', 'fr');

	// Settings → Languages: none yet, said plainly, with one way to add one.
	await page.goto(`/studio/projects/${project}/settings`);
	await page.getByTestId('languages-link').click();
	await expect(page).toHaveURL(/\/settings\/languages$/);
	await expect(page.getByTestId('languages-empty')).toContainText('Your project publishes in one language.');
	await expect(page.getByTestId('languages-empty')).toContainText('Add a language to publish translations.');
	await expect(page.getByTestId('add-language')).toHaveCount(1);
	await screen(page, 'empty');

	// A code that isn't one is refused in the form, beside the field.
	await page.getByTestId('add-language').click();
	const dialog = page.getByRole('dialog', { name: 'Add language' });
	await dialog.getByLabel('Language code').fill('not a code');
	await dialog.getByTestId('add-language-submit').click();
	await expect(dialog.getByTestId('language-tag-problem')).toContainText('two or three letters');
	await expect(dialog.getByLabel('Language code')).toHaveAttribute('aria-invalid', 'true');
	await expect(dialog.getByLabel('Language code')).toBeFocused();
	await screen(page, 'add');
	await dialog.getByRole('button', { name: 'Cancel' }).click();

	// The first language is the default; later ones show the default where they are missing.
	await addLanguage(page, 'pt-BR', 'Português (Brasil)');
	await expect(row(page, 'pt-BR').locator('[data-testid="language-default"]')).toBeVisible();
	await addLanguage(page, 'en', 'English');
	expect(await chain(page, 'en')).toEqual(['pt-BR']);
	// The same code twice is refused, whatever its case.
	await page.getByTestId('add-language').click();
	await dialog.getByLabel('Language code').fill('EN');
	await dialog.getByTestId('add-language-submit').click();
	await expect(dialog.getByTestId('language-tag-problem')).toContainText('English is in the list already');
	await dialog.getByRole('button', { name: 'Cancel' }).click();

	// Make English the default; Portuguese then falls back to it from the start.
	await row(page, 'en').locator('[data-testid="make-default"]').click();
	await expect(row(page, 'en').locator('[data-testid="language-default"]')).toBeVisible();
	await expect(row(page, 'pt-BR').locator('[data-testid="language-default"]')).toHaveCount(0);
	await row(page, 'en').locator('[data-testid="fallback-remove"]').click();
	expect(await chain(page, 'en')).toEqual([]);
	await addLanguage(page, 'pt', 'Português');
	expect(await chain(page, 'pt')).toEqual(['en']);

	// pt-BR: Português, then English — added and reordered by keyboard; focus stays on the
	// language that moved.
	await addFallback(page, 'pt-BR', 'English');
	await addFallback(page, 'pt-BR', 'Português');
	expect(await chain(page, 'pt-BR')).toEqual(['en', 'pt']);
	// Nothing is left to add, so Add is gone: focus is on the language just added.
	await expect(page.locator('[data-remove="pt-BR|pt"]')).toBeFocused();
	await page.locator('[data-move="pt-BR|en|down"]').focus();
	await page.keyboard.press('Enter');
	expect(await chain(page, 'pt-BR')).toEqual(['pt', 'en']);
	await expect(page.locator('[data-move="pt-BR|en|up"]')).toBeFocused();
	await expect(page.locator('[data-move="pt-BR|pt|up"]')).toBeDisabled();
	// Every other language is in its list, so there is nothing left to add.
	await expect(page.locator('[data-add-fallback="pt-BR"]')).toHaveCount(0);

	// Unsaved until Save: the bar says so.
	await expect(page.getByTestId('save-bar')).toContainText('You have unsaved changes');
	await screen(page, 'list');
	// "Bonjour" was written in fr before there was any list, and this one leaves fr out: refused,
	// with the entry named and linked, and fr offered to add.
	await page.getByTestId('save-bar').getByTestId('save').click();
	const inUse = page.getByTestId('languages-in-use');
	await expect(inUse.getByTestId('in-use-summary')).toHaveText('1 entry is written in fr. Move or delete them first.');
	await expect(inUse.getByRole('link', { name: 'Bonjour' })).toHaveAttribute('href', `/studio/projects/${project}/notes/${bonjour}`);
	await expect(inUse.getByTestId('in-use-place')).toHaveAttribute('href', `/studio/projects/${project}/notes?locale=fr`);
	await expect(inUse.getByTestId('keep-language')).toHaveCount(0);
	await screen(page, 'in-use');
	await inUse.getByTestId('add-in-use-language').click();
	const adding = page.getByRole('dialog', { name: 'Add language' });
	await expect(adding.getByLabel('Language code')).toHaveValue('fr');
	await expect(adding.getByLabel('Name')).toHaveValue('Français');
	await adding.getByTestId('add-language-submit').click();
	await expect(adding).toBeHidden();
	await expect(inUse).toHaveCount(0);
	expect(await chain(page, 'fr')).toEqual(['en']);
	await page.getByTestId('save-bar').getByTestId('save').click();
	await expect(page.getByTestId('languages-saved')).toContainText('Saved.');
	await expect(page.getByTestId('save-bar')).toHaveCount(0);
	const first = await savedList(page, project);
	expect(first.version).toBe(1);
	expect(first.locales).toEqual([
		{ tag: 'pt-BR', label: 'Português (Brasil)', default: false, fallback: ['pt', 'en'] },
		{ tag: 'en', label: 'English', default: true, fallback: [] },
		{ tag: 'pt', label: 'Português', default: false, fallback: ['en'] },
		{ tag: 'fr', label: 'Français', default: false, fallback: ['en'] }
	]);

	// Someone else saves meanwhile: this page's save is refused, and the list as it is now loads.
	await row(page, 'pt').locator('[data-testid="language-label"]').fill('Portuguese');
	const theirs = first.locales.map((one) => (one.tag === 'pt' ? { ...one, label: 'Português (Portugal)' } : one));
	const moved = await api(page, 'PUT', `${base}/locales`, { locales: theirs }, { 'If-Match': `"locales-${first.version}"`, 'Idempotency-Key': key() });
	expect(moved.status, JSON.stringify(moved.body)).toBe(200);
	await page.getByTestId('save-bar').getByTestId('save').click();
	await expect(page.getByTestId('languages-stale')).toContainText('Someone changed languages; review and save again');
	await expect(row(page, 'pt').locator('[data-testid="language-label"]')).toHaveValue('Português (Portugal)');
	await expect(page.getByTestId('save-bar')).toHaveCount(0);
	expect((await savedList(page, project)).locales.find((one) => one.tag === 'pt')?.label).toBe('Português (Portugal)');

	// Removing pt-BR, which "Olá" is written in, asks first, then saving names the entry with a
	// link to it, and Keep puts the language back.
	await row(page, 'pt-BR').locator('[data-testid="remove-language"]').click();
	await expect(page.getByRole('alertdialog')).toContainText('Remove Português (Brasil)?');
	await page.getByTestId('confirm-remove-language').click();
	await expect(row(page, 'pt-BR')).toHaveCount(0);
	await page.getByTestId('save-bar').getByTestId('save').click();
	await expect(inUse.getByTestId('in-use-summary')).toHaveText('1 entry is written in Português (Brasil) (pt-BR). Move or delete them first.');
	await expect(inUse.getByRole('link', { name: 'Olá' })).toHaveAttribute('href', `/studio/projects/${project}/notes/${ola}`);
	await expect(inUse.getByTestId('in-use-place')).toHaveAttribute('href', `/studio/projects/${project}/notes?locale=pt-BR`);
	await expect(inUse.getByTestId('add-in-use-language')).toHaveCount(0);
	await inUse.getByTestId('keep-language').click();
	await expect(row(page, 'pt-BR')).toBeVisible();
	await expect(inUse).toHaveCount(0);
	// Nothing else referred to it, so the list is as saved again.
	await expect(page.getByTestId('save-bar')).toHaveCount(0);
	expect((await savedList(page, project)).locales.map((one) => one.tag)).toEqual(['pt-BR', 'en', 'pt', 'fr']);

	// The editor: a choice among the languages, "Default language (en)" for none.
	await page.goto(`/studio/projects/${project}/notes/${welcome}`);
	const select = page.getByTestId('entry-locale-select');
	await expect(select).toHaveValue('');
	await expect(select.locator('option:checked')).toHaveText('Default language (en)');
	await expect(select.locator('option')).toHaveText(['Default language (en)', 'Português (Brasil) — pt-BR', 'English — en', 'Português (Portugal) — pt', 'Français — fr']);
	await select.selectOption('pt');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	expect(((await api(page, 'GET', `${base}/collections/notes/entries/${welcome}`)).body?.['data'] as { locale: string }).locale).toBe('pt');
	expect(await internalTerms(page)).toEqual([]);
	await screen(page, 'editor');

	// From here the page's read of the list is changed to stand in for a list that moved after the
	// page read it (the server never lets a list leave out a tag live entries carry, so this is
	// the only way to see one): it lacks fr, which "Bonjour" is written in, and has "it", which
	// the server's list doesn't. Every answer to a save is the server's own.
	await page.route('**/environments/production/locales', async (route) => {
		if (route.request().method() !== 'GET') return route.continue();
		const response = await route.fetch();
		const read = (await response.json()) as { data: { locales: { tag: string }[] } };
		read.data.locales = [...read.data.locales.filter((one) => one.tag !== 'fr'), { tag: 'it', label: 'Italiano', default: false, fallback: [] } as { tag: string }];
		await route.fulfill({ response, json: read });
	});
	// A tag the list lacks is marked, and the entry's other changes still save: the editor sends
	// its language only when it changes.
	await page.goto(`/studio/projects/${project}/notes/${bonjour}`);
	await expect(page.getByTestId('locale-unlisted')).toContainText('Not in your languages');
	await expect(page.getByTestId('entry-locale-select')).toHaveValue('fr');
	await expect(page.getByTestId('entry-locale-select').locator('option:checked')).toHaveText('fr — not in your languages');
	await page.getByRole('textbox', { name: 'Note' }).fill('Salut');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await screen(page, 'editor-unlisted');
	// A language the server doesn't list is refused, and the refusal is said beside the choice.
	await page.getByTestId('entry-locale-select').selectOption('it');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'refused');
	await expect(page.getByTestId('locale-problem')).toContainText('isn’t one of this project’s languages');
	await expect(page.getByTestId('entry-locale-select')).toHaveAttribute('aria-invalid', 'true');
	// Choosing a listed one saves, and the marks go.
	await page.getByTestId('entry-locale-select').selectOption('en');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await expect(page.getByTestId('locale-problem')).toHaveCount(0);
	await expect(page.getByTestId('locale-unlisted')).toHaveCount(0);
	await page.unroute('**/environments/production/locales');
	await page.reload();
	await expect(page.getByTestId('entry-locale-select')).toHaveValue('en');
	await expect(page.getByTestId('entry-locale-select').locator('option[value="it"]')).toHaveCount(0);
	expect(((await api(page, 'GET', `${base}/collections/notes/entries/${bonjour}`)).body?.['data'] as { locale: string }).locale).toBe('en');

	// The collection list: each entry's language by name, and a filter by one.
	await page.goto(`/studio/projects/${project}/notes`);
	const rows = page.getByTestId('entries');
	await expect(rows.getByRole('row').filter({ hasText: 'Olá' }).getByTestId('entry-language')).toHaveText('Language: Português (Brasil)');
	await expect(rows.getByRole('row').filter({ hasText: 'Welcome' }).getByTestId('entry-language')).toHaveText('Language: Português (Portugal)');
	await expect(rows.getByRole('row').filter({ hasText: 'Bonjour' }).getByTestId('entry-language')).toHaveText('Language: English');
	await page.getByTestId('language-filter').selectOption('pt-BR');
	await expect(rows.getByRole('row')).toHaveCount(1);
	await expect(rows).toContainText('Olá');
	expect(await internalTerms(page)).toEqual([]);
	await screen(page, 'collection');
	// Opened from the in-use list's link, it starts filtered.
	await page.goto(`/studio/projects/${project}/notes?locale=en`);
	await expect(page.getByTestId('language-filter')).toHaveValue('en');
	await expect(page.getByTestId('language-filter-note')).toContainText('Entries with no language set count as English too');
	await expect(rows.getByRole('row')).toHaveCount(1);
	await expect(rows).toContainText('Bonjour');

	// A role that can't change the list reads it, without the controls.
	const editor = await member(page, browser, project, 'editor');
	await editor.setViewportSize(DESKTOP);
	await editor.goto(`/studio/projects/${project}/settings/languages`);
	await expect(editor.getByTestId('languages-read-only')).toContainText("Your role can't change the languages.");
	await expect(editor.getByTestId('languages').getByTestId('language')).toHaveCount(4);
	await expect(editor.getByTestId('languages')).toContainText('If missing, show Português (Portugal), then English.');
	await expect(editor.getByTestId('add-language')).toHaveCount(0);
	await expect(editor.locator('main input, main select')).toHaveCount(0);
	await screen(editor, 'read-only');
	await editor.context().close();
});

test('a refusal the server places in the list is said at that language', async ({ page }) => {
	await page.setViewportSize(DESKTOP);
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/settings/languages`);
	await addLanguage(page, 'en', 'English');
	await addLanguage(page, 'de', 'Deutsch');
	// The page checks what the server checks, so nothing it sends is refused this way; the PUT's
	// answer is replaced here to see where a refusal at `/locales/1/label` lands.
	await page.route('**/environments/production/locales', async (route) => {
		if (route.request().method() !== 'PUT') return route.continue();
		await route.fulfill({
			status: 422,
			json: { error: { code: 'validation_failed', message: 'label too long', request_id: 'test', details: { reason: 'invalid_locale_label', pointer: '/locales/1/label' } } }
		});
	});
	await page.getByTestId('save-bar').getByTestId('save').click();
	const problem = row(page, 'de').locator('[data-testid="language-problem"]');
	await expect(problem).toContainText('A language’s name is 1 to 80 characters.');
	await expect(row(page, 'de').locator('[data-testid="language-label"]')).toHaveAttribute('aria-invalid', 'true');
	await expect(row(page, 'en').locator('[data-testid="language-problem"]')).toHaveCount(0);
	expect(await violations(page)).toEqual([]);
	await page.unroute('**/environments/production/locales');
	// Without the stand-in, the same list saves.
	await page.getByTestId('save-bar').getByTestId('save').click();
	await expect(page.getByTestId('languages-saved')).toContainText('Saved.');
	await expect(problem).toHaveCount(0);
});
