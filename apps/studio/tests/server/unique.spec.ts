// Unique fields (step M.4, D-070) in the Studio, against the real server: the builder's switch,
// offered only where a field can be unique and left out when off; a value another entry has,
// refused under its field with "Open it" when the role may read that entry, and without its name
// when not (URL names too); a field made unique while two entries share a value, refused with the
// entries listed, then accepted once one changes; and a release that would publish a value
// another entry still publishes, said in the publish flow's words.

import { expect, type Page, test } from '@playwright/test';
import { api, applyField, editField, env, internalTerms, key, member, newEntry, newProject, publishThroughApi, saveBuilder, signIn, violations } from './helpers.ts';

const text = (id: string, label: string, rest: Record<string, unknown> = {}) => ({ id, label, type: 'text', required: false, max_chars: 200, multiline: false, ...rest });
const PEOPLE = {
	name: 'People',
	title_field: 'name',
	fields: [
		text('name', 'Name', { required: true }),
		{ id: 'email', label: 'Email', type: 'email', required: false, unique: true },
		{ id: 'slug', label: 'URL name', type: 'slug', required: false, source: 'name' }
	]
};

async function people(page: Page, definition: unknown = PEOPLE): Promise<{ project: string; ada: string }> {
	await signIn(page);
	const project = await newProject(page);
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'people', definition }, { 'Idempotency-Key': key() })).status).toBe(201);
	const ada = await newEntry(page, project, 'Ada', 'people', { name: 'Ada', email: 'ada@example.com', slug: 'ada' });
	return { project, ada };
}

async function fields(page: Page, project: string, slug: string): Promise<Record<string, any>[]> {
	return (await api(page, 'GET', `${env(project)}/content-types/${slug}`)).body?.['data'].definition.fields as Record<string, any>[];
}

/** A new person, made in the Studio as `page`; their editor is open. */
async function newPerson(page: Page, project: string, name: string) {
	await page.goto(`/studio/projects/${project}/people?new=1`);
	await page.getByLabel('Name', { exact: true }).fill(name);
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
}

test('the switch is offered only where a field can be unique, and left out of the definition when off', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const definition = {
		name: 'People',
		title_field: 'name',
		fields: [
			text('name', 'Name', { required: true }),
			{ id: 'email', label: 'Email', type: 'email', required: false },
			text('bio', 'Bio', { multiline: true }),
			{ id: 'contact', label: 'Contact', type: 'object', required: false, fields: [{ id: 'work', label: 'Work email', type: 'email', required: false }] }
		]
	};
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'people', definition }, { 'Idempotency-Key': key() })).status).toBe(201);
	await page.goto(`/studio/projects/${project}/settings/types/people`);
	// Offered in a field's advanced settings only where it can be: one-line text and email, not
	// several lines; a group can't be unique, nor anything inside one.
	for (const [place, offered] of [['name', 1], ['email', 1], ['bio', 0], ['contact/work', 0], ['contact', 0]] as const) {
		const dialog = await editField(page, place, 'Advanced');
		await expect(dialog.getByTestId('field-unique'), place).toHaveCount(offered);
		if (place === 'email') expect(await violations(page)).toEqual([]);
		await page.keyboard.press('Escape');
		await expect(dialog).toBeHidden();
	}

	await (await editField(page, 'email', 'Advanced')).getByLabel('No two entries may share this').check();
	await applyField(page);
	await expect(page.getByTestId('field-email')).toContainText('Unique');
	await saveBuilder(page);
	await expect(page.getByText('Saved People, version 2')).toBeVisible();
	let saved = await fields(page, project, 'people');
	expect(saved[1]).toMatchObject({ id: 'email', unique: true });
	expect(saved[0]).not.toHaveProperty('unique');

	await page.reload();
	const email = await editField(page, 'email', 'Advanced');
	await expect(email.getByLabel('No two entries may share this')).toBeChecked();
	await email.getByLabel('No two entries may share this').uncheck();
	await applyField(page);
	await saveBuilder(page);
	await expect(page.getByText('Saved People, version 3')).toBeVisible();
	saved = await fields(page, project, 'people');
	expect(saved[1]).not.toHaveProperty('unique');
});

test('a value another entry has is refused under its field, with a way to that entry', async ({ page }) => {
	const { project, ada } = await people(page);
	await newPerson(page, project, 'Bea');
	await page.getByLabel('Email (optional)').fill('ada@example.com');
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'refused');
	const problem = page.locator('[data-testid="field-problem"][data-field="email"]');
	await expect(problem).toContainText('Another entry already has this Email.');
	await expect(problem.getByTestId('open-holder')).toHaveText('Open “Ada”');
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);

	// On a phone, the refusal fits.
	await page.setViewportSize({ width: 390, height: 844 });
	await page.waitForLoadState('networkidle');
	expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'the editor scrolls sideways on a phone').toBeLessThanOrEqual(0);
	await page.setViewportSize({ width: 1280, height: 800 });

	await problem.getByTestId('open-holder').click();
	await expect(page).toHaveURL(new RegExp(`/people/${ada}$`));
	await expect(page.getByLabel('Name', { exact: true })).toHaveValue('Ada');
});

