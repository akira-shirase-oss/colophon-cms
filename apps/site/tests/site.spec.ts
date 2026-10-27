// The public website (apps/site), prerendered, in Chromium: every page renders without console
// errors, has one h1 and passes axe (WCAG 2.1 AA) in light and dark; nothing is requested from
// another origin; the blog has every post and the changelog every entry of their sources.
import AxeBuilder from '@axe-core/playwright';
import { expect, type Page, test } from '@playwright/test';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { PAGES } from './pages.ts';

const here = path.dirname(fileURLToPath(import.meta.url));
const BLOG = path.resolve(here, '../src/content/blog');
const CHANGELOG = path.resolve(here, '../../../CHANGELOG.md');
// The build log's own source, when this checkout sits next to it (the maintainer's machine).
const BUILD_LOG_SOURCE = path.resolve(here, '../../../../../minimal-portfolio/content/colophon');

const posts = readdirSync(BLOG).filter((name) => name.endsWith('.md'));

async function axe(page: Page) {
	const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa']).analyze();
	return results.violations.map(
		(violation) => `${violation.id}: ${violation.help} (${violation.nodes.map((node) => node.target.join(' ')).join(', ')})`
	);
}

/** Console errors and uncaught exceptions while the page is open. */
function watchErrors(page: Page, allow: RegExp[] = []) {
	const errors: string[] = [];
	page.on('console', (message) => {
		if (message.type() === 'error' && !allow.some((pattern) => pattern.test(message.text()))) errors.push(message.text());
	});
	page.on('pageerror', (error) => errors.push(error.message));
	return errors;
}

for (const scheme of ['light', 'dark'] as const) {
	for (const { path: route, name } of PAGES) {
		test(`${name} (${route}) renders cleanly, with one h1, and passes axe in ${scheme}`, async ({ page }) => {
			await page.emulateMedia({ colorScheme: scheme });
			// The 404 page is answered with a 404 status, which the browser logs; nothing else may be.
			const errors = watchErrors(page, name === '404' ? [/status of 404/] : []);
			const response = await page.goto(route);
			expect(response?.status()).toBe(name === '404' ? 404 : 200);
			await page.waitForLoadState('networkidle');
			await expect(page.locator('html')).toHaveClass(scheme === 'dark' ? /\bdark\b/ : /^(?!.*\bdark\b)/);
			await expect(page.locator('h1')).toHaveCount(1);
			await expect(page).toHaveTitle(/\S/);
			expect(await axe(page), `axe on ${route} in ${scheme}`).toEqual([]);
			expect(errors, `console errors on ${route}`).toEqual([]);
		});
	}
}

test('no page requests anything from another origin', async ({ page, baseURL }) => {
	const foreign: string[] = [];
	page.on('request', (request) => {
		const url = request.url();
		if (!url.startsWith(baseURL ?? '') && !url.startsWith('data:')) foreign.push(url);
	});
	for (const { path: route } of PAGES) {
		await page.goto(route);
		await page.waitForLoadState('networkidle');
	}
	// Client-side navigation too: the blog and a post through the router.
	await page.goto('/');
	await page.getByRole('navigation', { name: 'Main' }).getByRole('link', { name: 'Blog' }).click();
	await expect(page).toHaveURL(/\/blog\/$/);
	await page.waitForLoadState('networkidle');
	expect(foreign).toEqual([]);
	// One typeface, self-hosted.
	expect(await page.evaluate(() => getComputedStyle(document.body).fontFamily)).toContain('Inter Variable');
});

test('the blog lists every post in src/content/blog, and every post has its page and canonical original', async ({ page, request }) => {
	await page.goto('/blog/');
	await expect(page.getByTestId('blog-post')).toHaveCount(posts.length);
	if (existsSync(BUILD_LOG_SOURCE)) {
		const source = readdirSync(BUILD_LOG_SOURCE).filter((name) => name.endsWith('.md') && name !== '_index.md');
		expect(posts.sort(), 'the copies match the build log (scripts/import-blog.ts)').toEqual(source.sort());
		for (const name of source) {
			expect(readFileSync(path.join(BLOG, name), 'utf8'), `${name} is unchanged`).toBe(readFileSync(path.join(BUILD_LOG_SOURCE, name), 'utf8'));
		}
	}
	for (const name of posts) {
		const slug = name.replace(/\.md$/, '');
		const response = await request.get(`/blog/${slug}/`);
		expect(response.status(), slug).toBe(200);
		const html = await response.text();
		expect(html, `${slug} names its original`).toContain(`<link rel="canonical" href="https://404human.dev/colophon/${slug}/"`);
		expect(html.match(/<h1[\s>]/g)?.length, `${slug} has one h1`).toBe(1);
	}
});

