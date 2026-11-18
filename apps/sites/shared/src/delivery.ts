// Anonymous reads of the published projection, `GET /content/v1/{project}/{environment}/{collection}`
// (docs/modules/06-delivery.md): no credential, published entries only. A build pages through the
// whole collection at the generation the first page names (the cursor carries it, so pages can
// never mix two releases, DL2), and records the `Colophon-Generation` header as the site's
// release marker, the same one the reference site and the probes read.

import type { Site } from './config.ts';

/** One image a published revision names, as the row's `media` sidecar records it: the version
 *  its release published (`url`, immutable) and the asset's newest (`current_url`). */
export interface PublishedMedia {
	url: string;
	current_url: string;
	type: string;
	width: number;
	height: number;
	/** The released version's renditions by name; `wN` exists only when the original is wider than
	 *  N, and is exactly N pixels wide. */
	renditions: Record<string, string>;
}

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
	/** Who may read it (D-137): `public`, `authenticated` or `role:<name>`. */
	visibility: string;
	/** Every asset the revision names, by id, at the version its release published; absent when
	 *  it names none, and on answers cached before the sidecar existed. */
	media?: Record<string, PublishedMedia>;
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
 * Every published entry of `collection` the caller may read: anonymous without `token`, or as
 * the site user a live audience session names — whose role the database applies (D-137). A
 * member read is answered `private` straight from Postgres, never the anonymous cache. Throws
 * on a refused or failed answer: a seeded site that cannot read its API should fail the build,
 * not go live silently empty.
 */
export async function entries(site: Site, collection: string, token?: string): Promise<Entries> {
	const base = `${site.api}/content/v1/${site.project}/${site.environment}/${collection}`;
	const headers: Record<string, string> = { accept: 'application/json' };
	if (token !== undefined) headers.authorization = `Bearer ${token}`;
	const items: Entry[] = [];
	let generation = 0;
	let cursor: string | null = null;
	for (let page = 0; page < MAX_PAGES; page += 1) {
		const url = new URL(base);
		url.searchParams.set('limit', String(PAGE));
		if (cursor !== null) url.searchParams.set('cursor', cursor);
		const response = await fetch(url, { headers });
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

/**
 * One published entry by id — the same read `entries` makes, narrowed. `null` when the caller
 * may not read it (gated reads are indistinguishable from unpublished ones) or it is gone.
 */
export async function entry(site: Site, collection: string, id: string, token?: string): Promise<Entry | null> {
	const url = `${site.api}/content/v1/${site.project}/${site.environment}/${collection}/${id}`;
	const headers: Record<string, string> = { accept: 'application/json' };
	if (token !== undefined) headers.authorization = `Bearer ${token}`;
	const response = await fetch(url, { headers });
	if (response.status === 404 || response.status === 410) return null;
	if (!response.ok) {
		throw new Error(`GET ${new URL(url).pathname}: HTTP ${response.status}`);
	}
	return ((await response.json()) as { data?: Entry }).data ?? null;
}

/** The public URL of a media asset: `/media/v1/{project}/{environment}/{asset}[/{rendition}]`. */
export function mediaUrl(site: Site, asset: string, rendition?: string): string {
	const base = `${site.api}/media/v1/${site.project}/${site.environment}/${asset}`;
	return rendition === undefined ? base : `${base}/${rendition}`;
}

/** An `<img>`'s attributes: where, how big, and the candidates a browser may pick from. */
export interface Picture {
	src: string;
	width?: number;
	height?: number;
	srcset?: string;
	sizes?: string;
}

/** The width a rendition name promises: `w320` → 320. */
function renditionWidth(name: string): number | undefined {
	const match = /^w([1-9][0-9]{0,5})$/.exec(name);
	return match?.[1] === undefined ? undefined : Number(match[1]);
}

/**
 * An image `entry` names, at the version its release published: the sidecar's immutable
 * address, `rendition` when the worker wrote it (else the original — a narrow upload has none),
 * its size so the page reserves the box, and every rendition as a `srcset` candidate for
 * `sizes`. An asset the sidecar doesn't hold — an answer cached before it existed — falls back
 * to the bare address, as before.
 */
export function picture(site: Site, entry: Entry, asset: string, options: { rendition?: string; sizes?: string } = {}): Picture {
	const media = entry.media !== undefined && Object.hasOwn(entry.media, asset) ? entry.media[asset] : undefined;
	if (media === undefined) return { src: mediaUrl(site, asset, options.rendition) };
	const candidates: Array<[string, number]> = [];
	for (const [name, url] of Object.entries(media.renditions)) {
		const width = renditionWidth(name);
		if (width !== undefined && width < media.width) candidates.push([url, width]);
	}
	candidates.sort((a, b) => a[1] - b[1]);
	const chosen = options.rendition === undefined ? undefined : media.renditions[options.rendition];
	const picked: Picture = { src: chosen ?? media.url, width: media.width, height: media.height };
	if (candidates.length > 0) {
		candidates.push([media.url, media.width]);
		picked.srcset = candidates.map(([url, width]) => `${url} ${String(width)}w`).join(', ');
		picked.sizes = options.sizes ?? '(max-width: 1280px) 100vw, 1280px';
	}
	return picked;
}

/** The entry's URL name: its slug when it set one, else its id. */
export function entryPath(entry: Entry): string {
	return encodeURIComponent(entry.slug ?? entry.id);
}
