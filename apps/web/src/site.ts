// The website-starter's pages (W.3; D-089, D-092): published `pages`, `navigation` and `theme`
// entries become the site — one file per page at its slug, the menu resolved to addresses, the
// theme as the one stylesheet. Everything renders through @colophon/site-renderer, so the bytes
// here are the bytes the Studio's Look tab shows and the testbed previews. A page the renderer
// can't read fails the whole build: nothing half-built goes live.
//
// A project without the starter has no published pages, and the build is the blog alone, exactly
// as before. Pages but no theme keep the starter's palette — the same one the canvas shows until
// a theme is saved — so the site and its preview can't disagree about a default.

import { renderNav, renderSections, siteCss, pageHtml, type NavItem, type SectionContext, type ThemeContent } from '@colophon/site-renderer';
import { THEME_SEED } from '@colophon/site-template';

import { marker, type Release } from './pages.ts';

/** One published entry of any collection, as the public API answers (the article shape plus its
 *  content, which the site types read field by field). */
export interface Entry {
	id: string;
	title: string;
	slug?: string;
	content: Record<string, unknown>;
	published_in: number;
}

/** The starter theme's own content, used when a site has pages but no published theme entry. */
const DEFAULT_THEME: ThemeContent = THEME_SEED as ThemeContent;

/** `/{slug}/`, or `/{id}/` for a page without a slug — the public address a page is written at. */
export function pagePath(page: Entry): string {
	return `/${encodeURIComponent(page.slug ?? page.id)}/`;
}

/** The menu's items as addresses: `page` links resolve to the published page's path, `url` links
 *  stay as written. An item whose page isn't in this release is left out rather than linked dead. */
function menu(navigation: Entry | undefined, pages: Entry[]): NavItem[] {
	const items = (navigation?.content['items'] ?? []) as { label?: unknown; page?: unknown; url?: unknown }[];
	const links: NavItem[] = [];
	for (const item of items) {
		let href = typeof item.url === 'string' && item.url !== '' ? item.url : undefined;
		if (href === undefined && typeof item.page === 'string') {
			const target = pages.find((page) => page.id === item.page);
			if (target !== undefined) href = pagePath(target);
		}
		if (typeof item.label === 'string' && item.label !== '' && href !== undefined) links.push({ label: item.label, href });
	}
	return links;
}

const DEFAULT_CTX: SectionContext = { assetUrl: (id) => `/media/v1/${id}` };

function head(release: Release): string {
	return `${marker(release.generation)}\n<link rel="stylesheet" href="/site.css">\n`;
}

/** One page's document: its sections through the shared renderer, its title and description. */
function pageDocument(release: Release, page: Entry, nav: string, ctx: SectionContext): string {
	const seo = page.content['seo'] as { description?: unknown } | undefined;
	const description = typeof seo?.description === 'string' ? seo.description : undefined;
	return pageHtml({
		title: String(page.content['title'] ?? page.title),
		...(description === undefined ? {} : { description }),
		head: head(release),
		nav,
		body: renderSections(page.content['sections'], ctx)
	});
}

/** Which page sits at `/`: the first menu item's page, else the page slugged `home`, else the
 *  first page the release has. */
function homePage(navigation: Entry | undefined, pages: Entry[]): Entry {
	const items = (navigation?.content['items'] ?? []) as { page?: unknown }[];
	for (const item of items) {
		if (typeof item.page !== 'string') continue;
		const target = pages.find((page) => page.id === item.page);
		if (target !== undefined) return target;
	}
	return pages.find((page) => page.slug === 'home') ?? (pages[0] as Entry);
}

/**
 * The site a release's entries make, as files to write: every published page at its address, the
 * chosen home page again at `/`, the theme as `/site.css`, and a 404 wearing the same nav.
 * Throws whatever the renderer throws — a page that can't be read fails the build whole.
 */
export function siteFiles(release: Release, pages: Entry[], navigation: Entry | undefined, theme: Entry | undefined, ctx?: SectionContext): Map<string, string> {
	const files = new Map<string, string>();
	if (pages.length === 0) return files;

	const nav = renderNav(menu(navigation, pages));
	files.set('/site.css', `${siteCss((theme?.content ?? DEFAULT_THEME) as ThemeContent)}\n`);
	for (const page of pages) {
		const path = `${pagePath(page)}index.html`;
		// Two pages at one address would hide one of them (or cover a fixed page): fail whole.
		if (files.has(path)) throw new Error(`two pages would be served at ${path}`);
		files.set(path, pageDocument(release, page, nav, ctx ?? DEFAULT_CTX));
	}
	const home = pageDocument(release, homePage(navigation, pages), nav, ctx ?? DEFAULT_CTX);
	files.set('/index.html', home);
	const missing: Entry = {
		id: '404',
		title: 'Not found',
		content: { title: 'Not found', sections: [{ _block: 'hero', heading: 'Not found', tagline: 'Nothing is published at this address.' }] },
		published_in: release.generation
	};
	files.set('/404.html', pageDocument(release, missing, nav, ctx ?? DEFAULT_CTX));
	return files;
}
