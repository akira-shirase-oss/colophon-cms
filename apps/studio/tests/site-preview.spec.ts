// Preview on the editor's own site (D-173), in the browser, without a colophon-server: the
// management routes are answered by site-preview-world.ts as the contract says, and the editor's
// site is a stub page that talks to the Studio the way the SDK's preview helper does. Covered:
// the Settings card (checks as you type, save, the server's refusal said plainly, read-only for
// other roles), the Preview button when nothing is set up, every state of the pane, the save →
// site message (only after a save the server acknowledged), renewal, revocation, keyboard-only
// use, the phone's Edit / Preview tabs, axe in light and dark, and, with STUDIO_SHOTS_DIR set,
// photographs at 360 and 1440 px in light and dark (named preview-*).

import AxeBuilder from '@axe-core/playwright';
import { expect, type Locator, type Page, test } from '@playwright/test';
import { EDITOR_URL, ENTRY, PROJECT, serve, SETTINGS_URL, SITE, TEMPLATE, world, type World } from './site-preview-world.ts';

const WIDE = { width: 1440, height: 900 };
const PHONE = { width: 360, height: 780 };

async function visit(page: Page, w: World, studio: string | undefined, url = EDITOR_URL) {
	await serve(page, w, new URL(studio ?? '').origin);
	await page.goto(url);
}

async function axe(page: Page) {
	const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa']).analyze();
	return results.violations.map((violation) => `${violation.id}: ${violation.help} (${violation.nodes.map((node) => node.target.join(' ')).join(', ')})`);
}

/** With STUDIO_SHOTS_DIR set, a photograph of the page there as `<name>.png`. */
async function shot(page: Page, name: string) {
	const directory = process.env['STUDIO_SHOTS_DIR'];
	if (directory === undefined || directory === '') return;
	// Animations settle first (the skeleton pulses; spinners turn).
	await page.screenshot({ path: `${directory}/${name}.png`, animations: 'disabled', caret: 'hide' });
}

const pane = (page: Page) => page.getByTestId('preview-pane');
const site = (page: Page) => page.frameLocator('[data-testid="preview-frame"]');
const sync = (page: Page) => page.getByTestId('preview-sync');

async function sideBySide(page: Page) {
	await page.getByTestId('preview-menu').click();
	await page.getByRole('menuitem', { name: /Side by side/ }).click();
}

/** Whether `locator`'s element shows a focus mark (a ring or an outline). */
async function ringed(locator: Locator): Promise<boolean> {
	return locator.evaluate((element) => {
		const style = getComputedStyle(element);
		return (style.boxShadow !== 'none' && style.boxShadow !== '') || (style.outlineStyle !== 'none' && style.outlineWidth !== '0px');
	});
}

test.use({ viewport: WIDE });

