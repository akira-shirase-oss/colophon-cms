// The entry page, tidied like the builder (D-081): the trail names the entry by its title; the side
// panel says where it stands in one line (not "Live in" and "Readers see this version" both);
// "Save now" shows only with something to save; the public address and "See what readers get" wait
// in Technical details; and on a phone the status and Publish share one bar at the bottom. The
// fields follow the type's order, rich text included (D-082).

import { expect, test } from '@playwright/test';
import { api, env, key, newEntry, newProject, publishThroughApi, signIn, violations } from './helpers.ts';

const paragraphs = (count: number) => ({
	schema: 'colophon/rich-text',
	version: 1,
	doc: {
		type: 'doc',
		content: Array.from({ length: count }, (_, index) => ({
			type: 'paragraph',
			content: [{ type: 'text', text: `Paragraph ${index + 1}: fresh peas, wild garlic and a new lemon tart, served all spring.` }]
		}))
	}
});

test('the side panel says where an entry stands once, offers Save now only when there is something to save, and keeps developer details closed', async ({ page, context }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Spring menu is here');
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	const status = page.getByTestId('save-status');
	await expect(status).toHaveAttribute('data-status', 'saved');

	// The trail names the entry, not "Entry".
	const trail = page.getByRole('navigation', { name: 'breadcrumb' });
	await expect(trail).toContainText('Spring menu is here');
	await expect(trail.getByText('Entry', { exact: true })).toHaveCount(0);

	// A draft: one line says so, and Publish is there.
	const panel = page.getByTestId('entry-actions');
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'draft');
	await expect(page.getByTestId('publish-state')).toHaveText('Readers can’t see it: it isn’t published.');
	await expect(page.getByTestId('publish-button')).toBeVisible();
	await expect(page.getByTestId('save-now')).toHaveCount(0);
	await expect(page.getByTestId('public-address')).toHaveCount(0);

	// Published: one status line names the release; nothing else repeats it.
	const generation = await publishThroughApi(page, project, 'Out it goes', [entry]);
	await page.reload();
	await expect(status).toHaveAttribute('data-status', 'saved');
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'live');
	await expect(page.getByTestId('publish-state')).toHaveText(`Readers see this version, in Release ${generation}.`);
	await expect(page.getByTestId('publish-state').getByRole('link', { name: `Release ${generation}` })).toHaveAttribute(
		'href',
		`/studio/projects/${project}/releases/${generation}`
	);
	await expect(panel.getByText(`Release ${generation}`)).toHaveCount(1);
	await expect(panel.getByText('Live in', { exact: true })).toHaveCount(0);
	await expect(page.getByTestId('save-now')).toHaveCount(0);
	await expect(page.getByTestId('unpublish-button')).toBeVisible();

	// The public address and what readers get are closed until Technical details is opened.
	await expect(page.getByTestId('public-address')).toBeHidden();
	await expect(page.getByTestId('see-what-readers-get')).toBeHidden();
	await expect(page.getByText('Where readers get it')).toBeHidden();
	await page.getByTestId('entry-details').getByTestId('technical-details').locator('summary').click();
	await expect(page.getByTestId('public-address')).toBeVisible();
	await expect(page.getByTestId('see-what-readers-get')).toBeVisible();
	expect(await violations(page)).toEqual([]);

	// Unsaved changes: Save now appears, and says so; held saves keep it there.
	let hold = true;
	await page.route(`**/entries/${entry}`, async (route) => {
		if (route.request().method() === 'PUT' && hold) return;
		await route.fallback();
	});
	await page.getByLabel('Title', { exact: true }).fill('Spring menu is here, with tarts');
	await expect(page.getByTestId('save-now')).toBeVisible();
	await expect(page.getByTestId('publish-state')).toHaveText(`Readers see the version in Release ${generation}, not your newer changes.`);
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'changed');
	hold = false;
	await page.unrouteAll({ behavior: 'ignoreErrors' });
	await page.reload();
	await page.getByLabel('Title', { exact: true }).fill('Spring menu is here, with tarts');
	await expect(status).toHaveAttribute('data-status', 'saved');
	await expect(page.getByTestId('save-now')).toHaveCount(0);
	// The trail follows the saved title.
	await expect(trail).toContainText('Spring menu is here, with tarts');

	// A save that fails: Save now is back, to try again at once.
	await context.setOffline(true);
	await page.getByLabel('Title', { exact: true }).fill('Spring menu, offline');
	await expect(status).toHaveAttribute('data-status', 'retrying');
	await expect(page.getByTestId('save-now')).toBeVisible();
	await context.setOffline(false);
	await page.getByTestId('save-now').click();
	await expect(status).toHaveAttribute('data-status', 'saved');
	await expect(page.getByTestId('save-now')).toHaveCount(0);
});

