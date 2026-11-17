// The `media` sidecar on delivery rows, and `mediaFor`, which turns it into what a renderer needs
// to draw an image: a source, its size, and responsive candidates.
//
// A release records the version of every image its entries name — asset fields and images inside
// rich text alike — so `url` is the exact bytes that release published (an immutable address,
// cacheable for a year) and a pinned read at generation N shows N's images. Replacing an image
// reaches `url` at the entry's next publish; `current_url` always follows the newest version, for
// sites that want that instead (`mediaFor(row, { current: true })`).

import type { components } from './api.types.js';

/** One image a published revision names, as its delivery row's `media` sidecar records it —
 *  the generated wire type: `url` (the released version's original, immutable),
 *  `current_url` (follows replacements), `type`, the original's `width`/`height`, and
 *  `renditions` by name (`wN` exists only when the original is wider than N, and is exactly N
 *  pixels wide). */
export type PublishedMedia = components['schemas']['PublishedMedia'];

/** A delivery row's sidecar: every asset its published revision names, by asset id. Absent when
 *  the revision names none (and on answers cached before the sidecar existed). */
export type MediaSidecar = Record<string, PublishedMedia>;

/** What a renderer needs for one image; structurally `@colophon/content-renderer`'s
 *  `ImageSource`, so `mediaFor(row)` is its `media` option as it stands. */
export interface ImageSource {
	src: string;
	width?: number | undefined;
	height?: number | undefined;
	srcset?: string | undefined;
	sizes?: string | undefined;
}

export interface MediaOptions {
	/** Follow replacements: `current_url` and its bare-address renditions instead of the
	 *  released version's immutable ones. Images then change without a republish — and may
	 *  differ from what a pinned read of the same generation showed. */
	current?: boolean;
	/** The `sizes` sent with `srcset`. Default: full width up to 1280 px. */
	sizes?: string;
}

/** The default slot: the page's full width until 1280 px, then 1280 px. */
export const DEFAULT_SIZES = '(max-width: 1280px) 100vw, 1280px';

/** A rendition name's width: `w320` → 320; anything else isn't a width candidate. */
function renditionWidth(name: string): number | undefined {
	const match = /^w([1-9][0-9]{0,5})$/.exec(name);
	return match?.[1] === undefined ? undefined : Number(match[1]);
}

/** One image's source from its sidecar record: the original (released or current) at its
 *  width, with each `wN` rendition as an `N w` candidate, narrowest first. */
export function imageSource(media: PublishedMedia, options: MediaOptions = {}): ImageSource {
	const original = options.current === true ? media.current_url : media.url;
	const candidates: Array<[string, number]> = [];
	for (const [name, url] of Object.entries(media.renditions ?? {})) {
		const width = renditionWidth(name);
		if (width === undefined || width >= media.width) continue;
		candidates.push([options.current === true ? `${media.current_url}/${name}` : url, width]);
	}
	candidates.sort((a, b) => a[1] - b[1]);
	const source: ImageSource = { src: original, width: media.width, height: media.height };
	if (candidates.length > 0) {
		candidates.push([original, media.width]);
		source.srcset = candidates.map(([url, width]) => `${url} ${String(width)}w`).join(', ');
		source.sizes = options.sizes ?? DEFAULT_SIZES;
	}
	return source;
}

/** The renderer's `media` callback for one delivery row:
 *
 *    renderToHtml(row.content.body, { assetUrl, media: mediaFor(row) })
 *
 *  It answers `undefined` for an id the sidecar doesn't hold — an answer cached before the
 *  sidecar existed — so the renderer falls back to `assetUrl`'s bare address. An asset field
 *  works the same way: `mediaFor(row)(row.content.photo)`. */
export function mediaFor(
	row: { media?: MediaSidecar | null | undefined } | null | undefined,
	options: MediaOptions = {}
): (assetId: string) => ImageSource | undefined {
	const sidecar = row?.media ?? undefined;
	return (assetId) => {
		if (sidecar === undefined || !Object.hasOwn(sidecar, assetId)) return undefined;
		const media = sidecar[assetId];
		return media === undefined ? undefined : imageSource(media, options);
	};
}
