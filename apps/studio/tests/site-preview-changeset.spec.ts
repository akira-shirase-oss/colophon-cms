// "Preview on your site" for a publish request (gap/preview), in the browser without a server:
// the request page issues a grant for exactly the request (`{"changeset": id}`) and opens its
// address in a new tab; not set up, it says why and where (or whom to ask); every refusal is said
// plainly with the codes behind Technical details; once the request is live it isn't offered.
// Keyboard, axe in light and dark, and (with STUDIO_SHOTS_DIR set) photographs at 360 and 1440 px.

import AxeBuilder from '@axe-core/playwright';
import { expect, type Page, test } from '@playwright/test';
import { CHANGESET, CHANGESET_URL, PROJECT, serve, SITE, world, type World } from './site-preview-world.ts';

const WIDE = { width: 1440, height: 900 };
const PHONE = { width: 360, height: 780 };
const NOTE = 'Shows exactly what this request will publish, for 15 minutes. If the request changes, preview it again.';

async function visit(page: Page, w: World, studio: string | undefined) {
	await serve(page, w, new URL(studio ?? '').origin);
	await page.goto(CHANGESET_URL);
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Spring issue');
}

async function axe(page: Page) {
	const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa']).analyze();
	return results.violations.map((violation) => `${violation.id}: ${violation.help} (${violation.nodes.map((node) => node.target.join(' ')).join(', ')})`);
}

async function shot(page: Page, name: string) {
	const directory = process.env['STUDIO_SHOTS_DIR'];
	if (directory === undefined || directory === '') return;
	await page.screenshot({ path: `${directory}/${name}.png`, animations: 'disabled', caret: 'hide' });
}

const strip = (page: Page) => page.getByTestId('changeset-preview');
const button = (page: Page) => page.getByTestId('changeset-preview-button');
const problem = (page: Page) => page.getByTestId('changeset-preview-problem');

test.use({ viewport: WIDE });

test('opens exactly this request on the site, in a new tab, with its own one-time code', async ({ page, baseURL }) => {
	const w = world();
	let release = () => {};
	w.hold = new Promise((done) => (release = done));
	await visit(page, w, baseURL);
	await expect(button(page)).toHaveText('Preview on your site');
	await expect(strip(page)).toContainText(NOTE);
	await expect(button(page)).toHaveAccessibleDescription(NOTE);

	// Opening: the tab opens at once (no popup blocker), and the button says what is happening.
	const [tab] = await Promise.all([page.waitForEvent('popup'), button(page).click()]);
	await expect(button(page)).toHaveText('Opening…');
	await expect(button(page)).toHaveAttribute('aria-busy', 'true');
	await expect(button(page)).toBeEnabled();
	await shot(page, 'preview-changeset-opening-1440-light');
	release();
	w.hold = null;
	await tab.waitForURL(`${SITE}/**`);
	expect(new URL(tab.url()).searchParams.get('code')).toBe('cpc_grant1');
	expect(await tab.evaluate(() => window.opener)).toBeNull();
	expect(w.issued).toEqual([{ changeset: CHANGESET }]);
	await expect(button(page)).toHaveText('Preview on your site');
	await expect(problem(page)).toHaveCount(0);
	expect(w.unanswered).toEqual([]);
});

test('not set up: disabled, with the reason and where to set it up', async ({ page, baseURL }) => {
	await visit(page, world({ template: null }), baseURL);
	await expect(button(page)).toBeDisabled();
	await expect(page.getByTestId('changeset-preview-reason')).toHaveText('Preview on your site isn’t set up yet. Set it up in Website connections');
	await expect(button(page)).toHaveAccessibleDescription('Preview on your site isn’t set up yet. Set it up in Website connections');
	await expect(page.getByTestId('changeset-preview-set-up')).toHaveAttribute('href', `/studio/projects/${PROJECT}/settings/connections#preview`);
	await expect(strip(page)).not.toContainText(NOTE);
	await shot(page, 'preview-changeset-not-set-up-1440-light');
});

test('not set up, for a role that can’t set it up: ask an owner', async ({ page, baseURL }) => {
	await visit(page, world({ template: null, role: 'editor' }), baseURL);
	await expect(button(page)).toBeDisabled();
	await expect(page.getByTestId('changeset-preview-reason')).toHaveText('Preview on your site isn’t set up yet: ask an owner of this project.');
	await expect(page.getByTestId('changeset-preview-set-up')).toHaveCount(0);
});

