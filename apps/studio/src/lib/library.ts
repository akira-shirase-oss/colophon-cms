// The Media page's helpers (D2): addresses on the media origin, how an upload's state and size
// read, the metadata rules the server holds (so the form can say what is wrong before it asks),
// and the bounded upload queue's limits. Pure functions, no Svelte: the page, the details sheet
// and the queue share them.

import { type Asset, assetUrl } from './api.ts';

/** Uploads one drop or pick may queue; more are left out with a note, never silently. */
export const MAX_QUEUE = 50;
/** Uploads sent at once: the rest wait their turn. */
export const CONCURRENCY = 3;
/** Uploads per library page. */
export const PAGE_SIZE = 48;
/** The most cards the page holds before it asks for a search instead of another page. */
export const MAX_CARDS = 480;
/** The library search's bound (the server's too). */
export const MAX_QUERY = 100;

/** The server's metadata bounds (D2): what a PATCH accepts. */
export const LIMITS = { alt: 300, title: 200, credit: 200, tags: 10, tag: 40 } as const;
/** One tag: lowercase letters, digits and hyphens, starting with a letter or digit. */
export const TAG = /^[a-z0-9][a-z0-9-]{0,39}$/;

/** The thumbnail for a usable upload: its 320-pixel rendition when the source warranted one,
 *  else the original (small images have no renditions). `null` for anything not servable. */
export function thumbnailUrl(project: string, asset: Asset): string | null {
	if (asset.state !== 'usable') return null;
	const base = assetUrl(project, asset.id);
	return asset.derivatives.includes('w320') ? `${base}/w320` : base;
}

/** The large preview: the 1280-pixel rendition when there is one, else the original. */
export function previewUrl(project: string, asset: Asset): string | null {
	if (asset.state !== 'usable') return null;
	const base = assetUrl(project, asset.id);
	return asset.derivatives.includes('w1280') ? `${base}/w1280` : base;
}

/** The address of one version's bytes, which never changes (D-136). */
export function versionUrl(project: string, asset: string, sha256: string): string {
	return `${assetUrl(project, asset)}/${sha256}`;
}

/** The current version's digest (D-136): the server sends it with every upload; read defensively
 *  so a record without one (nothing checked yet) is simply `null`. */
export function currentSha(asset: Asset): string | null {
	const sha = (asset as Asset & { sha256?: unknown }).sha256;
	return typeof sha === 'string' && sha !== '' ? sha : null;
}

/** `path` as a full address on this origin, for copying into a site or a message. */
export function absolute(path: string, origin: string): string {
	return new URL(path, origin).href;
}

/** What a picture of the upload says to someone who can't see it: its alt text, else its title,
 *  else nothing — the file name is already written beside it, so repeating it is noise. */
export function altOf(asset: Pick<Asset, 'alt' | 'title'>): string {
	const alt = asset.alt?.trim() ?? '';
	if (alt !== '') return alt;
	return asset.title?.trim() ?? '';
}

/** How an upload's state reads beside its name. */
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

/** The longer words for a state, where there is room (the details sheet). */
export function stateExplained(state: string): string {
	switch (state) {
		case 'usable':
			return 'Checked and ready to use in any entry.';
		case 'rejected':
			return "Colophon couldn't accept this file, so it can't be used.";
		case 'deleting':
			return 'Being deleted: it is no longer served, and the stored file goes shortly.';
		case 'deleted':
			return 'Deleted. The record stays so its history is still readable.';
		default:
			return 'Still being checked. It can be chosen once it is ready; this usually takes a few seconds.';
	}
}

