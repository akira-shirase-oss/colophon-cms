// Where the docs live and where the website is, for the build (vite.config.ts, the Markdown
// pipeline, the feed) and the tests. The browser gets the base path through Vite's `define`
// (`__BASE__`, read in lib/paths.ts). Node only: it reads the website's routes.

import { existsSync, readdirSync, statSync } from 'node:fs';
import path from 'node:path';

/**
 * The docs' base path. The website links to `/docs/` on its own host (apps/site/src/lib/site.ts,
 * docs/modules/20-site.md), so the docs are built for that address.
 */
export const DOCS_BASE = base(process.env['COLOPHON_DOCS_BASE'] ?? '/docs');

/** SvelteKit's rule for a base path: empty, or a slash and a name with no slash at the end. */
function base(value: string): '' | `/${string}` {
	if (value === '') return '';
	if (!value.startsWith('/') || value.endsWith('/')) throw new Error(`COLOPHON_DOCS_BASE must be empty or like /docs, not ${value}`);
	return value as `/${string}`;
}

/**
 * The public origin, used only where an absolute URL is required (the changelog's feed). The same
 * variable and default as the website's (apps/site/vite.config.ts): hosting is undecided (O-012),
 * so the default is a reserved name that can't resolve.
 */
export const PUBLIC_ORIGIN = (process.env['COLOPHON_SITE_ORIGIN'] ?? 'https://colophon.invalid').replace(/\/$/, '');

/** A path inside the docs, with the base path. */
export function docsPath(path: string): string {
	return path.startsWith('/') && !path.startsWith('//') ? `${DOCS_BASE}${path}` : path;
}

/** The website's address for one of its paths, such as `/roadmap/`: the same host, so root-relative. */
export function sitePath(path: string): string {
	return path;
}

const SITE = path.resolve(import.meta.dirname, '../../../../site');

let pages: Set<string> | undefined;

/**
 * Every address the website prerenders, read from its source: each static route, each blog post
 * and the feeds. The docs' links to the website are checked against this list at build time and
 * in the tests, since the website is a separate app that the docs' prerenderer can't crawl.
 */
export function sitePages(): Set<string> {
	if (pages) return pages;
	const found = new Set<string>();
	const routes = path.join(SITE, 'src', 'routes');
	const walk = (dir: string, address: string) => {
		for (const name of readdirSync(dir)) {
			const full = path.join(dir, name);
			if (statSync(full).isDirectory()) {
				if (name.startsWith('[')) continue;
				// A folder named like a file (`rss.xml`) is an endpoint: its address has no trailing slash.
				walk(full, /\.[a-z]+$/.test(name) ? `${address}${name}` : `${address}${name}/`);
			} else if (name === '+page.svelte' || name === '+server.ts') {
				found.add(address);
			}
		}
	};
	if (existsSync(routes)) walk(routes, '/');
	const blog = path.join(SITE, 'src', 'content', 'blog');
	if (existsSync(blog)) {
		for (const post of readdirSync(blog)) if (post.endsWith('.md')) found.add(`/blog/${post.slice(0, -3)}/`);
	}
	pages = found;
	return found;
}

/** Whether an address (without the origin, query or fragment) is one of the website's pages. */
export function isSitePath(address: string): boolean {
	const bare = address.split(/[?#]/)[0] ?? address;
	return sitePages().has(bare);
}