test('a role that reads only its own entries is refused without the other entry’s name, URL names too', async ({ page, browser }) => {
	const { project } = await people(page);
	const permissions = [
		{ action: 'content.read', subject: 'people', condition: 'own' },
		{ action: 'content.create', subject: 'people' },
		{ action: 'content.update', subject: 'people', condition: 'own' }
	];
	expect((await api(page, 'POST', `/api/v1/projects/${project}/roles`, { key: 'own-writer', name: 'Own writer', permissions })).status).toBe(201);
	const writer = await member(page, browser, project, 'own-writer');
	await newPerson(writer, project, 'Bea');
	await writer.getByLabel('Email (optional)').fill('ada@example.com');
	await expect(writer.getByTestId('save-status')).toHaveAttribute('data-status', 'refused');
	const problem = writer.locator('[data-testid="field-problem"][data-field="email"]');
	await expect(problem).toHaveText('Another entry already has this Email.');
	await expect(problem.getByTestId('open-holder')).toHaveCount(0);
	expect(await internalTerms(writer)).toEqual([]);
	await writer.getByLabel('Email (optional)').fill('bea@example.com');
	await expect(writer.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');

	// A URL name taken by an entry the role can't read: refused, without naming it.
	await writer.getByLabel('URL name (optional)').fill('ada');
	await expect(writer.getByTestId('save-status')).toHaveAttribute('data-status', 'refused');
	await expect(writer.getByTestId('save-status')).toContainText('another entry already uses this URL name: choose another');
	await expect(writer.getByTestId('save-status')).not.toContainText('Ada');
	await writer.context().close();
});

test('a field made unique while two entries share a value is refused with them listed, then accepted', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const codes = { name: 'Codes', title_field: 'title', fields: [text('title', 'Title', { required: true }), text('code', 'Code')] };
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'codes', definition: codes }, { 'Idempotency-Key': key() })).status).toBe(201);
	await newEntry(page, project, 'First', 'codes', { title: 'First', code: 'X1' });
	const second = await newEntry(page, project, 'Second', 'codes', { title: 'Second', code: 'X1' });

	await page.goto(`/studio/projects/${project}/settings/types/codes`);
	await (await editField(page, 'code', 'Advanced')).getByLabel('No two entries may share this').check();
	await applyField(page);
	await saveBuilder(page);
	const refusal = page.getByTestId('type-refusal');
	await expect(refusal).toContainText('Some entries share a value in this field');
	await expect(refusal.getByRole('link', { name: 'First' })).toBeVisible();
	await expect(refusal.getByRole('link', { name: 'Second' })).toBeVisible();
	await expect(refusal.getByTestId('blocker-problem').first()).toHaveText('It shares its Code with another entry.');
	await expect(page.getByTestId('field-code').getByTestId('row-problem')).toContainText('Some entries share a value in this field');
	expect(await violations(page)).toEqual([]);
	await page.getByTestId('field-code').getByRole('button', { name: 'Open its settings' }).click();
	await expect(page.getByTestId('field-dialog').getByTestId('unique-problem')).toBeVisible();
	await page.keyboard.press('Escape');

	// One changes; now it can be unique.
	const etag = await page.evaluate(async (path) => (await fetch(path)).headers.get('etag') ?? '', `${env(project)}/collections/codes/entries/${second}`);
	expect((await api(page, 'PUT', `${env(project)}/collections/codes/entries/${second}`, { title: 'Second', code: 'X2' }, { 'If-Match': etag, 'Idempotency-Key': key() })).status).toBe(200);
	await saveBuilder(page);
	await expect(page.getByText('Saved Codes, version 2')).toBeVisible();
	expect((await fields(page, project, 'codes'))[1]).toMatchObject({ id: 'code', unique: true });
});

test('publishing a value another entry still publishes is said in the publish flow’s words', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const codes = (unique: boolean) => ({
		name: 'Codes',
		title_field: 'title',
		fields: [text('title', 'Title', { required: true }), text('code', 'Code', unique ? { unique: true } : {})]
	});
	const base = `${env(project)}/content-types`;
	expect((await api(page, 'POST', base, { slug: 'codes', definition: codes(false) }, { 'Idempotency-Key': key() })).status).toBe(201);
	// Before the field is unique: X publishes "k" and its draft moves to "m"; Y's draft is "k".
	const x = await newEntry(page, project, 'X', 'codes', { title: 'X', code: 'k' });
	await publishThroughApi(page, project, 'X', [x]);
	const etag = await page.evaluate(async (path) => (await fetch(path)).headers.get('etag') ?? '', `${env(project)}/collections/codes/entries/${x}`);
	expect((await api(page, 'PUT', `${env(project)}/collections/codes/entries/${x}`, { title: 'X', code: 'm' }, { 'If-Match': etag, 'Idempotency-Key': key() })).status).toBe(200);
	const y = await newEntry(page, project, 'Y', 'codes', { title: 'Y', code: 'k' });
	// Drafts (m, k) and what is published (k) share nothing, so the field can become unique.
	expect((await api(page, 'PUT', `${base}/codes`, codes(true), { 'If-Match': '"v1"', 'Idempotency-Key': key() })).status).toBe(200);

	// Y alone would publish "k", which readers still get from X.
	await page.goto(`/studio/projects/${project}/codes/${y}`);
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await page.getByTestId('publish-button').click();
	await expect(page.getByTestId('publish-outcome')).toContainText('A value in it that must be unique is still what readers get from another entry.');
	expect(await internalTerms(page)).toEqual([]);
});
