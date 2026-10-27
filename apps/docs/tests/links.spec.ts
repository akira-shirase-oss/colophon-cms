// Links: crawl the built docs over HTTP from the front page, follow every address and asset under the
// base path, and check each answers 200 and that every #fragment names an element on its page.
// Every page in the build must be reachable by links. Links out to the website must name one of
// its pages, and every docs address the website links to must exist here.

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { expect, test } from '@playwright/test';
import { isSitePath } from '../src/lib/markdown/addresses.ts';
import { BASE, builtPages, docs, REPO } from './site.ts';

const ATTRIBUTE = /\s(?:href|src)="([^"]+)"/g;
const ID = /\sid="([^"]+)"/g;

test('every internal link, anchor and asset resolves, every page is reachable, and links to the website land on its pages', async ({ request, baseURL }) => {
	test.setTimeout(180_000);
	const origin = new URL(baseURL ?? '').origin;
	const start = docs('/');
	const queue = [start];
	const seen = new Set<string>(queue);
	const ids = new Map<string, Set<string>>();
	const fragments: { from: string; page: string; id: string }[] = [];
	const broken: string[] = [];
	const toSite = new Set<string>();

	while (queue.length > 0) {
		const address = queue.shift() ?? start;
		const response = await request.get(address, { maxRedirects: 0 });
		if (response.status() !== 200) {
			broken.push(`${address}: ${response.status()}`);
			continue;
		}
		if (!(response.headers()['content-type'] ?? '').startsWith('text/html')) continue;
		const html = await response.text();
		ids.set(address, new Set([...html.matchAll(ID)].map((match) => match[1] ?? '')));
		for (const match of html.matchAll(ATTRIBUTE)) {
			const raw = (match[1] ?? '').replaceAll('&amp;', '&');
			const url = new URL(raw, `${origin}${address}`);
			if (url.origin !== origin) continue;
			const path = url.pathname;
			if (!path.startsWith(`${BASE}/`)) {
				toSite.add(path);
				continue;
			}
			if (url.hash) fragments.push({ from: address, page: path, id: decodeURIComponent(url.hash.slice(1)) });
			// An address that isn't a file has to end in a slash: static hosts serve folders that way.
			if (!path.endsWith('/') && !/\.[a-z0-9]+$/i.test(path)) broken.push(`${address} links to ${path} without its trailing slash`);
			if (!seen.has(path)) {
				seen.add(path);
				queue.push(path);
			}
		}
	}

	for (const { from, page, id } of fragments) {
		const known = ids.get(page);
		if (known && !known.has(id)) broken.push(`${from} links to ${page}#${id}, which has no such id`);
	}
	expect(broken, 'broken links and anchors').toEqual([]);
	const unreachable = builtPages().filter((page) => !seen.has(docs(page)));
	expect(unreachable, 'pages no link leads to').toEqual([]);
	expect([...toSite].filter((path) => !isSitePath(path)), "links to the website that aren't its pages").toEqual([]);
	// The header, the footer and the front page link to the website's home and its main pages.
	for (const path of ['/', '/product/', '/roadmap/', '/blog/']) expect(toSite.has(path), `a link to the website's ${path}`).toBe(true);
});

test("every docs address the website links to is one of the docs' pages", () => {
	const pages = new Set(builtPages().map((page) => docs(page)));
	const files: string[] = [];
	const walk = (dir: string) => {
		for (const name of readdirSync(dir)) {
			const path = join(dir, name);
			if (statSync(path).isDirectory()) walk(path);
			else if (/\.(svelte|ts)$/.test(name)) files.push(path);
		}
	};
	walk(join(REPO, 'apps', 'site', 'src'));
	const addresses = new Set<string>();
	for (const file of files) {
		for (const match of readFileSync(file, 'utf8').matchAll(/['"`](\/docs\/[^'"`#?\s]*)/g)) addresses.add(match[1] ?? '');
	}
	expect(addresses.size, 'the website links to the docs').toBeGreaterThan(1);
	const missing = [...addresses].filter((address) => !pages.has(address) && !address.endsWith('.xml'));
	expect(missing, "website links into the docs that aren't pages").toEqual([]);
	expect(addresses.has('/docs/changelog/feed.xml'), 'the website links to the changelog feed').toBe(true);
});
