// A stand-in for Colophon's public API: pages of entries at a release, as /content/v1 answers,
// routed by the collection in the address.

import type { Article } from '../src/pages.ts';
import type { Entry } from '../src/site.ts';

export function doc(text: string): unknown {
	return { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] } };
}

export function item(id: string, title: string, publishedIn = 1, slug?: string): Article {
	return { id, title, ...(slug === undefined ? {} : { slug }), content: { title, body: doc(`Body of ${title}`) }, published_in: publishedIn };
}

/** One published site entry: `content` is whatever its type holds (sections, items, modes). */
export function site(id: string, title: string, content: Record<string, unknown>, publishedIn = 1, slug?: string): Entry {
	return { id, title, ...(slug === undefined ? {} : { slug }), content, published_in: publishedIn };
}

/** The collections a fake serves, each release to its entries. `articles` is one of them. */
export type Collections = Map<string, Map<number, unknown[]>>;

/** A fetch that serves each collection's release (release → entries, newest first) in pages of
 *  `pageSize`, recording every URL asked for. The latest release is the highest a collection has;
 *  a collection with no entries answers empty, like the API does for one that doesn't exist. */
export function api(collections: Collections, pageSize = 100): { fetch: typeof fetch; asked: URL[] } {
	const asked: URL[] = [];
	const fake = (async (input: string | URL | Request) => {
		const url = new URL(String(input));
		asked.push(url);
		const collection = decodeURIComponent(url.pathname.split('/').at(-1) ?? '');
		const releases = collections.get(collection) ?? new Map<number, unknown[]>();
		// The project's latest release, whatever collection it touched: an empty collection still
		// names the current generation, as the API does — never 0 while anything is published.
		const latest = Math.max(0, ...[...collections.values()].flatMap((releases) => [...releases.keys()]));
		const cursor = url.searchParams.get('cursor');
		const [generation, offset] = cursor === null ? [Number(url.searchParams.get('generation') ?? latest), 0] : cursor.split(':').map(Number);
		const all = releases.get(generation ?? latest) ?? [];
		const start = offset ?? 0;
		const data = all.slice(start, start + pageSize);
		const next = start + pageSize < all.length ? `${generation}:${start + pageSize}` : null;
		return new Response(JSON.stringify({ data, next_cursor: next }), {
			headers: { 'content-type': 'application/json', 'colophon-generation': String(generation) }
		});
	}) as typeof fetch;
	return { fetch: fake, asked };
}

/** The articles-only shape the first tests were written against. */
export function articles(releases: Map<number, Article[]>, pageSize = 100): { fetch: typeof fetch; asked: URL[] } {
	return api(new Map([['articles', releases as Map<number, unknown[]>]]), pageSize);
}
