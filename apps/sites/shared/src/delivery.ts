// Anonymous reads of the published projection, `GET /content/v1/{project}/{environment}/{collection}`
// (docs/modules/06-delivery.md): no credential, published entries only. A build pages through the
// whole collection at the generation the first page names (the cursor carries it, so pages can
// never mix two releases, DL2), and records the `Colophon-Generation` header as the site's
// release marker, the same one the reference site and the probes read.

import type { Site } from './config.ts';

/** One published entry, as the delivery API answers. */
export interface Entry {
	id: string;
	revision_id: string;
	collection: string;
	title: string;
	slug?: string;
	content: Record<string, unknown>;
	content_hash: string;
	/** The release it was published in. */
	published_in: number;
}

/** A collection's whole published set and the generation it was read at. */
export interface Entries {
	items: Entry[];
	generation: number;
}

interface Page {
	data?: Entry[];
	next_cursor?: string | null;
}

const PAGE = 100;
/** A bound on paging: more pages than this means something is wrong, so the build stops. */
const MAX_PAGES = 50;

/**
 * Every published entry of `collection`. Throws on a refused or failed answer: a seeded site
 * that cannot read its API should fail the build, not go live silently empty.
 */
export async function entries(site: Site, collection: string): Promise<Entries> {
	const base = `${site.api}/content/v1/${site.project}/${site.environment}/${collection}`;
	const items: Entry[] = [];
	let generation = 0;
	let cursor: string | null = null;
	for (let page = 0; page < MAX_PAGES; page += 1) {
		const url = new URL(base);
		url.searchParams.set('limit', String(PAGE));
		if (cursor !== null) url.searchParams.set('cursor', cursor);
		const response = await fetch(url, { headers: { accept: 'application/json' } });
		if (!response.ok) {
			throw new Error(`GET ${url.pathname}: HTTP ${response.status}`);
		}
		const named = Number(response.headers.get('colophon-generation'));
		if (Number.isFinite(named)) generation = named;
		const body = (await response.json()) as Page;
		items.push(...(body.data ?? []));
		cursor = body.next_cursor ?? null;
		if (cursor === null) break;
	}
	return { items, generation };
}

/** The public URL of a media asset: `/media/v1/{project}/{environment}/{asset}[/{rendition}]`. */
export function mediaUrl(site: Site, asset: string, rendition?: string): string {
	const base = `${site.api}/media/v1/${site.project}/${site.environment}/${asset}`;
	return rendition === undefined ? base : `${base}/${rendition}`;
}

/** The entry's URL name: its slug when it set one, else its id. */
export function entryPath(entry: Entry): string {
	return encodeURIComponent(entry.slug ?? entry.id);
}
