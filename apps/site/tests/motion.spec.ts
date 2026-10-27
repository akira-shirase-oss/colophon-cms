// The live parts of the site: the hero's story, the bento graphics, the data flow, the API panes
// and the charts. Under reduced motion everything shows its final frame and nothing animates;
// with motion, the story plays to its end and replays, axe still passes, and the controls in the
// graphics work from the keyboard.
import AxeBuilder from '@axe-core/playwright';
import { expect, type Page, test } from '@playwright/test';

async function scrollThrough(page: Page) {
	const height = await page.evaluate(() => document.documentElement.scrollHeight);
	for (let y = 0; y < height; y += 500) {
		await page.evaluate((top) => window.scrollTo(0, top), y);
		await page.waitForTimeout(40);
	}
}

function watchErrors(page: Page) {
	const errors: string[] = [];
	page.on('console', (message) => message.type() === 'error' && errors.push(message.text()));
	page.on('pageerror', (error) => errors.push(error.message));
	return errors;
}

/** CSS animations and transitions that are running or about to. */
const moving = (page: Page) =>
	page.evaluate(() =>
		document
			.getAnimations()
			.filter((animation) => animation.playState === 'running' || animation.pending)
			.map((animation) => {
				const target = (animation.effect as KeyframeEffect | null)?.target;
				return `${(animation as CSSAnimation).animationName ?? 'transition'} on ${target instanceof Element ? target.className : '?'}`;
			})
	);

test.describe('reduced motion', () => {
	test.use({ reducedMotion: 'reduce' });

	for (const path of ['/', '/product/', '/compare/']) {
		test(`${path}: final frames only, nothing animates`, async ({ page }) => {
			const errors = watchErrors(page);
			await page.goto(path);
			await scrollThrough(page);
			await page.evaluate(() => window.scrollTo(0, 0));
			await page.waitForTimeout(300);
			// No graphic was switched on, and nothing is moving.
			await expect(page.locator('[data-run]')).toHaveCount(0);
			expect(await moving(page)).toEqual([]);
			expect(errors).toEqual([]);
		});
	}

	test('the hero shows the end of its story', async ({ page }) => {
		await page.goto('/');
		const story = page.locator('[data-story]');
		await expect(story).toHaveAttribute('data-phase', 'final');
		const visual = story.locator('[aria-hidden="true"]').first();
		await expect(visual).toContainText('Spring menu');
		for (const word of ['Saved · version 7', 'Approved', 'Verified at 1 location', 'sha256:9f2c…e41a']) {
			const piece = visual.getByText(word, { exact: false }).first();
			await expect(piece).toBeVisible();
			expect(Number(await piece.evaluate((element) => getComputedStyle(element.closest('[data-on]') ?? element).opacity))).toBe(1);
		}
		// The words for people who can't see the picture.
		await expect(story.locator('p.sr-only')).toContainText('Verified at 1 location');
	});
});