/** A byte count for people. */
export function size(bytes: number): string {
	if (bytes < 1024) return `${bytes} B`;
	if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
	return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** What an upload is, in one line: its dimensions when known, and its size ("1200×800 · 340 KB"). */
export function measure(asset: Pick<Asset, 'width' | 'height' | 'size_bytes'>): string {
	const dimensions = asset.width !== null && asset.height !== null ? `${asset.width}×${asset.height}` : '';
	return [dimensions, size(asset.size_bytes)].filter((part) => part !== '').join(' · ');
}

/** The checker's reason, as a sentence: it starts with a capital. */
export function sentence(text: string): string {
	return text.slice(0, 1).toUpperCase() + text.slice(1);
}

/** Only settled states can be asked to go; a file still being checked waits for its verdict. */
export const deletable = (asset: Pick<Asset, 'state'>) => asset.state === 'usable' || asset.state === 'rejected';

/** Only a ready upload takes new bytes (a replacement needs a version to replace). */
export const replaceable = (asset: Pick<Asset, 'state'>) => asset.state === 'usable';

/** The tags typed into the form, split on commas and spaces, lowercased, without repeats. */
export function parseTags(text: string): string[] {
	const seen: string[] = [];
	for (const part of text.split(/[\s,]+/)) {
		const tag = part.trim().toLowerCase();
		if (tag !== '' && !seen.includes(tag)) seen.push(tag);
	}
	return seen;
}

/** The metadata form's values, as typed. */
export interface MetadataForm {
	alt: string;
	title: string;
	credit: string;
	tags: string;
}

/** The form for `asset`, as stored. */
export function formOf(asset: Pick<Asset, 'alt' | 'title' | 'credit' | 'tags'>): MetadataForm {
	return { alt: asset.alt ?? '', title: asset.title ?? '', credit: asset.credit ?? '', tags: asset.tags.join(', ') };
}

/** What is wrong with the form, per field, in plain words — the server's rules, said before it
 *  is asked. Empty when it can be saved. */
export function formProblems(form: MetadataForm): Partial<Record<keyof MetadataForm, string>> {
	const problems: Partial<Record<keyof MetadataForm, string>> = {};
	if ([...form.alt.trim()].length > LIMITS.alt) problems.alt = `Alt text is at most ${LIMITS.alt} characters.`;
	if ([...form.title.trim()].length > LIMITS.title) problems.title = `A title is at most ${LIMITS.title} characters.`;
	if ([...form.credit.trim()].length > LIMITS.credit) problems.credit = `A credit is at most ${LIMITS.credit} characters.`;
	const tags = parseTags(form.tags);
	const bad = tags.find((tag) => !TAG.test(tag));
	if (tags.length > LIMITS.tags) problems.tags = `At most ${LIMITS.tags} tags.`;
	else if (bad !== undefined)
		problems.tags = `“${bad.slice(0, 50)}” can't be a tag: use lowercase letters, digits and hyphens, at most ${LIMITS.tag} characters.`;
	return problems;
}

/** The PATCH body for `form`: every field, an emptied one cleared (`null`). */
export function metadataOf(form: MetadataForm): { alt: string | null; title: string | null; credit: string | null; tags: string[] } {
	const text = (value: string) => (value.trim() === '' ? null : value.trim());
	return { alt: text(form.alt), title: text(form.title), credit: text(form.credit), tags: parseTags(form.tags) };
}

/** Whether `form` says something different from what `asset` stores. */
export function changed(form: MetadataForm, asset: Pick<Asset, 'alt' | 'title' | 'credit' | 'tags'>): boolean {
	const now = metadataOf(form);
	return (
		now.alt !== (asset.alt ?? null) ||
		now.title !== (asset.title ?? null) ||
		now.credit !== (asset.credit ?? null) ||
		now.tags.join(',') !== asset.tags.join(',')
	);
}

/** Which form field a refusal's pointer (`/alt`, `/tags/3`) is about, if any. */
export function fieldAt(pointer: string | undefined): keyof MetadataForm | null {
	const field = pointer?.split('/')[1];
	return field === 'alt' || field === 'title' || field === 'credit' || field === 'tags' ? field : null;
}

/** Whether a drag carries files (not text or a link being dragged around the page). */
export function carriesFiles(event: DragEvent): boolean {
	return Array.from(event.dataTransfer?.types ?? []).includes('Files');
}
