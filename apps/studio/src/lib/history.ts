// An entry's history as people read it (D-184): autosaves a person made within five minutes of
// each other are one sitting, said once with who, when and what changed ("Body: +2 paragraphs ·
// Title edited"), instead of a row per save that differs only by its time.

import type { Content, Definition, FieldDefinition, Revision } from './api.ts';
import { readable } from './diff.ts';

/** Saves this close together, by the same person, are one sitting. */
export const SITTING_GAP_MS = 5 * 60_000;

/** One sitting: consecutive saves by one person, newest first. */
export interface Sitting {
	revisions: Revision[];
	/** The newest save: the entry as the sitting left it. */
	last: Revision;
	/** The oldest save. */
	first: Revision;
	author: string;
}

/** `revisions` (newest first, as the API lists them) grouped into sittings. A revision in `ends`
 *  (the live one) is always the newest of its sitting, so restoring a sitting can restore it. */
export function sittings(revisions: Revision[], ends: (string | null | undefined)[] = []): Sitting[] {
	const out: Sitting[] = [];
	for (const revision of revisions) {
		const current = out.at(-1);
		const older = current?.first;
		const joins =
			current !== undefined &&
			older !== undefined &&
			older.author_id === revision.author_id &&
			new Date(older.created_at).getTime() - new Date(revision.created_at).getTime() <= SITTING_GAP_MS &&
			!ends.includes(revision.id);
		if (joins) {
			current.revisions.push(revision);
			current.first = revision;
		} else {
			out.push({ revisions: [revision], last: revision, first: revision, author: revision.author_id });
		}
	}
	return out;
}

interface Node {
	type?: string;
	content?: Node[];
}

/** How many of each kind of block a rich text holds, for "+2 paragraphs, +1 image". */
function blocks(value: unknown): Record<'paragraph' | 'heading' | 'image', number> {
	const counts = { paragraph: 0, heading: 0, image: 0 };
	const walk = (node: Node) => {
		if (node.type === 'paragraph' || node.type === 'heading' || node.type === 'image') counts[node.type] += 1;
		for (const child of node.content ?? []) walk(child);
	};
	const doc = (value as { doc?: Node } | undefined)?.doc;
	if (doc !== undefined) walk(doc);
	return counts;
}

const NOUNS = { paragraph: ['paragraph', 'paragraphs'], heading: ['heading', 'headings'], image: ['image', 'images'] } as const;

/** What changed in one rich-text field: "+2 paragraphs, −1 image", or "edited". */
function richChange(before: unknown, after: unknown): string {
	const was = blocks(before);
	const now = blocks(after);
	const parts: string[] = [];
	for (const kind of ['paragraph', 'heading', 'image'] as const) {
		const delta = now[kind] - was[kind];
		if (delta !== 0) parts.push(`${delta > 0 ? '+' : '−'}${Math.abs(delta)} ${NOUNS[kind][Math.abs(delta) === 1 ? 0 : 1]}`);
	}
	return parts.length > 0 ? parts.join(', ') : 'edited';
}

/** One field's change in a few words, or '' when it didn't change. */
function fieldChange(field: FieldDefinition, before: Content | null, after: Content): string {
	const was = before?.[field.id];
	const now = after[field.id];
	if (field.type === 'rich_text') {
		if (JSON.stringify(was ?? null) === JSON.stringify(now ?? null)) return '';
		return `${field.label}: ${richChange(was, now)}`;
	}
	const wasText = was === undefined ? '' : readable(field, was);
	const nowText = now === undefined ? '' : readable(field, now);
	if (wasText === nowText) return '';
	if (wasText === '') return `${field.label} added`;
	if (nowText === '') return `${field.label} cleared`;
	return `${field.label} edited`;
}

/** What a sitting changed, from the entry `before` it (null: it was the first) to `after`, in
 *  words: "Title edited · Body: +2 paragraphs, +1 image". At most three fields, then "and N more". */
export function changeSummary(definition: Definition, before: Content | null, after: Content): string {
	if (before === null) return 'First version';
	const changes = definition.fields.map((field) => fieldChange(field, before, after)).filter((text) => text !== '');
	if (changes.length === 0) return 'No change to the content';
	const shown = changes.slice(0, 3).join(' · ');
	return changes.length > 3 ? `${shown} · and ${changes.length - 3} more` : shown;
}

/** "22:04" today ("10:04 PM" where the clock has twelve hours), "17 Nov, 22:04" this year,
 *  "17 Nov 2025, 22:04" before. */
export function clock(at: string, now = new Date()): string {
	const when = new Date(at);
	const time = when.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
	if (when.toDateString() === now.toDateString()) return time;
	const date = when.toLocaleDateString(undefined, { day: 'numeric', month: 'short', ...(when.getFullYear() === now.getFullYear() ? {} : { year: 'numeric' }) });
	return `${date}, ${time}`;
}

/** Up to two initials for an avatar: "Dev Owner" → "DO". */
export function initials(name: string): string {
	const words = name.trim().split(/\s+/).filter((word) => word !== '');
	return words
		.slice(0, 2)
		.map((word) => word[0]?.toUpperCase() ?? '')
		.join('');
}
