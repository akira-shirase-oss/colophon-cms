// The look taken from the reference layout (docs/modules/20-site.md, "Look"): the home page's
// sections in the reference's order, its measured sizes, the motion (word-by-word headline, the
// window that stands up as the page scrolls, sections that rise in once, the bar that turns into a
// pill) and the controls it brings (the showcase's tabs, the milestones' switch). Under reduced
// motion every one of them sits on its final frame.
import { expect, type Page, test } from '@playwright/test';

const SECTIONS = ['hero-title', 'built-on', 'devices', 'writing', 'publishing', 'promises', 'question', 'where', 'journal', 'loop', 'footer-start'];

/** Every section heading's top, in document order. */
/** Whether an element's transform leaves it flat and in place (a bare perspective() changes nothing). */
const flat = (page: Page, selector: string) =>
	page.locator(selector).evaluate((element) => {
		const m = new DOMMatrix(getComputedStyle(element).transform);
		return [m.m11, m.m12, m.m21, m.m22, m.m41, m.m42, m.m23, m.m32].map((value) => Math.round(value * 1000) / 1000);
	});
const FLAT = [1, 0, 0, 1, 0, 0, 0, 0];

const tops = (page: Page) =>
	page.evaluate((ids) => ids.map((id) => document.getElementById(id)?.getBoundingClientRect().top ?? Number.NaN), SECTIONS);

test('the home page has the reference’s sections, in its order', async ({ page }) => {
	await page.goto('/');
	const positions = await tops(page);
	expect(positions.every((top) => Number.isFinite(top)), 'every section is there').toBe(true);
	expect([...positions].sort((a, b) => a - b)).toEqual(positions);
});

test('measurements match the reference at 1,440px', async ({ page }) => {
	await page.setViewportSize({ width: 1440, height: 900 });
	await page.goto('/');
	const m = await page.evaluate(() => {
		const box = (selector: string) => document.querySelector(selector)!.getBoundingClientRect();
		const style = (selector: string) => getComputedStyle(document.querySelector(selector)!);
		return {
			h1: style('#hero-title').fontSize,
			h1Top: box('#hero-title').top,
			h2: style('#devices').fontSize,
			hero: box('section[aria-labelledby="hero-title"]').height,
			showcase: [box('[data-testid="device-showcase"]').width, box('[data-testid="device-showcase"]').height],
			card: style('[data-testid="bento-card"]').borderTopLeftRadius,
			pill: box('section[aria-labelledby="hero-title"] .pill').height,
			eyebrow: style('.t-eyebrow').fontSize
		};
	});
	expect(m.h1).toBe('76px');
	expect(m.h1Top).toBe(160);
	expect(m.h2).toBe('52px');
	expect(m.hero).toBe(1020);
	expect(m.showcase).toEqual([1072, 700]);
	expect(m.card).toBe('24px');
	expect(m.pill).toBe(55);
	expect(m.eyebrow).toBe('15px');
});