test('the changelog shows every entry of CHANGELOG.md, in order', async ({ page }) => {
	const source = readFileSync(CHANGELOG, 'utf8');
	const titles = [...source.matchAll(/^### (.+)$/gm)].map((match) => (match[1] ?? '').split(' · ').slice(1).join(' · '));
	expect(titles.length).toBeGreaterThan(20);
	await page.goto('/changelog/');
	await expect(page.getByTestId('changelog-entry')).toHaveCount(titles.length);
	expect(await page.getByTestId('changelog-title').allTextContents()).toEqual(titles);
	// Every merged step on main is in the file (git log --merges main, "Merge P<n> step <x.y>").
	const merged = [...source.matchAll(/^- \*\*Merged:\*\* \d{4}-\d{2}-\d{2} · (.+)$/gm)];
	expect(merged.length).toBe(titles.length - [...source.matchAll(/^- \*\*Status:\*\*/gm)].length);
});

test('the feed and the sitemap are prerendered', async ({ request }) => {
	const feed = await request.get('/blog/rss.xml');
	expect(feed.status()).toBe(200);
	const xml = await feed.text();
	expect(xml.match(/<item>/g)?.length).toBe(posts.length);
	for (const name of posts) expect(xml).toContain(`<link>https://404human.dev/colophon/${name.replace(/\.md$/, '')}/</link>`);

	const sitemap = await (await request.get('/sitemap.xml')).text();
	for (const { path: route, name } of PAGES) {
		if (name === '404' || name === 'blog-post') continue;
		expect(sitemap).toContain(`${route}</loc>`);
	}
	expect(await (await request.get('/robots.txt')).text()).toContain('Sitemap: ');
});

test('every feature says done, building or planned', async ({ page }) => {
	await page.goto('/product/');
	const features = page.locator('[data-status]').filter({ has: page.locator('h3') });
	expect(await features.count()).toBeGreaterThan(30);
	for (const status of await features.evaluateAll((items) => items.map((item) => item.getAttribute('data-status')))) {
		expect(['done', 'building', 'planned']).toContain(status);
	}
	await expect(page.getByText('Planned').first()).toBeVisible();
});

test('the licence is described as proposed, not confirmed', async ({ page }) => {
	await page.goto('/open-source/');
	const main = page.getByRole('main');
	await expect(main.getByText(/Apache-2\.0 is proposed for the core/)).toBeVisible();
	await expect(main.getByText('not yet confirmed', { exact: true })).toBeVisible();
	await expect(main.getByText('Open decision O-014')).toBeVisible();
	// The footer says the same on every page.
	await expect(page.getByRole('contentinfo').getByText(/Apache-2\.0 is proposed, not yet confirmed/)).toBeVisible();
});

test.describe('with motion', () => {
	test.use({ reducedMotion: 'no-preference' });
	test('the appearance choice changes the theme and survives a reload', async ({ page }) => {
		await page.emulateMedia({ colorScheme: 'light' });
		await page.goto('/');
		await page.getByRole('button', { name: 'Appearance' }).click();
		await page.getByRole('menuitemradio', { name: 'Dark' }).click();
		await expect(page.locator('html')).toHaveClass(/\bdark\b/);
		await page.reload();
		await expect(page.locator('html')).toHaveClass(/\bdark\b/);
		await page.getByRole('button', { name: 'Appearance' }).click();
		await page.getByRole('menuitemradio', { name: 'System' }).click();
		await expect(page.locator('html')).not.toHaveClass(/\bdark\b/);
	});
});

test('on a phone, the menu opens, passes axe and navigates', async ({ page }) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await page.goto('/');
	// No sideways scrolling at phone width.
	expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
	await page.getByRole('button', { name: 'Menu' }).click();
	const menu = page.getByRole('dialog');
	await expect(menu).toBeVisible();
	expect(await axe(page), 'axe with the menu open').toEqual([]);
	await menu.getByRole('link', { name: 'Roadmap' }).click();
	await expect(page).toHaveURL(/\/roadmap\/$/);
	await expect(page.getByRole('heading', { level: 1 })).toContainText('milestones');
});

test('pages fit a 320px screen without sideways scrolling', async ({ page }) => {
	await page.setViewportSize({ width: 320, height: 720 });
	for (const { path: route } of PAGES) {
		await page.goto(route);
		expect(await page.evaluate(() => document.documentElement.scrollWidth), route).toBeLessThanOrEqual(320);
	}
});
