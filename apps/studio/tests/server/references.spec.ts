// Links between entries (step M.2, D-065) in the Studio, against the real server: a link field built
// in the builder; an author chosen by searching titles, kept after a reload; a list of three,
// reordered; a linked entry the role can't read shown as "An entry you can't see"; an id the server
// refuses marked on its chip.

import { expect, type Page, test } from '@playwright/test';
import { api, env, internalTerms, key, member, newEntry, newProject, signIn, violations } from './helpers.ts';

const PEOPLE = {
	name: 'People',
	title_field: 'title',
	fields: [{ id: 'title', label: 'Name', type: 'text', required: true, max_chars: 200, multiline: false }]
};
const STORIES = {
	name: 'Stories',
	title_field: 'title',
	fields: [
		{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false },
		{ id: 'author', label: 'Author', type: 'reference', required: false, to: 'people', many: false },
		{ id: 'related', label: 'Related', type: 'reference', required: false, to: 'stories', many: true, max: 3 }
	]
};

/** A project with people and stories, made through the API. */
export async function newsroom(page: Page): Promise<{ project: string; ada: string; grace: string; story: string; others: string[] }> {
	await signIn(page);
	const project = await newProject(page);
	for (const [slug, definition] of [['people', PEOPLE], ['stories', STORIES]] as const) {
		const made = await api(page, 'POST', `${env(project)}/content-types`, { slug, definition }, { 'Idempotency-Key': key() });
		expect(made.status, JSON.stringify(made.body)).toBe(201);
	}
	const ada = await newEntry(page, project, 'Ada Lovelace', 'people', { title: 'Ada Lovelace' });
	const grace = await newEntry(page, project, 'Grace Hopper', 'people', { title: 'Grace Hopper' });
	const others = [];
	for (const title of ['Second story', 'Third story', 'Fourth story']) others.push(await newEntry(page, project, title, 'stories', { title }));
	const story = await newEntry(page, project, 'Notes on the engine', 'stories', { title: 'Notes on the engine' });
	return { project, ada, grace, story, others };
}

const status = (page: Page) => page.getByTestId('save-status');

test('a link field is built in the builder: the collection it links to, one or a list', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	for (const name of ['People', 'Stories']) {
		await page.goto(`/studio/projects/${project}/settings`);
		await page.locator('#type-name').fill(name);
		await page.getByRole('button', { name: 'Create and add fields' }).click();
		await expect(page.getByTestId('field-title')).toBeVisible();
	}
	await page.getByLabel('New field').fill('Author');
	await page.getByLabel('Type').selectOption({ label: 'Link to entries' });
	await page.getByRole('button', { name: 'Add field' }).click();
	const author = page.getByTestId('field-author');
	await author.getByLabel('Links to').selectOption({ label: 'People' });
	await expect(author.getByLabel('One entry')).toBeChecked();
	await page.getByLabel('New field').fill('Related');
	await page.getByLabel('Type').selectOption({ label: 'Link to entries' });
	await page.getByRole('button', { name: 'Add field' }).click();
	const related = page.getByTestId('field-related');
	await related.getByLabel('Links to').selectOption({ label: 'Stories (this one)' });
	await related.getByLabel('A list of up to').check();
	await related.getByLabel('Most entries in Related').fill('3');
	expect(await violations(page)).toEqual([]);
	await page.getByRole('button', { name: 'Save content type' }).click();
	await expect(page.getByText('Saved Stories, version 2')).toBeVisible();

	const saved = await api(page, 'GET', `${env(project)}/content-types/stories`);
	const fields = saved.body?.['data'].definition.fields as Record<string, unknown>[];
	expect(fields.find((field) => field['id'] === 'author')).toMatchObject({ type: 'reference', to: 'people', many: false });
	expect(fields.find((field) => field['id'] === 'related')).toMatchObject({ type: 'reference', to: 'stories', many: true, max: 3 });
	// Saved, a link keeps pointing where it points.
	await page.reload();
	await expect(page.getByTestId('field-author').getByLabel('Links to')).toBeDisabled();
});