test('on a phone, the status and Publish share one bar that stays at the bottom while the body scrolls', async ({ browser, page }) => {
	await signIn(page);
	const project = await newProject(page);
	const draft = await newEntry(page, project, 'A longer read', 'articles', { title: 'A longer read', body: paragraphs(14) });
	const phone = await (await browser.newContext({ baseURL: new URL(page.url()).origin, viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true })).newPage();
	await phone.context().addCookies(await page.context().cookies());

	const bar = phone.getByTestId('entry-actions');
	async function barHolds(button: 'publish-button' | 'unpublish-button') {
		await expect(phone.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
		// At the top, and halfway down the body.
		for (const share of [0, 0.5]) {
			await phone.evaluate((part) => window.scrollTo(0, Math.round((document.documentElement.scrollHeight - window.innerHeight) * part)), share);
			await expect(phone.getByTestId('entry-state')).toBeInViewport();
			await expect(phone.getByTestId(button)).toBeInViewport();
			const box = await bar.boundingBox();
			expect(box, 'the bar is on screen').not.toBeNull();
			// One row, at the very bottom: the status on the left, the button on the right.
			expect(Math.round((box?.y ?? 0) + (box?.height ?? 0))).toBe(844);
			expect(box?.height ?? 0, 'the bar is one row (it was three, about 120px)').toBeLessThanOrEqual(80);
			const status = await phone.getByTestId('entry-state').boundingBox();
			const action = await phone.getByTestId(button).boundingBox();
			expect(Math.abs((status?.y ?? 0) + (status?.height ?? 0) / 2 - ((action?.y ?? 0) + (action?.height ?? 0) / 2)), 'status and button on one line').toBeLessThan(12);
			expect(action?.x ?? 0).toBeGreaterThan(status?.x ?? 0);
		}
	}

	await phone.goto(`/studio/projects/${project}/articles/${draft}`);
	await barHolds('publish-button');
	// The trail fits: the collection and the entry's title, cut short if long.
	await expect(phone.getByRole('navigation', { name: 'breadcrumb' })).toContainText('A longer read');

	await publishThroughApi(page, project, 'Out', [draft]);
	await phone.reload();
	await expect(phone.getByTestId('entry-state')).toHaveAttribute('data-state', 'live');
	await barHolds('unpublish-button');
	// The details follow the fields, below the bar's place at the end of the page.
	await phone.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
	await expect(phone.getByTestId('entry-details')).toBeInViewport();
	await phone.waitForLoadState('networkidle');
	expect(await phone.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'the editor scrolls sideways on a phone').toBeLessThanOrEqual(0);
	expect(await violations(phone)).toEqual([]);
	await phone.context().close();
});

test('the editor shows fields in the order the type lists them, with rich text full width where it sits', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const products = {
		name: 'Products',
		title_field: 'name',
		fields: [
			{ id: 'name', label: 'Name', type: 'text', required: true, max_chars: 200, multiline: false },
			{ id: 'description', label: 'Description', type: 'rich_text', required: false },
			{ id: 'price', label: 'Price', type: 'number', required: false },
			{ id: 'care', label: 'Care notes', type: 'rich_text', required: false },
			{ id: 'on_sale', label: 'On sale', type: 'boolean', required: false }
		]
	};
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'products', definition: products }, { 'Idempotency-Key': key() })).status).toBe(201);
	const entry = await newEntry(page, project, 'Tote', 'products', { name: 'Tote' });
	await page.goto(`/studio/projects/${project}/products/${entry}`);
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');

	// Top to bottom as the builder lists them, not plain fields first and rich text last.
	const fields = [
		page.getByLabel('Name', { exact: true }),
		page.getByRole('textbox', { name: 'Description' }),
		page.getByLabel('Price (optional)', { exact: true }),
		page.getByRole('textbox', { name: 'Care notes' }),
		page.getByLabel('On sale', { exact: true })
	];
	const tops: number[] = [];
	for (const field of fields) tops.push((await field.boundingBox())?.y ?? Number.NaN);
	expect(tops).toEqual([...tops].sort((a, b) => a - b));
	// Each rich text field takes the content's full width.
	const panel = await page.locator('#panel-content').boundingBox();
	for (const rich of await page.getByTestId('rich-field').all()) expect((await rich.boundingBox())?.width).toBeCloseTo(panel?.width ?? 0, 0);
	expect(await violations(page)).toEqual([]);

	// Fields after a rich text field save like any other.
	await page.getByLabel('Price (optional)', { exact: true }).fill('24');
	await page.getByRole('textbox', { name: 'Care notes' }).click();
	await page.keyboard.type('Wash cold.');
	await page.getByLabel('On sale', { exact: true }).check();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	const saved = (await api(page, 'GET', `${env(project)}/collections/products/entries/${entry}`)).body?.['data'].content;
	expect(saved.price).toBe(24);
	expect(saved.on_sale).toBe(true);
	expect(JSON.stringify(saved.care)).toContain('Wash cold.');
	await page.reload();
	await expect(page.getByLabel('Price (optional)', { exact: true })).toHaveValue('24');
	await expect(page.getByRole('textbox', { name: 'Care notes' })).toContainText('Wash cold.');
});
