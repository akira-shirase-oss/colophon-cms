// Rich text on the demo sites goes through `@colophon/content-renderer` (the one renderer the
// Studio's preview and the reference site share, D-023), with image nodes pointing at this
// project's own `/media/v1` origin. The renderer throws `UnrenderableContent` on anything outside
// the format rather than guessing; a site build fails with it, which is what the strictness is
// for (R3).

import { renderToHtml } from '@colophon/content-renderer';

import { mediaUrl, picture, type Entry } from './delivery.ts';
import type { Site } from './config.ts';

/**
 * The HTML of a `colophon/rich-text` v1 field, or `''` when the content has none. Given the
 * `entry` it came from, images show the version its release published, with their size and
 * renditions (renderer v2, the row's `media` sidecar); without it, or for an image the sidecar
 * doesn't hold, the bare current address.
 */
export function richText(site: Site, value: unknown, entry?: Entry): string {
	if (value === undefined || value === null) return '';
	return renderToHtml(value as Parameters<typeof renderToHtml>[0], {
		assetUrl: (assetId) => mediaUrl(site, assetId),
		media: (assetId) => (entry?.media !== undefined && Object.hasOwn(entry.media, assetId) ? picture(site, entry, assetId) : undefined)
	});
}
