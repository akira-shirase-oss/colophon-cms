import { mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, test } from 'vitest';

import { BuildFailed, build } from '../src/build.ts';
import { marker } from '../src/pages.ts';
import { api, articles, item, site } from './colophon.ts';

const settings = { api: 'http://colophon.test', project: 'p1', environment: 'production' };

describe('a build', () => {
	test('reads one release, page after page, and marks every page with it', async () => {
		const root = await mkdtemp(join(tmpdir(), 'site-'));
		const list = [item('a3', 'Third <b>'), item('a2', 'Second'), item('a1', 'First')];
		const colophon = articles(new Map([[6, [item('old', 'Older')]], [7, list]]), 2);
		const built = await build({ ...settings, root, fetch: colophon.fetch, now: new Date('2026-10-26T12:00:00Z') });
		expect(built.generation).toBe(7);
		// Pages first (its answer pins release 7), then articles' two pages, then the menu and theme —
		// every later read already pinned.
		expect(colophon.asked.map((url) => url.search)).toEqual([
			'?limit=100',
			'?limit=100&generation=7',
			'?limit=100&cursor=7%3A2',
			'?limit=100&generation=7',
			'?limit=100&generation=7'
		]);
		expect(built.pages.sort()).toEqual(['/404.html', '/articles/a1/index.html', '/articles/a2/index.html', '/articles/a3/index.html', '/colophon/index.html', '/index.html']);
		for (const page of built.pages) {
			const html = await readFile(join(built.dir, page), 'utf8');
			expect(html.split(marker(7))).toHaveLength(2);
		}
		const home = await readFile(join(built.dir, 'index.html'), 'utf8');
		expect(home).toContain('Third &lt;b&gt;');
		expect(home).not.toContain('<b>');
		expect(await readFile(join(built.dir, 'articles/a2/index.html'), 'utf8')).toContain('<p>Body of Second</p>');
		expect(await readFile(join(built.dir, 'colophon/index.html'), 'utf8')).toContain('release <strong>7</strong>');
		expect((await readFile(join(built.dir, 'generation.txt'), 'utf8')).trim()).toBe('7');
	});

	test('serves an article at its slug, and refuses two articles at one address', async () => {
		const root = await mkdtemp(join(tmpdir(), 'site-'));
		const slugged = articles(new Map([[2, [item('a2', 'Second', 1, 'second-post'), item('a1', 'First')]]]));
		const built = await build({ ...settings, root, fetch: slugged.fetch });
		expect(built.pages).toContain('/articles/second-post/index.html');
		expect(built.pages).toContain('/articles/a1/index.html');
		expect(await readFile(join(built.dir, 'index.html'), 'utf8')).toContain('href="/articles/second-post/"');

		const clash = articles(new Map([[3, [item('a2', 'Second', 1, 'a1'), item('a1', 'First')]]]));
		await expect(build({ ...settings, root, fetch: clash.fetch })).rejects.toBeInstanceOf(BuildFailed);
	});

	test('pins every read to the release asked for', async () => {
		const root = await mkdtemp(join(tmpdir(), 'site-'));
		const colophon = articles(new Map([[3, [item('a', 'Then')]], [9, [item('b', 'Now')]]]));
		const built = await build({ ...settings, generation: 3, root, fetch: colophon.fetch });
		expect(built.generation).toBe(3);
		expect(colophon.asked[0]?.searchParams.get('generation')).toBe('3');
		expect(await readFile(join(built.dir, 'index.html'), 'utf8')).toContain('Then');
	});

	test('fails rather than mix releases, guess at content or build from an error', async () => {
		const root = await mkdtemp(join(tmpdir(), 'site-'));
		const mixed = (async () =>
			new Response(JSON.stringify({ data: [], next_cursor: null }), { headers: { 'colophon-generation': '8' } })) as typeof fetch;
		await expect(build({ ...settings, generation: 7, root, fetch: mixed })).rejects.toThrow(BuildFailed);
		const broken = (async () => new Response('{}', { status: 503 })) as typeof fetch;
		await expect(build({ ...settings, root, fetch: broken })).rejects.toThrow('HTTP 503');
		const strange = item('x', 'Strange');
		strange.content.body = { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'script' }] } };
		await expect(build({ ...settings, root, fetch: articles(new Map([[1, [strange]]])).fetch })).rejects.toThrow();
	});

	// The website starter (W.3): published pages become the site — one file each at its slug, the
	// menu's resolved links, the theme as the linked stylesheet — beside any articles.
	const hero = { _block: 'hero', heading: 'Your site, your way', tagline: 'It is' };
	const cta = { _block: 'cta', heading: 'Ready?', label: 'Read more', url: 'https://colophon.test/docs' };
	const page = (id: string, slug: string, sections: unknown[] = [hero, cta]) =>
		site(id, slug, { title: slug, slug, seo: { description: `About ${slug}` }, sections }, 1, slug);
	const menu = site('n1', 'Menu', {
		items: [
			{ label: 'Home', page: 'p1' },
			{ label: 'About', page: 'p2' },
			{ label: 'Docs', url: 'https://colophon.test/docs' }
		]
	});
	const palette = site('t1', 'Theme', {
		light: {
			primary: '#112244',
			surface: '#ffffff',
			text: '#111111',
			muted: '#555555',
			radius_md: 8,
			shadow: 'subtle',
			font_body: 'system',
			font_heading: 'serif',
			base_size: 16
		}
	});
	const themed = (gen: number) =>
		api(
			new Map<string, Map<number, unknown[]>>([
				['pages', new Map([[gen, [page('p1', 'home'), page('p2', 'about')]]])],
				['navigation', new Map([[gen, [menu]]])],
				['theme', new Map([[gen, [palette]]])]
			])
		);

	test('builds the site: pages at their slugs, the menu resolved, the theme linked', async () => {
		const root = await mkdtemp(join(tmpdir(), 'site-'));
		const colophon = themed(4);
		const built = await build({ ...settings, root, fetch: colophon.fetch });
		expect(built.pages.sort()).toEqual(['/404.html', '/about/index.html', '/colophon/index.html', '/home/index.html', '/index.html']);

		const home = await readFile(join(built.dir, 'index.html'), 'utf8');
		expect(home).toContain('Your site, your way');
		expect(home).toContain('href="/home/"');
		expect(home).toContain('href="/about/"');
		expect(home).toContain('href="https://colophon.test/docs"');
		expect(home).toContain('<meta name="description" content="About home"');
		expect(home).toContain('<link rel="stylesheet" href="/site.css">');
		expect(home.split(marker(4))).toHaveLength(2);

		const css = await readFile(join(built.dir, 'site.css'), 'utf8');
		expect(css).toContain('--site-primary:#112244');
		expect(css).toContain('--site-radius-md:8px');
		expect(css).toContain('--site-font-heading:ui-serif');
	});

	test('serves the first menu page at /, skips unpublished menu links and keeps the blog', async () => {
		const root = await mkdtemp(join(tmpdir(), 'site-'));
		const first = site('n1', 'Menu', {
			items: [
				{ label: 'Start', page: 'p2' },
				{ label: 'Gone', page: 'p9' }
			]
		});
		const colophon = api(
			new Map<string, Map<number, unknown[]>>([
				['pages', new Map([[5, [page('p1', 'home'), page('p2', 'about')]]])],
				['navigation', new Map([[5, [first]]])],
				['theme', new Map([[5, [site('t1', 'Theme', { light: { primary: '#112244' } })]]])],
				['articles', new Map([[5, [item('a1', 'An article', 5, 'post')]]])]
			])
		);
		const built = await build({ ...settings, root, fetch: colophon.fetch });
		expect(built.pages).toContain('/articles/post/index.html');
		const index = await readFile(join(built.dir, 'index.html'), 'utf8');
		// `/` is About — the first menu item's page — not Home.
		expect(index).toContain('About about');
		expect(index).toContain('href="/about/"');
		expect(index).not.toContain('Gone');
	});

	test('falls back to the starter palette without a published theme, and fails on an unknown section', async () => {
		const root = await mkdtemp(join(tmpdir(), 'site-'));
		const collections = (entries: unknown[][]) =>
			api(
				new Map<string, Map<number, unknown[]>>(
					entries.map(([name, items, gen]) => [String(name), new Map([[Number(gen), items as unknown[]]])])
				)
			);
		const themeless = collections([['pages', [page('p1', 'home')], 2]]);
		const built = await build({ ...settings, root, fetch: themeless.fetch });
		expect(await readFile(join(built.dir, 'site.css'), 'utf8')).toContain('--site-primary:#6d28d9');

		const weird = collections([['pages', [site('p1', 'home', { title: 'home', sections: [{ _block: 'gallery' }] })], 2]]);
		await expect(build({ ...settings, root, fetch: weird.fetch })).rejects.toBeInstanceOf(BuildFailed);
	});

	test('fails a page whose slug covers a fixed path, and a build that mixes releases', async () => {
		const root = await mkdtemp(join(tmpdir(), 'site-'));
		const covering = api(
			new Map<string, Map<number, unknown[]>>([['pages', new Map<number, unknown[]>([[2, [page('p1', 'colophon')]]])]])
		);
		await expect(build({ ...settings, root, fetch: covering.fetch })).rejects.toBeInstanceOf(BuildFailed);

		const mixed = (async (input: string | URL | Request) => {
			const collection = decodeURIComponent(new URL(String(input)).pathname.split('/').at(-1) ?? '');
			const generation = collection === 'pages' ? '4' : '5';
			return new Response(JSON.stringify({ data: [], next_cursor: null }), { headers: { 'colophon-generation': generation } });
		}) as typeof fetch;
		await expect(build({ ...settings, root, fetch: mixed })).rejects.toThrow('asked for release 4, got 5');
	});
});