test.describe('with motion', () => {
	test.use({ reducedMotion: 'no-preference' });

	test('the hero plays its story to the end, rests, and replays', async ({ page }) => {
		const errors = watchErrors(page);
		await page.goto('/');
		const story = page.locator('[data-story]');
		await story.scrollIntoViewIfNeeded();
		await expect(story).toHaveAttribute('data-started');
		await expect(story).not.toHaveAttribute('data-phase', 'final');
		await expect(story).toHaveAttribute('data-phase', 'final', { timeout: 10_000 });
		const replay = page.getByRole('button', { name: 'Replay the illustration' });
		await expect(replay).toBeVisible();
		await replay.click();
		await expect(story).not.toHaveAttribute('data-phase', 'final');
		await expect(story).toHaveAttribute('data-phase', 'final', { timeout: 10_000 });
		expect(errors).toEqual([]);
	});

	for (const scheme of ['light', 'dark'] as const) {
		test(`the home page passes axe in ${scheme} with its graphics running`, async ({ page }) => {
			const errors = watchErrors(page);
			await page.emulateMedia({ colorScheme: scheme });
			await page.goto('/');
			await page.locator('[data-story]').scrollIntoViewIfNeeded();
			await expect(page.locator('[data-story]')).toHaveAttribute('data-phase', 'final', { timeout: 10_000 });
			await scrollThrough(page);
			await page.waitForTimeout(1500);
			// Graphics switch on as they come into view.
			expect(await page.locator('[data-run]').count()).toBeGreaterThan(8);
			// Looping decorative pictures are checked at rest (reduced motion, in site.spec.ts); here,
			// everything a reader reads while they move.
			const results = await new AxeBuilder({ page })
				.withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'])
				.exclude('.stage-dots')
				.exclude('.flow')
				.analyze();
			expect(results.violations.map((violation) => `${violation.id}: ${violation.nodes.map((node) => node.target.join(' ')).join(', ')}`)).toEqual([]);
			expect(errors).toEqual([]);
		});
	}

	test('a graphic pauses when it leaves the screen', async ({ page }) => {
		await page.goto('/');
		const card = page.getByTestId('bento-card').first();
		await card.scrollIntoViewIfNeeded();
		await expect(card).toHaveAttribute('data-run', 'on');
		await page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight));
		await expect(card).toHaveAttribute('data-run', 'off');
		const states = await card.evaluate((element) =>
			element.getAnimations({ subtree: true }).map((animation) => animation.playState)
		);
		expect(states.length).toBeGreaterThan(0);
		expect(states.every((state) => state === 'paused' || state === 'finished')).toBe(true);
	});
});

test('every bento card has a status and words for its picture', async ({ page }) => {
	await page.goto('/');
	const cards = page.getByTestId('bento-card');
	await expect(cards).toHaveCount(8);
	for (const card of await cards.all()) {
		await expect(card.locator('[data-status]')).toHaveCount(1);
		const interactive = (await card.locator('input[type=range]').count()) > 0;
		if (!interactive) await expect(card.locator('.sr-only', { hasText: 'Illustration:' })).toHaveCount(1);
	}
});

test('the release slider works from the keyboard', async ({ page }) => {
	await page.goto('/');
	const slider = page.getByRole('slider', { name: /Viewing release/ });
	await slider.focus();
	await page.keyboard.press('End');
	await expect(page.getByText('Viewing release 42')).toBeVisible();
	await page.keyboard.press('ArrowLeft');
	await expect(page.getByText('Viewing release 41')).toBeVisible();
	await expect(page.getByText('?generation=41')).toBeVisible();
});

test('the API panes are tabs: arrow keys move between them, and each shows its answer', async ({ page }) => {
	await page.goto('/');
	const read = page.getByRole('tab', { name: 'Read' });
	await read.click();
	await expect(page.getByRole('tabpanel')).toContainText('colophon-generation: 42');
	await page.keyboard.press('End');
	await expect(page.getByRole('tab', { name: /Slug taken/ })).toBeFocused();
	await expect(page.getByRole('tabpanel')).toContainText('slug_taken');
	await expect(page.getByRole('tabpanel')).toContainText('Building');
	await page.getByRole('tab', { name: 'Stale save' }).click();
	await expect(page.getByRole('tabpanel')).toContainText('"reason": "stale_revision"');
	await expect(page.getByRole('tabpanel')).toContainText('HTTP/1.1 409 Conflict');
});

test('the comparison backlog filters by status', async ({ page }) => {
	await page.goto('/compare/');
	const rows = page.getByTestId('parity').locator('tbody tr');
	const all = await rows.count();
	await page.getByRole('button', { name: /^Planned/ }).click();
	await expect(page.getByRole('button', { name: /^Planned/ })).toHaveAttribute('aria-pressed', 'true');
	const planned = await rows.count();
	expect(planned).toBeGreaterThan(0);
	expect(planned).toBeLessThan(all);
	for (const kind of await rows.evaluateAll((items) => items.map((item) => item.getAttribute('data-kind')))) expect(kind).toBe('planned');
	await page.getByRole('button', { name: /^All/ }).click();
	await expect(rows).toHaveCount(all);
});
