// The website starter (W.0) in the Studio, against the real server: the "A website" card opens a
// dialog that says what a run adds, installing writes the component, the three types and the
// starter entries — the theme wholly from its defaults — a second open says nothing is left, and
// the seeded Home page edits like any entry.

import { expect, test } from '@playwright/test';
import { api, env, newProject, signIn, violations } from './helpers.ts';

test('the website starter installs, seeds and reruns harmlessly', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);

	await page.goto(`/studio/projects/${project}/settings/types`);
	await page.getByTestId('site-template').click();
	const plan = page.getByTestId('site-plan');
	await expect(plan).toContainText('the Pages type');
	await expect(plan).toContainText('the Section style component');
	await page.getByTestId('site-install').click();
	const steps = page.getByTestId('site-install-steps');
	await expect(steps).toContainText('Adding Pages');
	await expect(page.getByTestId('site-open-home')).toBeVisible();

	// What the run wrote, asked back over the API.
	const type = (slug: string) => api(page, 'GET', `${env(project)}/content-types/${slug}`);
	expect((await type('pages')).status).toBe(200);
	expect((await type('navigation')).body?.['data'].definition.single).toBe(true);
	expect((await type('theme')).body?.['data'].definition.single).toBe(true);
	expect((await api(page, 'GET', `${env(project)}/components/section-style`)).status).toBe(200);
	const sections = ((await type('pages')).body?.['data'].definition.fields as { id: string; blocks?: { name: string }[] }[]).find(
		(field) => field.id === 'sections'
	);
	expect(sections?.blocks?.map((kind) => kind.name)).toEqual(['hero', 'rich_text', 'features', 'testimonial', 'cta', 'faq']);

	const entries = (collection: string) => api(page, 'GET', `${env(project)}/collections/${collection}/entries`);
	const pages = (await entries('pages')).body?.['data'] as { id: string; revision: { title: string } }[];
	const home = pages.find((entry) => entry.revision.title === 'Home');
	expect(home).toBeDefined();
	const draft = (await api(page, 'GET', `${env(project)}/collections/pages/entries/${home?.id}`)).body?.['data'];
	expect((draft?.content?.sections as { _block: string }[]).map((item) => item._block)).toEqual(['hero', 'cta']);
	expect(draft?.content?.slug).toBe('home');

	const theme = (await entries('theme')).body?.['data'] as { id: string }[];
	expect(theme).toHaveLength(1);
	const themeDraft = (await api(page, 'GET', `${env(project)}/collections/theme/entries/${theme[0]?.id}`)).body?.['data'];
	expect(themeDraft?.content?.light?.primary).toBe('#6d28d9');
	expect(themeDraft?.content?.light?.radius_md).toBe(8);

	const nav = (await entries('navigation')).body?.['data'] as { id: string }[];
	const navDraft = (await api(page, 'GET', `${env(project)}/collections/navigation/entries/${nav[0]?.id}`)).body?.['data'];
	expect(navDraft?.content?.items).toEqual([{ label: 'Home', page: home?.id }]);

	// Opening again says there's nothing left to add.
	await page.getByTestId('site-open-home').click();
	await expect(page).toHaveURL(new RegExp(`/pages/${home?.id}$`));

	// The editor lays each field out in a column — label and hints above the control.
	const titleBox = (await page.getByLabel('Title', { exact: true }).boundingBox()) ?? { y: 0 };
	const labelBox = (await page.locator('label[for$="-title"]').boundingBox()) ?? { y: 1 };
	expect(labelBox.y).toBeLessThan(titleBox.y);

	await page.goto(`/studio/projects/${project}/settings/types`);
	await page.getByTestId('site-template').click();
	await expect(page.getByTestId('site-installed')).toBeVisible();
	expect(await violations(page)).toEqual([]);
});

test('a seeded page gains a section through the picker', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/settings/types`);
	await page.getByTestId('site-template').click();
	await page.getByTestId('site-install').click();
	await page.getByTestId('site-open-home').click();

	await expect(page.getByTestId('section-kind')).toHaveValue('hero');
	await page.getByTestId('section-kind').selectOption('features');
	await page.getByTestId('section-add').click();
	await page.getByTestId('save-now').click();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');

	const entry = new URL(page.url()).pathname.split('/').at(-1) ?? '';
	const draft = (await api(page, 'GET', `${env(project)}/collections/pages/entries/${entry}`)).body?.['data'];
	expect((draft?.content?.sections as { _block: string }[]).map((item) => item._block)).toEqual(['hero', 'cta', 'features']);
});

test('the Look tab renders the page and a section click opens its form', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/settings/types`);
	await page.getByTestId('site-template').click();
	await page.getByTestId('site-install').click();
	await page.getByTestId('site-open-home').click();

	// With the second section closed, clicking it in the canvas returns to the form and opens it.
	const secondToggle = page.getByTestId('section').nth(1).locator('button[aria-expanded]');
	await secondToggle.click();
	await expect(secondToggle).toHaveAttribute('aria-expanded', 'false');

	// The canvas is the shared renderer's document over the draft: the seeded hero and call to
	// action, the theme's stylesheet, the menu's resolved Home link.
	await page.getByRole('tab', { name: 'Look' }).click();
	const canvas = page.frameLocator('[data-testid="site-canvas"]');
	await expect(canvas.locator('h1')).toHaveText('Your site, your way');
	await expect(canvas.locator('.site-nav a')).toHaveText('Home');
	await expect(canvas.locator('.site-cta .site-btn')).toHaveText('Read more');
	await expect(canvas.locator('.site-section')).toHaveCount(2);

	await canvas.locator('.site-cta').click();
	await expect(page.getByRole('tab', { name: 'Content' })).toHaveAttribute('aria-selected', 'true');
	await expect(secondToggle).toHaveAttribute('aria-expanded', 'true');
	await expect(secondToggle).toBeFocused();
	expect(await violations(page)).toEqual([]);
});

test('the theme entry shows its design and warns on weak contrast', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/settings/types`);
	await page.getByTestId('site-template').click();
	await page.getByTestId('site-install').click();
	await expect(page.getByTestId('site-open-home')).toBeVisible();

	const theme = ((await api(page, 'GET', `${env(project)}/collections/theme/entries`)).body?.['data'] as { id: string }[])[0];
	await page.goto(`/studio/projects/${project}/theme/${theme?.id}`);

	// The design sheet is the shared renderer's stylesheet with this draft's variables.
	await page.getByRole('tab', { name: 'Look' }).click();
	const sheet = page.frameLocator('[data-testid="theme-canvas"]');
	await expect(sheet.locator('h1').first()).toHaveText('Heading in the heading font');
	await expect(sheet.locator('.site-btn').first()).toHaveText('A primary button');

	// A primary that can't carry white text is named, with its ratio.
	await page.getByRole('tab', { name: 'Content' }).click();
	await page.getByTestId('group-light').getByLabel('Primary', { exact: true }).fill('#ffffff');
	await page.getByRole('tab', { name: 'Look' }).click();
	const warnings = page.getByTestId('contrast-warnings');
	await expect(warnings).toContainText('buttons and primary sections — 1.00:1');
	await expect(warnings).toContainText('links and accents — 1.00:1');
	await expect(warnings).toContainText('#ffffff on #ffffff');
	// Axe reads a hidden panel out of the tree; on Content the canvas's document is out of scope.
	await page.getByRole('tab', { name: 'Content' }).click();
	expect(await violations(page)).toEqual([]);
});
