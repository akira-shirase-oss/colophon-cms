// Every internal link resolves: crawl the built site over HTTP from the front page, follow every
// address and asset it references, and check each answers 200 and that every #fragment names an
// element on its page. Every page in the build must be reachable by links.

import { expect, test } from '@playwright/test';
import { builtPages } from './site.ts';

const ATTRIBUTE = /\s(?:href|src)="([^"]+)"/g;
const ID = /\sid="([^"]+)"/g;

test('every internal link and asset resolves, and every page is reachable', async ({ request, baseURL }) => {
	test.setTimeout(120_000);
	const origin = new URL(baseURL ?? '').origin;
	const queue = ['/'];
	const seen = new Set<string>(queue);
	const ids = new Map<string, Set<string>>();
	const fragments: { from: string; page: string; id: string }[] = [];
	const broken: string[] = [];

	while (queue.length > 0) {
		const address = queue.shift() ?? '/';
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
	expect(broken).toEqual([]);
	const unreachable = builtPages().filter((page) => !seen.has(page));
	expect(unreachable, 'pages no link leads to').toEqual([]);
});
