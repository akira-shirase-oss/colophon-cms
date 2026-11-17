// Small rules the Studio's media views share (the library panel, the rich-text image view and
// the insert dialog): which address shows an upload's picture, how its state reads, and whether
// a piece of alt text looks like a file name instead of a description.

import { type Asset, assetUrl } from './api.ts';

/** The rendition the worker writes for thumbnails (D-096): present only when the source is wider. */
export const THUMBNAIL = 'w320';
/** The full-width rendition, for a large preview. */
export const LARGE = 'w1280';

/** A usable upload's picture for a tile: its `w320` rendition when the worker wrote one, else the
 *  original (a small upload has no rendition — nothing upscales). */
export function thumbnailUrl(project: string, asset: Pick<Asset, 'id' | 'derivatives'>): string {
	const base = assetUrl(project, asset.id);
	return asset.derivatives.includes(THUMBNAIL) ? `${base}/${THUMBNAIL}` : base;
}

/** The candidates a browser may choose from for a picture shown about `width` CSS pixels wide:
 *  each rendition the worker wrote, by its width, and the original. Empty when there is only the
 *  original, which `src` already names. */
export function thumbnailSrcset(project: string, asset: Pick<Asset, 'id' | 'derivatives' | 'width'>): string {
	const base = assetUrl(project, asset.id);
	const sizes = asset.derivatives
		.map((name) => ({ name, width: /^w(\d+)$/.exec(name)?.[1] }))
		.filter((rendition): rendition is { name: string; width: string } => rendition.width !== undefined)
		.map((rendition) => `${base}/${rendition.name} ${rendition.width}w`);
	if (sizes.length === 0) return '';
	if (asset.width !== null) sizes.push(`${base} ${asset.width}w`);
	return sizes.join(', ');
}

/** How an upload's state reads beside it, in plain words. */
export function stateText(state: string): string {
	switch (state) {
		case 'usable':
			return 'Ready';
		case 'rejected':
			return 'Rejected';
		case 'deleting':
			return 'Deleting…';
		case 'deleted':
			return 'Deleted';
		default:
			return 'Checking…';
	}
}

/** What a picture is, for people who can't see it: the library's alt text, else its title. Never
 *  the file name, which says nothing about the picture. */
export function describedAs(asset: Pick<Asset, 'alt' | 'title'> | undefined): string {
	return asset?.alt?.trim() || asset?.title?.trim() || '';
}

const FILE_EXTENSION = /\.(png|jpe?g|webp|gif|avif|heic|svg|bmp|tiff?)$/i;
const CAMERA_NAME = /^(img|dsc|dscn|dscf|pxl|mvimg|screenshot|screen shot|photo|image|untitled)[\s_-]*\d*([\s_-]\d+)*$/i;
const ID_LIKE = /^[\w-]*\d{4,}[\w-]*$/;

/** Whether `alt` reads like a file name ("IMG_1234.jpg", "cover.png", "Screenshot 2026-11-17")
 *  rather than a description of the picture: worth a gentle warning, never a refusal. */
export function looksLikeFileName(alt: string): boolean {
	const text = alt.trim();
	if (text === '') return false;
	if (FILE_EXTENSION.test(text)) return true;
	if (CAMERA_NAME.test(text)) return true;
	// One word made of letters and a long run of digits, like DSC_00412 or 20261117_093011.
	return !/\s/.test(text) && ID_LIKE.test(text);
}

/** Whether a library upload may be chosen: one that is gone, or on its way out, can't be. */
export const pickable = (asset: Asset) => asset.state !== 'deleting' && asset.state !== 'deleted';