test('the build log’s quotes link to the posts they were copied from', async ({ page }) => {
	await page.goto('/');
	const quotes = page.getByRole('list', { name: /From the build log/ }).getByRole('link');
	expect(await quotes.count()).toBeGreaterThanOrEqual(3);
	const first = quotes.first();
	const text = (await first.locator('q').textContent())?.trim() ?? '';
	await first.click();
	await expect(page).toHaveURL(/\/blog\/[^/]+\/$/);
	// The first words, before any apostrophe (posts keep straight ones; the cards print curly ones).
	await expect(page.locator('main')).toContainText(text.split(/['’]/)[0]!.slice(0, 30));
});

test('fonts: Inter for text and Fragment Mono for code, both self-hosted', async ({ page }) => {
	await page.goto('/blog/checkpoint-3-4/');
	expect(await page.evaluate(() => getComputedStyle(document.body).fontFamily)).toContain('Inter Variable');
	expect(await page.locator('.prose-site code').first().evaluate((element) => getComputedStyle(element).fontFamily)).toContain('Fragment Mono');
	await page.waitForLoadState('networkidle');
	expect(await page.evaluate(() => [...document.fonts].some((face) => face.family.replaceAll('"', '') === 'Fragment Mono' && face.status === 'loaded'))).toBe(true);
});

test('the showcase’s tabs switch pictures from the keyboard', async ({ page }) => {
	await page.goto('/');
	const tabs = page.getByRole('tablist', { name: 'Where to look' });
	const studio = tabs.getByRole('tab', { name: 'Studio' });
	await studio.focus();
	await expect(studio).toHaveAttribute('aria-selected', 'true');
	await page.keyboard.press('ArrowRight');
	const site = tabs.getByRole('tab', { name: 'Your site' });
	await expect(site).toBeFocused();
	await expect(site).toHaveAttribute('aria-selected', 'true');
	const panel = page.locator(`#${await site.getAttribute('aria-controls')}`);
	await expect(panel).toContainText('colophon-generation 42');
	expect(await panel.evaluate((element) => (element as HTMLElement).inert)).toBe(false);
	expect(await page.locator(`#${await studio.getAttribute('aria-controls')}`).evaluate((element) => (element as HTMLElement).inert)).toBe(true);
});

test('the milestones’ switch shows now or next', async ({ page }) => {
	await page.goto('/');
	const group = page.getByRole('group', { name: 'Show the milestone' });
	const now = group.getByRole('button', { name: 'Now' });
	const next = group.getByRole('button', { name: 'Next' });
	await expect(now).toHaveAttribute('aria-pressed', 'true');
	await expect(page.getByText('Milestone P3')).toBeVisible();
	await next.focus();
	await page.keyboard.press('Enter');
	await expect(next).toHaveAttribute('aria-pressed', 'true');
	await expect(page.getByText('Milestone R')).toBeVisible();
	await expect(page.getByText('There are no prices')).toBeVisible();
});

test.describe('with motion', () => {
	test.use({ reducedMotion: 'no-preference' });

	test('the headline arrives word by word and rests; the window stands up as the page scrolls', async ({ page }) => {
		await page.setViewportSize({ width: 1440, height: 900 });
		await page.goto('/');
		const words = page.locator('#hero-title .word-in');
		expect(await words.count()).toBeGreaterThan(4);
		const names = await words.evaluateAll((items) => items.map((item) => item.getAnimations().map((a) => (a as CSSAnimation).animationName)));
		expect(names.flat()).toContain('word-in');
		await page.waitForTimeout(1800);
		for (const word of await words.all()) expect(Number(await word.evaluate((element) => getComputedStyle(element).opacity))).toBe(1);

		const tilt = () => page.locator('.window').evaluate((element) => Number(getComputedStyle(element).getPropertyValue('--tilt')));
		await page.waitForTimeout(400);
		expect(await tilt()).toBeGreaterThan(0.3);
		await page.evaluate(() => window.scrollTo(0, 800));
		await expect.poll(tilt).toBe(0);
	});

	test('a section below the fold rises in once, and ends on its resting frame', async ({ page }) => {
		await page.goto('/');
		const section = page.locator('section[aria-labelledby="promises"]');
		await expect(section).toHaveAttribute('data-reveal', 'armed');
		expect(Number(await section.evaluate((element) => getComputedStyle(element).opacity))).toBe(0);
		await section.scrollIntoViewIfNeeded();
		await expect(section).toHaveAttribute('data-reveal', 'shown');
		await expect.poll(() => section.evaluate((element) => getComputedStyle(element).opacity)).toBe('1');
		await expect.poll(() => section.evaluate((element) => getComputedStyle(element).transform)).toMatch(/^(none|matrix\(1, 0, 0, 1, 0, 0\))$/);
	});

	test('the bar turns into a pill once the page scrolls', async ({ page }) => {
		await page.setViewportSize({ width: 1440, height: 900 });
		await page.goto('/');
		const header = page.locator('.site-header');
		const ground = header.locator('.pill-ground');
		await expect(header).not.toHaveAttribute('data-scrolled');
		expect(await ground.evaluate((element) => getComputedStyle(element).opacity)).toBe('0');
		await page.evaluate(() => window.scrollTo(0, 600));
		await expect(header).toHaveAttribute('data-scrolled');
		await expect.poll(() => ground.evaluate((element) => getComputedStyle(element).opacity)).toBe('1');
	});

	test('the strip of projects drifts while on screen', async ({ page }) => {
		await page.goto('/');
		const track = page.locator('section[aria-labelledby="built-on"] .track');
		await track.scrollIntoViewIfNeeded();
		const x = () => track.evaluate((element) => new DOMMatrix(getComputedStyle(element).transform).m41);
		const before = await x();
		await page.waitForTimeout(600);
		expect(await x()).toBeLessThan(before - 10);
	});
});

test.describe('reduced motion', () => {
	test.use({ reducedMotion: 'reduce' });

	test('every new piece sits on its final frame', async ({ page }) => {
		await page.goto('/');
		for (let y = 0; y < (await page.evaluate(() => document.documentElement.scrollHeight)); y += 600) {
			await page.evaluate((top) => window.scrollTo(0, top), y);
			await page.waitForTimeout(30);
		}
		await expect(page.locator('[data-reveal]')).toHaveCount(0);
		expect(await flat(page, '.window')).toEqual(FLAT);
		for (const word of await page.locator('#hero-title .word-in').all()) {
			expect(await word.evaluate((element) => [getComputedStyle(element).opacity, getComputedStyle(element).filter])).toEqual(['1', 'none']);
		}
		expect(await page.locator('section[aria-labelledby="built-on"] .track').evaluate((element) => getComputedStyle(element).transform)).toBe('none');
	});
});

test.describe('without JavaScript', () => {
	test.use({ javaScriptEnabled: false });

	test('the home page shows its final frames', async ({ page }) => {
		await page.goto('/');
		await expect(page.locator('#hero-title')).toBeVisible();
		await expect(page.locator('[data-reveal]')).toHaveCount(0);
		for (const word of await page.locator('#hero-title .word-in').all()) {
			expect(await word.evaluate((element) => getComputedStyle(element).opacity)).toBe('1');
		}
		expect(await flat(page, '.window')).toEqual(FLAT);
		await expect(page.locator('section[aria-labelledby="promises"]')).toBeVisible();
	});
});
