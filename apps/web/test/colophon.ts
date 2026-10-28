// A stand-in for Colophon's public API: pages of articles at a release, as /content/v1 answers.

import type { Article } from '../src/pages.ts';

export function doc(text: string): unknown {
	return { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] } };
}

export function item(id: string, title: string, publishedIn = 1, slug?: string): Article {
	return { id, title, ...(slug === undefined ? {} : { slug }), content: { title, body: doc(`Body of ${title}`) }, published_in: publishedIn };
}

/** A fetch that serves `releases` (release → articles, newest first) in pages of `pageSize`,
 *  recording every URL asked for. The latest release is the highest. */
export function api(releases: Map<number, Article[]>, pageSize = 100): { fetch: typeof fetch; asked: URL[] } {
	const asked: URL[] = [];
	const latest = Math.max(0, ...releases.keys());
	const fake = (async (input: string | URL | Request) => {
		const url = new URL(String(input));
		asked.push(url);
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