test('an author is chosen by searching titles; a list of three is reordered; both come back after a reload', async ({ page }) => {
	const { project, story, others } = await newsroom(page);
	await page.goto(`/studio/projects/${project}/stories/${story}`);
	const author = page.getByTestId('reference-author');
	await author.getByTestId('reference-search').fill('Ada');
	await author.getByRole('button', { name: /Ada Lovelace/ }).click();
	await expect(author.getByTestId('reference-chip')).toHaveText(/Ada Lovelace/);
	await expect(author.getByTestId('reference-search')).toHaveCount(0);
	await expect(status(page)).toHaveAttribute('data-status', 'saved');

	const related = page.getByTestId('reference-related');
	for (const title of ['Second story', 'Third story', 'Fourth story']) {
		await related.getByTestId('reference-search').fill(title.split(' ')[0] ?? '');
		await related.getByRole('button', { name: new RegExp(title) }).click();
	}
	await expect(related.getByTestId('reference-chip')).toHaveCount(3);
	await expect(related).toContainText('At most 3 entries');
	await related.getByRole('button', { name: 'Move Fourth story up' }).click();
	await expect(related.getByTestId('reference-chip')).toHaveText([/Second story/, /Fourth story/, /Third story/]);
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);

	const read = await api(page, 'GET', `${env(project)}/collections/stories/entries/${story}`);
	expect(read.body?.['data'].content.related).toEqual([others[0], others[2], others[1]]);

	await page.reload();
	await expect(page.getByTestId('reference-author').getByTestId('reference-chip')).toHaveText(/Ada Lovelace/);
	await expect(page.getByTestId('reference-related').getByTestId('reference-chip')).toHaveText([/Second story/, /Fourth story/, /Third story/]);
	await page.getByTestId('reference-related').getByRole('button', { name: 'Remove Third story' }).click();
	await expect(page.getByTestId('reference-related').getByTestId('reference-chip')).toHaveCount(2);
	await expect(status(page)).toHaveAttribute('data-status', 'saved');

	// On a phone, the picker fits.
	await page.setViewportSize({ width: 390, height: 844 });
	await page.waitForLoadState('networkidle');
	expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'the editor scrolls sideways on a phone').toBeLessThanOrEqual(0);
	expect(await violations(page)).toEqual([]);
});

test('a linked entry the role can’t read shows as “An entry you can’t see”, never as its id', async ({ page, browser }) => {
	const { project, story, ada } = await newsroom(page);
	expect((await api(page, 'PUT', `${env(project)}/collections/stories/entries/${story}`, { title: 'Notes on the engine', author: ada }, await ifMatch(page, project, story))).status).toBe(200);
	const read = { action: 'content.read', subject: 'stories' };
	expect((await api(page, 'POST', `/api/v1/projects/${project}/roles`, { key: 'stories-only', name: 'Stories only', permissions: [read, { action: 'content.update', subject: 'stories' }] })).status).toBe(201);
	const other = await member(page, browser, project, 'stories-only');
	await other.goto(`/studio/projects/${project}/stories/${story}`);
	const chip = other.getByTestId('reference-author').getByTestId('reference-chip');
	await expect(chip).toHaveText(/An entry you can’t see/);
	await expect(chip).not.toContainText(ada);
	expect(await internalTerms(other)).toEqual([]);
	await other.context().close();
});

test('an id the server refuses is marked on its chip, in plain words', async ({ page }) => {
	const { project, story, others } = await newsroom(page);
	expect(
		(await api(page, 'PUT', `${env(project)}/collections/stories/entries/${story}`, { title: 'Notes on the engine', related: [others[0], others[1]] }, await ifMatch(page, project, story)))
			.status
	).toBe(200);
	await page.goto(`/studio/projects/${project}/stories/${story}`);
	await expect(page.getByTestId('reference-related').getByTestId('reference-chip')).toHaveCount(2);
	// As if the second had gone between choosing it and saving: the server names its place.
	await page.route(`**/collections/stories/entries/${story}`, (route) =>
		route.request().method() === 'PUT'
			? route.fulfill({
					status: 422,
					json: {
						error: {
							code: 'validation_failed',
							message: 'this names no entry of the collection the field points at',
							details: { reason: 'reference_not_found', pointer: '/related/1' }
						}
					}
				})
			: route.fallback()
	);
	await page.getByLabel('Title', { exact: true }).fill('Notes on the engine, revised');
	await expect(status(page)).toHaveAttribute('data-status', 'refused');
	await expect(status(page)).toContainText('A linked entry isn’t there');
	const chips = page.getByTestId('reference-related').getByTestId('reference-chip');
	await expect(chips.nth(1)).toHaveAttribute('data-invalid', 'true');
	await expect(chips.nth(0)).not.toHaveAttribute('data-invalid', 'true');
	await expect(page.locator('[data-testid="field-problem"][data-field="related"]')).toContainText('Remove it and choose again');
	expect(await internalTerms(page)).toEqual([]);
});

/** The headers that save on top of an entry's current version. */
async function ifMatch(page: Page, project: string, entry: string): Promise<Record<string, string>> {
	const etag = await page.evaluate(async (path) => (await fetch(path)).headers.get('etag') ?? '', `${env(project)}/collections/stories/entries/${entry}`);
	return { 'If-Match': etag, 'Idempotency-Key': key() };
}
