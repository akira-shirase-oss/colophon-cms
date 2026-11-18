// The `media` sidecar a delivery row carries: every asset its published revision names, at the
// version its release recorded. The reference site draws images from it — the immutable address
// the release published, its size, its renditions as `srcset` candidates (renderer v2) — so a
// page built from release N shows N's images, and a replaced image reaches the site when the
// entries that use it are published again. An answer without the sidecar (cached before it
// existed) falls back to the bare address, as before.

import type { ImageSource } from '@colophon/content-renderer';

/** One image as the sidecar records it. */
export interface PublishedMedia {
	url: string;
	current_url: string;
	type: string;
	width: number;
	height: number;
	/** `wN` exists only when the original is wider than N, and is exactly N pixels wide. */
	renditions: Record<string, string>;
}

/** Every asset a row's revision names, by id. */
export type MediaSidecar = Record<string, PublishedMedia>;

/** The sidecar's record for `asset`, never an inherited property. */
export function recorded(media: MediaSidecar | undefined, asset: string): PublishedMedia | undefined {
	return media !== undefined && Object.hasOwn(media, asset) ? media[asset] : undefined;
}

/** The renderer's `media` callback for one row: the released original at its size, renditions
 *  narrowest first as width candidates, the article column as the slot. */
export function mediaOf(media: MediaSidecar | undefined): (asset: string) => ImageSource | undefined {
	return (asset) => {
		const found = recorded(media, asset);
		if (found === undefined) return undefined;
		const candidates: Array<[string, number]> = [];
		for (const [name, url] of Object.entries(found.renditions)) {
			const match = /^w([1-9][0-9]{0,5})$/.exec(name);
			const width = match?.[1] === undefined ? undefined : Number(match[1]);
			if (width !== undefined && width < found.width) candidates.push([url, width]);
		}
		candidates.sort((a, b) => a[1] - b[1]);
		if (candidates.length === 0) return { src: found.url, width: found.width, height: found.height };
		candidates.push([found.url, found.width]);
		return {
			src: found.url,
			width: found.width,
			height: found.height,
			srcset: candidates.map(([url, width]) => `${url} ${String(width)}w`).join(', '),
			// The article column is 40rem wide at most (STYLE).
			sizes: '(max-width: 40rem) 100vw, 40rem'
		};
	};
}