test('refusals in plain words, the codes behind Technical details, and the button stays to try again', async ({ page, baseURL }) => {
	const w = world({ issue: 'already-live' });
	await visit(page, w, baseURL);
	const cases: [World['issue'], string, string][] = [
		['already-live', 'This publish request is already live', 'HTTP 409 · already_published'],
		['limit', 'Too many previews open', 'HTTP 429 · preview_limit'],
		['offline', 'Colophon didn’t answer. Check your connection, then try again.', 'no answer'],
		['unavailable', 'Colophon couldn’t start the preview just now. Try again in a moment.', 'HTTP 503']
	];
	for (const [issue, words, technical] of cases) {
		w.issue = issue;
		await button(page).click();
		await expect(problem(page)).toContainText(words);
		await problem(page).getByText('Technical details').click();
		await expect(problem(page).getByTestId('technical-details')).toContainText(technical);
		if (issue === 'already-live') await shot(page, 'preview-changeset-refused-1440-light');
		await expect(button(page)).toBeEnabled();
	}
	// Nothing was left open: each refused try closed the tab it opened.
	expect(page.context().pages()).toHaveLength(1);
	// The address was taken away since the page loaded: it says so, as when it was never set.
	w.issue = 'not-configured';
	w.template = null;
	await button(page).click();
	await expect(button(page)).toBeDisabled();
	await expect(page.getByTestId('changeset-preview-reason')).toContainText('Preview on your site isn’t set up yet.');
	await expect(problem(page)).toHaveCount(0);
});

test('a request that is live already isn’t offered a preview', async ({ page, baseURL }) => {
	await visit(page, world({ request: 'published' }), baseURL);
	await expect(page.getByTestId('changeset-status')).toHaveAttribute('data-state', 'published');
	await expect(strip(page)).toHaveCount(0);
});

test('keyboard only: reach it, press it, and the focus stays on it, visibly', async ({ page, baseURL }) => {
	const w = world();
	await visit(page, w, baseURL);
	await page.locator('body').focus();
	for (let presses = 0; presses < 60; presses += 1) {
		await page.keyboard.press('Tab');
		if (await button(page).evaluate((element) => element === document.activeElement)) break;
	}
	await expect(button(page)).toBeFocused();
	const ringed = () => button(page).evaluate((element) => getComputedStyle(element).boxShadow !== 'none');
	expect(await ringed()).toBe(true);
	const [tab] = await Promise.all([page.waitForEvent('popup'), page.keyboard.press('Enter')]);
	await tab.waitForURL(`${SITE}/**`);
	await page.bringToFront();
	await expect(button(page)).toBeFocused();
	expect(w.issued).toEqual([{ changeset: CHANGESET }]);
});

test.describe('accessibility', () => {
	for (const scheme of ['light', 'dark'] as const) {
		test(`no violations, ${scheme}: ready, refused, and not set up`, async ({ page, baseURL }) => {
			await page.emulateMedia({ colorScheme: scheme });
			const w = world({ issue: 'already-live' });
			await visit(page, w, baseURL);
			expect(await axe(page), 'ready').toEqual([]);
			await button(page).click();
			await expect(problem(page)).toBeVisible();
			await problem(page).getByText('Technical details').click();
			expect(await axe(page), 'refused').toEqual([]);
			w.template = null;
			await page.reload();
			await expect(button(page)).toBeDisabled();
			expect(await axe(page), 'not set up').toEqual([]);
		});
	}
});

test.describe('photographs', () => {
	test.skip(!process.env['STUDIO_SHOTS_DIR'], 'set STUDIO_SHOTS_DIR to photograph');
	for (const scheme of ['light', 'dark'] as const) {
		for (const size of [WIDE, PHONE]) {
			test(`${size.width} px, ${scheme}`, async ({ page, baseURL }) => {
				await page.setViewportSize(size);
				await page.emulateMedia({ colorScheme: scheme });
				await visit(page, world(), baseURL);
				await expect(button(page)).toBeEnabled();
				await expect(page.getByTestId('changeset-items')).toContainText('Hello, preview');
				await shot(page, `preview-changeset-${size.width}-${scheme}`);
			});
		}
	}
});