test.describe('Settings → Website connections → Preview on your site', () => {
	test('empty, checked as you type, saved, refused in plain words, and turned off', async ({ page, baseURL }) => {
		const w = world({ template: null });
		await visit(page, w, baseURL, SETTINGS_URL);
		const card = page.getByTestId('preview-target');
		await expect(card.getByRole('heading', { name: 'Preview on your site' })).toBeVisible();
		await expect(card.getByTestId('preview-target-state')).toHaveText('Not set up');
		await expect(card).toContainText('Show drafts on your own site before they go live. Your site needs a preview route — the SDK does it in about 10 lines.');
		await shot(page, 'preview-settings-empty-1440-light');

		const address = page.getByLabel('Preview address');
		const problem = page.getByTestId('preview-address-problem');
		await address.fill('https://www.example.com/api/preview');
		await address.blur();
		await expect(problem).toHaveText('Add {code} where your site’s preview route expects the code, like ?code={code}.');
		await expect(address).toHaveAttribute('aria-invalid', 'true');
		await expect(address).toHaveAttribute('aria-describedby', /preview-address-problem/);
		// Checked as you type once touched.
		await address.fill('http://www.example.com/api/preview?code={code}');
		await expect(problem).toHaveText('Use an https:// address. Plain http:// works only for localhost while you develop.');
		await address.fill('https://www.example.com/api/preview?code={code}&t={title}');
		await expect(problem).toHaveText('{title} isn’t a placeholder Colophon fills. Use {code}, {collection}, {id}, {slug} or {locale}.');
		await shot(page, 'preview-settings-problem-1440-light');
		// Nothing is sent while the address is wrong.
		await page.getByTestId('preview-address-save').click();
		expect(w.calls.filter((call) => call.startsWith('PUT'))).toEqual([]);
		await expect(address).toBeFocused();

		await address.fill(TEMPLATE);
		await expect(problem).toHaveCount(0);
		await expect(page.getByTestId('preview-target-summary')).toHaveText('Previews will open on https://site.preview.test.');
		await page.getByTestId('preview-address-save').click();
		await expect(card.getByTestId('preview-target-state')).toHaveText('On');
		await expect(page.getByTestId('preview-target-summary')).toContainText('Previews open on https://site.preview.test');
		expect(w.template).toBe(TEMPLATE);

		// The server's refusal, in its own (plain) words, with the codes behind Technical details.
		await address.fill('https://refuse-me.example.com/preview?code={code}');
		await page.getByTestId('preview-address-save').click();
		await expect(problem).toHaveText('The preview address must point at a public site.');
		await card.getByText('Technical details').click();
		await expect(card.getByTestId('technical-details')).toContainText('HTTP 422 · invalid_preview_url');
		expect(w.template).toBe(TEMPLATE);
		// Typing again clears the refusal.
		await address.fill(TEMPLATE);
		await expect(problem).toHaveCount(0);

		await page.getByTestId('preview-address-clear').click();
		await expect(card.getByTestId('preview-target-state')).toHaveText('Not set up');
		expect(w.template).toBeNull();
	});

	test('a role that doesn’t manage connections reads where previews open, and who to ask', async ({ page, baseURL }) => {
		const w = world({ role: 'editor' });
		await visit(page, w, baseURL, SETTINGS_URL);
		await expect(page.getByTestId('preview-target-readonly')).toContainText('Editors can preview drafts on https://site.preview.test. Only owners and developers change where');
		await expect(page.getByLabel('Preview address')).toHaveCount(0);
		w.template = null;
		await page.reload();
		await expect(page.getByTestId('preview-target-readonly')).toHaveText('Not set up yet. Ask an owner of this project to add your site’s preview address.');
	});
});

test.describe('the Preview button', () => {
	test('not set up: disabled, with the reason and the way to set it up', async ({ page, baseURL }) => {
		await visit(page, world({ template: null }), baseURL);
		const button = page.getByTestId('preview-menu');
		await expect(button).toBeDisabled();
		await expect(page.getByTestId('preview-unavailable')).toContainText('Preview on your site isn’t set up.');
		await expect(button).toHaveAccessibleDescription('Preview on your site isn’t set up. Set it up');
		await expect(page.getByTestId('preview-set-up')).toHaveAttribute('href', `/studio/projects/${PROJECT}/settings/connections#preview`);
		await shot(page, 'preview-not-set-up-1440-light');
	});

	test('not set up, for a role that can’t set it up: ask an owner', async ({ page, baseURL }) => {
		await visit(page, world({ template: null, role: 'editor' }), baseURL);
		await expect(page.getByTestId('preview-menu')).toBeDisabled();
		await expect(page.getByTestId('preview-unavailable')).toHaveText(/Preview on your site isn’t set up: ask an owner\./);
		await expect(page.getByTestId('preview-set-up')).toHaveCount(0);
	});

	test('Open on my site: a new tab with its own grant, first-party', async ({ page, baseURL }) => {
		const w = world();
		await visit(page, w, baseURL);
		await page.getByTestId('preview-menu').click();
		const [tab] = await Promise.all([page.waitForEvent('popup'), page.getByRole('menuitem', { name: /Open on my site/ }).click()]);
		await tab.waitForURL(`${SITE}/**`);
		expect(new URL(tab.url()).searchParams.get('code')).toBe('cpc_grant1');
		await expect(tab.locator('#title')).toHaveText('Hello, preview');
		expect(await tab.evaluate(() => window.opener)).toBeNull();
		await expect(pane(page)).toHaveCount(0);
	});
});

