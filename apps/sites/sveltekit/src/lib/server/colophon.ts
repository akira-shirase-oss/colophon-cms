// How this site reads Colophon (D-220). The guide on the website quotes this file, so it is written out in
// full with plain `fetch` rather than through @colophon/site-kit's helpers.
//
// The rule it keeps: one build, one release. The first read learns the newest release number from the
// `Colophon-Generation` header; every read after it asks for that release with `?generation=N`, so a
// publish that lands in the middle of a build can't give one page the old content and the next page the
// new. Every page then names the release it was built from (the marker in +layout.svelte).

import type { Site } from '@colophon/site-kit';

/** A recipe as the delivery API answers it: the entry's envelope, and the content type's fields. */
export interface Recipe {
	id: string;
	title: string;
	slug?: string;
	/** The release the entry was published in. */
	published_in: number;
	content: {
		title: string;
		slug?: string;
		intro?: string;
		category?: string;
		difficulty?: string;
		prep_minutes?: number;
		cook_minutes?: number;
		servings?: number;
		ingredients?: { item: string; amount?: string }[];
		steps?: string[];
		notes?: unknown;
	};
}

interface Page {
	data?: Recipe[];
	next_cursor?: string | null;
}

const PAGE_SIZE = 100;
/** More pages than this means something is wrong, so the build stops instead of looping. */
const MAX_PAGES = 50;
/** Every request has a deadline: a CMS that doesn't answer fails the build, it doesn't hang it. */
const TIMEOUT_MS = 10_000;

const collectionUrl = (site: Site, collection: string) =>
	`${site.api}/content/v1/${site.project}/${site.environment}/${collection}`;

async function get(url: URL): Promise<Response> {
	const response = await fetch(url, { headers: { accept: 'application/json' }, signal: AbortSignal.timeout(TIMEOUT_MS) });
	if (!response.ok) throw new Error(`GET ${url.pathname}${url.search}: HTTP ${response.status}`);
	return response;
}

let pinned: Promise<number> | undefined;

/**
 * The release this build reads: the newest one when the build starts, learned once and shared by every
 * page the build renders (they run in one process). A failed read isn't cached, so a retry asks again.
 */
export function buildRelease(site: Site, collection: string): Promise<number> {
	pinned ??= (async () => {
		const url = new URL(collectionUrl(site, collection));
		url.searchParams.set('limit', '1');
		const generation = Number((await get(url)).headers.get('colophon-generation'));
		if (!Number.isInteger(generation) || generation < 1) throw new Error('the delivery API named no release');
		return generation;
	})().catch((error) => {
		pinned = undefined;
		throw error;
	});
	return pinned;
}

/** Every published entry of `collection`, read at release `generation`, page by page. */
export async function entriesAt(site: Site, collection: string, generation: number): Promise<Recipe[]> {
	const items: Recipe[] = [];
	let cursor: string | null = null;
	for (let page = 0; page < MAX_PAGES; page += 1) {
		const url = new URL(collectionUrl(site, collection));
		url.searchParams.set('generation', String(generation));
		url.searchParams.set('limit', String(PAGE_SIZE));
		if (cursor !== null) url.searchParams.set('cursor', cursor);
		const response = await get(url);
		const answered = Number(response.headers.get('colophon-generation'));
		if (answered !== generation) throw new Error(`asked for release ${generation}, got ${answered}`);
		const body = (await response.json()) as Page;
		items.push(...(body.data ?? []));
		cursor = body.next_cursor ?? null;
		if (cursor === null) return items;
	}
	throw new Error(`more than ${MAX_PAGES} pages in ${collection}`);
}