test.describe('side by side', () => {
	test('opening, live, unsaved, updated after the acknowledged save, reloaded, and revoked on close', async ({ page, baseURL }) => {
		const w = world();
		let release = () => {};
		w.hold = new Promise((done) => (release = done));
		await visit(page, w, baseURL);
		await sideBySide(page);

		// Opening: a skeleton the size of the frame, and what is happening.
		await expect(pane(page)).toHaveAttribute('data-state', 'opening');
		await expect(page.getByTestId('preview-opening')).toContainText('Opening a private preview of your site…');
		await expect(page.getByTestId('preview-pill')).toHaveText('Opening…');
		await expect(pane(page)).toBeFocused();
		const before = await page.getByTestId('preview-room').boundingBox();
		await shot(page, 'preview-opening-1440-light');
		release();
		w.hold = null;

		// Live: the site said it is showing the draft.
		await expect(pane(page)).toHaveAttribute('data-state', 'live');
		await expect(page.getByTestId('preview-pill')).toHaveText('Live preview');
		await expect(page.getByTestId('preview-host')).toHaveText('site.preview.test');
		await expect(page.getByTestId('preview-ends')).toHaveText(/^Ends in 1[12] min$/);
		await expect(sync(page)).toHaveText('Shows your saved draft');
		const frame = page.getByTestId('preview-frame');
		await expect(frame).toHaveAttribute('title', 'Preview of Hello, preview on site.preview.test');
		await expect(frame).toHaveAttribute('sandbox', 'allow-scripts allow-same-origin allow-forms allow-popups');
		await expect(frame).toHaveAttribute('referrerpolicy', 'no-referrer');
		await expect(site(page).locator('#title')).toHaveText('Hello, preview');
		// No layout jump between opening and live.
		expect(await page.getByTestId('preview-room').boundingBox()).toEqual(before);
		await expect(page.getByTestId('preview-split')).toHaveAttribute('data-layout', 'side-by-side');
		await shot(page, 'preview-live-1440-light');

		// Typing: the frame keeps the last saved version, and nothing is sent to the site.
		let save = () => {};
		const saved = new Promise<void>((done) => (save = done));
		await page.route(`**/collections/articles/entries/${ENTRY}`, async (route) => {
			if (route.request().method() === 'PUT') await saved;
			await route.fallback();
		});
		await page.getByLabel('Title', { exact: true }).fill('Hello, edited');
		await expect(sync(page)).toHaveText('Shows your last saved version');
		await expect(site(page).locator('#saved')).toHaveText('Showing the draft as last saved.');
		await shot(page, 'preview-unsaved-1440-light');

		// The server acknowledged the save: the site hears which version, at its exact origin.
		save();
		await expect(sync(page)).toHaveText('Updated');
		await expect(site(page).locator('#saved')).toHaveText(`Updated to version rev-2 of ${ENTRY}`);

		// Reload: a fresh one-time code for the same grant, through the site's route.
		await page.getByTestId('preview-reload').click();
		await expect(pane(page)).toHaveAttribute('data-state', 'live');
		await expect(frame).toHaveAttribute('src', /code=cpc_fresh1/);
		expect(w.opened).toEqual(['cpc_grant1', 'cpc_fresh1']);
		expect(w.calls).toContain('POST /previews/grant-1/code');

		// Width presets: a phone's width fits; a desktop's is shown smaller, never cut off.
		await page.getByTitle('Phone, 360 px').click();
		await expect(frame).toHaveCSS('width', '360px');
		await page.getByTitle('Desktop, 1280 px').click();
		await expect(page.getByTestId('preview-scale')).toHaveText(/^1280 px shown at \d+%$/);
		const room = (await page.getByTestId('preview-room').boundingBox())?.width ?? 0;
		expect((await frame.boundingBox())?.width ?? Infinity).toBeLessThanOrEqual(room);
		await page.getByTitle('Fit the pane').click();

		// Closing revokes the grant and gives focus back to the Preview button.
		await page.getByTestId('preview-close').click();
		await expect(pane(page)).toHaveCount(0);
		await expect.poll(() => w.calls).toContain('DELETE /previews/grant-1');
		await expect(page.getByTestId('preview-menu')).toBeFocused();
		expect(w.unanswered).toEqual([]);
	});

	test('the handle drags; leaving the page for another revokes the grant', async ({ page, baseURL }) => {
		const w = world();
		await visit(page, w, baseURL);
		await sideBySide(page);
		await expect(pane(page)).toHaveAttribute('data-state', 'live');
		const handle = page.getByRole('separator', { name: 'Resize the editor and the preview' });
		const box = (await handle.boundingBox()) ?? { x: 0, y: 0, width: 0, height: 0 };
		const editorBefore = (await page.locator(`#entry-${ENTRY}-editor`).boundingBox())?.width ?? 0;
		await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
		await page.mouse.down();
		await page.mouse.move(box.x + box.width / 2 + 160, box.y + box.height / 2, { steps: 8 });
		await page.mouse.up();
		expect(Number(await handle.getAttribute('aria-valuenow'))).toBeGreaterThan(45);
		expect((await page.locator(`#entry-${ENTRY}-editor`).boundingBox())?.width ?? 0).toBeGreaterThan(editorBefore + 100);
		// Away to the collection's list, inside the Studio: the pane goes, and so does its grant.
		await page.getByRole('link', { name: 'Articles' }).first().click();
		await expect(page).toHaveURL(new RegExp(`/projects/${PROJECT}/articles$`));
		await expect.poll(() => w.calls).toContain('DELETE /previews/grant-1');
	});

	test('the cookie kept out of the frame: said, with a new tab that works', async ({ page, baseURL }) => {
		const w = world({ site: 'cookieless' });
		await visit(page, w, baseURL);
		await sideBySide(page);
		await expect(pane(page)).toHaveAttribute('data-state', 'cookie-blocked');
		const banner = page.getByTestId('preview-cookie-blocked');
		await expect(banner).toContainText('Your browser kept the preview cookie out of this frame');
		await expect(banner).toContainText('So your site shows what readers see, not your draft.');
		await expect(page.getByTestId('preview-pill')).toHaveText('Not showing your draft');
		await shot(page, 'preview-cookie-blocked-1440-light');
		// The new tab gets a grant of its own, so the frame's session isn't replaced.
		const [tab] = await Promise.all([page.waitForEvent('popup'), banner.getByRole('button', { name: 'Open in a new tab' }).click()]);
		await tab.waitForURL(`${SITE}/**`);
		expect(new URL(tab.url()).searchParams.get('code')).toBe('cpc_grant2');
		expect(w.calls.filter((call) => call === 'POST /previews')).toHaveLength(2);
		expect(w.calls.filter((call) => call.startsWith('DELETE'))).toEqual([]);
	});

	test('a site that never says it loaded: after 10 seconds, "we can’t tell"; lookalike messages are ignored', async ({ page, baseURL }) => {
		await page.clock.install();
		const w = world({ site: 'impostor' });
		await visit(page, w, baseURL);
		await sideBySide(page);
		// The site loaded, and frames inside it (another origin, and the site's own) sent "ready":
		// neither is the preview frame itself, so the pane is still waiting.
		await expect(site(page).locator('#title')).toHaveText('Hello, preview');
		await expect.poll(() => page.frames().length).toBe(4);
		for (const frame of page.frames()) await frame.waitForLoadState('load');
		await expect(pane(page)).toHaveAttribute('data-state', 'opening');
		await page.clock.fastForward(10_500);
		await expect(pane(page)).toHaveAttribute('data-state', 'unconfirmed');
		const banner = page.getByTestId('preview-unconfirmed');
		await expect(banner).toContainText('We can’t tell whether your site loaded the preview');
		await expect(banner.getByRole('link', { name: 'Check the address' })).toHaveAttribute('href', `/studio/projects/${PROJECT}/settings/connections#preview`);
		await expect(banner.getByRole('button', { name: 'Open in a new tab' })).toBeVisible();
		// The frame is shown anyway: it may well be working.
		await expect(page.getByTestId('preview-frame')).toBeVisible();
		await expect(page.getByTestId('preview-pill')).toHaveText('Not confirmed');
		await shot(page, 'preview-unconfirmed-1440-light');
	});

	test('a save that failed: the preview says it is behind', async ({ page, baseURL }) => {
		const w = world({ save: 'fail' });
		await visit(page, w, baseURL);
		await sideBySide(page);
		await expect(pane(page)).toHaveAttribute('data-state', 'live');
		await page.getByLabel('Title', { exact: true }).fill('Hello, not saved');
		await expect(sync(page)).toHaveText('Preview is behind: your latest changes aren’t saved');
		await expect(site(page).locator('#saved')).toHaveText('Showing the draft as last saved.');
		await shot(page, 'preview-behind-1440-light');
	});

	test('renews itself every few minutes while open', async ({ page, baseURL }) => {
		await page.clock.install();
		const w = world();
		await visit(page, w, baseURL);
		await sideBySide(page);
		await expect(pane(page)).toHaveAttribute('data-state', 'live');
		await expect(page.getByTestId('preview-ends')).toHaveText(/^Ends in 1[12] min$/);
		await page.clock.fastForward('04:10');
		await expect.poll(() => w.calls.filter((call) => call === 'POST /previews/grant-1/renew').length).toBe(1);
		// Renewed to 15 minutes from the server's now; the page's clock ran 4 minutes ahead.
		await expect(page.getByTestId('preview-ends')).toHaveText(/^Ends in 1[01] min$/);
		await expect(pane(page)).toHaveAttribute('data-state', 'live');
	});

	test('ended (renewal refused): "This preview ended", then Start again', async ({ page, baseURL }) => {
		await page.clock.install();
		const w = world({ renew: 'gone', lasts: 2 * 60_000 });
		await visit(page, w, baseURL);
		await sideBySide(page);
		await expect(pane(page)).toHaveAttribute('data-state', 'live');
		await page.clock.fastForward(16_000);
		await expect(pane(page)).toHaveAttribute('data-state', 'ended');
		await expect(page.getByTestId('preview-ended')).toContainText('This preview ended');
		await expect(page.getByTestId('preview-frame')).toHaveCount(0);
		await expect(page.getByTestId('preview-reload')).toBeDisabled();
		await shot(page, 'preview-ended-1440-light');
		w.renew = 'ok';
		await page.getByTestId('preview-start-again').click();
		await expect(pane(page)).toHaveAttribute('data-state', 'live');
		expect(w.calls.filter((call) => call === 'POST /previews')).toHaveLength(2);
	});

	test('the site couldn’t be opened: plain words, the codes behind Technical details, and Try again', async ({ page, baseURL }) => {
		const w = world({ issue: 'unavailable' });
		await visit(page, w, baseURL);
		await sideBySide(page);
		await expect(pane(page)).toHaveAttribute('data-state', 'failed');
		const failed = page.getByTestId('preview-failed');
		await expect(failed.getByRole('heading')).toHaveText('The preview didn’t open');
		await expect(failed).toContainText('Colophon couldn’t start the preview just now. Try again in a moment.');
		await failed.getByText('Technical details').click();
		await expect(failed.getByTestId('technical-details')).toContainText('HTTP 503 · unavailable');
		await shot(page, 'preview-failed-1440-light');
		w.issue = 'grant';
		await page.getByTestId('preview-retry').click();
		await expect(pane(page)).toHaveAttribute('data-state', 'live');
	});

	test('other refusals: too many previews, an address that no longer works, and not set up after all', async ({ page, baseURL }) => {
		const w = world({ issue: 'limit' });
		await visit(page, w, baseURL);
		await sideBySide(page);
		await expect(page.getByTestId('preview-failed')).toContainText('Too many previews open');
		w.issue = 'invalid';
		await page.getByTestId('preview-retry').click();
		await expect(page.getByTestId('preview-failed')).toContainText('The preview address no longer works. Check it in Settings, under Website connections.');
		w.issue = 'not-configured';
		await page.getByTestId('preview-retry').click();
		await expect(pane(page)).toHaveAttribute('data-state', 'not-set-up');
		await expect(page.getByTestId('preview-not-set-up').getByRole('link', { name: 'Set it up' })).toHaveAttribute('href', /settings\/connections#preview$/);
	});
});

test('keyboard only: open, resize, change width, reload and close, with focus always shown', async ({ page, baseURL }) => {
	const w = world();
	await visit(page, w, baseURL);
	await page.getByRole('tab', { name: 'Content' }).focus();
	await page.keyboard.press('Tab');
	const button = page.getByTestId('preview-menu');
	await expect(button).toBeFocused();
	expect(await ringed(button)).toBe(true);
	await page.keyboard.press('Enter');
	await expect(page.getByRole('menuitem', { name: /Side by side/ })).toBeFocused();
	await page.keyboard.press('Enter');
	await expect(pane(page)).toBeFocused();
	await expect(pane(page)).toHaveAttribute('data-state', 'live');

	// Back one stop: the handle between the editor and the preview, a separator with arrow keys.
	await page.keyboard.press('Shift+Tab');
	const handle = page.getByRole('separator', { name: 'Resize the editor and the preview' });
	await expect(handle).toBeFocused();
	expect(await ringed(handle)).toBe(true);
	await expect(handle).toHaveAttribute('aria-valuenow', '45');
	await page.keyboard.press('ArrowLeft');
	await expect(handle).toHaveAttribute('aria-valuenow', '40');
	await page.keyboard.press('End');
	await expect(handle).toHaveAttribute('aria-valuenow', '70');
	await page.keyboard.press('Home');
	await expect(handle).toHaveAttribute('aria-valuenow', '30');
	await page.keyboard.press('ArrowRight');
	await expect(handle).toHaveAttribute('aria-valuetext', 'Editor 35%, preview 65%');

	// Into the pane: Reload, a new tab, Close, then the widths.
	await page.keyboard.press('Tab');
	const reload = page.getByTestId('preview-reload');
	await expect(reload).toBeFocused();
	expect(await ringed(reload)).toBe(true);
	await page.keyboard.press('Enter');
	await expect.poll(() => w.calls).toContain('POST /previews/grant-1/code');
	await expect(pane(page)).toHaveAttribute('data-state', 'live');
	await page.keyboard.press('Tab');
	await page.keyboard.press('Tab');
	await expect(page.getByTestId('preview-close')).toBeFocused();
	await page.keyboard.press('Tab');
	// The widths: Tab reaches the chosen one; arrow keys choose.
	const fit = page.getByRole('radio', { name: 'Fit the pane' });
	await expect(fit).toBeFocused();
	expect(await ringed(page.getByTestId('preview-widths').locator('label').filter({ has: fit }))).toBe(true);
	await page.keyboard.press('ArrowLeft');
	await expect(page.getByRole('radio', { name: /Desktop, 1280/ })).toBeChecked();
	await page.keyboard.press('ArrowLeft');
	await page.keyboard.press('ArrowLeft');
	await expect(page.getByRole('radio', { name: /Phone, 360/ })).toBeChecked();
	await expect(page.getByTestId('preview-frame')).toHaveCSS('width', '360px');

	// Close from the keyboard: revoked, and focus goes back to the Preview button.
	await page.keyboard.press('Shift+Tab');
	await expect(page.getByTestId('preview-close')).toBeFocused();
	await page.keyboard.press('Enter');
	await expect(pane(page)).toHaveCount(0);
	await expect(button).toBeFocused();
	await expect.poll(() => w.calls).toContain('DELETE /previews/grant-1');
});

test.describe('on a phone', () => {
	test.use({ viewport: PHONE, isMobile: true, hasTouch: true });

	test('side by side becomes two tabs, Edit and Preview, with no sideways scrolling', async ({ page, baseURL }) => {
		const w = world();
		await visit(page, w, baseURL);
		await page.getByTestId('preview-menu').click();
		await page.getByRole('menuitem', { name: /In this page/ }).click();
		await expect(page.getByTestId('preview-split')).toHaveAttribute('data-layout', 'tabs');
		const tabs = page.getByRole('tablist', { name: 'Edit or preview' });
		await expect(tabs.getByRole('tab', { name: 'Preview' })).toHaveAttribute('aria-selected', 'true');
		await expect(pane(page)).toHaveAttribute('data-state', 'live');
		await expect(pane(page)).toBeVisible();
		await expect(page.getByLabel('Title', { exact: true })).toBeHidden();
		await expect(page.getByRole('separator')).toHaveCount(0);
		expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(PHONE.width);
		await shot(page, 'preview-tabs-360-light');

		await tabs.getByRole('tab', { name: 'Edit' }).click();
		await expect(page.getByLabel('Title', { exact: true })).toBeVisible();
		await expect(pane(page)).toBeHidden();
		// The frame stays loaded behind its tab: a save still reaches the site.
		await page.getByLabel('Title', { exact: true }).fill('Hello from a phone');
		await page.getByRole('tab', { name: 'Edit' }).focus();
		await page.keyboard.press('ArrowRight');
		await expect(tabs.getByRole('tab', { name: 'Preview' })).toBeFocused();
		await expect(pane(page)).toBeVisible();
		await expect(site(page).locator('#saved')).toHaveText(`Updated to version rev-2 of ${ENTRY}`);
		expect(w.calls.filter((call) => call === 'POST /previews')).toHaveLength(1);
	});
});

test.describe('accessibility', () => {
	for (const scheme of ['light', 'dark'] as const) {
		test(`no violations, ${scheme}: the pane live, refused, and the cookie note; the Settings card with a problem`, async ({ page, baseURL }) => {
			await page.emulateMedia({ colorScheme: scheme });
			const w = world();
			await visit(page, w, baseURL);
			await sideBySide(page);
			await expect(pane(page)).toHaveAttribute('data-state', 'live');
			expect(await axe(page), 'live').toEqual([]);

			await page.getByTestId('preview-close').click();
			w.site = 'cookieless';
			await sideBySide(page);
			await expect(pane(page)).toHaveAttribute('data-state', 'cookie-blocked');
			expect(await axe(page), 'cookie blocked').toEqual([]);

			await page.getByTestId('preview-close').click();
			w.issue = 'unavailable';
			await sideBySide(page);
			await expect(pane(page)).toHaveAttribute('data-state', 'failed');
			expect(await axe(page), 'failed').toEqual([]);

			await page.goto(SETTINGS_URL);
			await page.getByLabel('Preview address').fill('https://www.example.com/no-code');
			await page.getByLabel('Preview address').blur();
			await expect(page.getByTestId('preview-address-problem')).toBeVisible();
			expect(await axe(page), 'settings').toEqual([]);
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
				const w = world();
				await visit(page, w, baseURL);
				await page.getByTestId('preview-menu').click();
				await page.getByRole('menuitem', { name: size === PHONE ? /In this page/ : /Side by side/ }).click();
				await expect(pane(page)).toHaveAttribute('data-state', 'live');
				await expect(site(page).locator('#title')).toHaveText('Hello, preview');
				await shot(page, `preview-${size === PHONE ? 'tabs' : 'side-by-side'}-${size.width}-${scheme}`);

				w.template = null;
				await page.goto(SETTINGS_URL);
				await page.getByLabel('Preview address').fill(TEMPLATE);
				await expect(page.getByTestId('preview-target-summary')).toContainText('Previews will open on');
				await page.getByTestId('preview-target').scrollIntoViewIfNeeded();
				await shot(page, `preview-settings-${size.width}-${scheme}`);
			});
		}
	}
});
