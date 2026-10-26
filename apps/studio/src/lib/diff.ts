// What changed between two versions of an entry, field by field, word by word: for reviewing a
// changeset (live → pinned) and for comparing an entry's revisions. Rich text is compared as its
// readable text, block by block; the structure check is the validator's job, not the reader's.

import type { Content, Definition, FieldDefinition } from './api.ts';

export interface Piece {
	kind: 'same' | 'added' | 'removed';
	text: string;
}

export interface FieldChange {
	field: FieldDefinition;
	before: string;
	after: string;
	changed: boolean;
	pieces: Piece[];
}

/** The most tokens compared word by word; past this a changed field shows as replaced whole. */
const MAX_CELLS = 4_000_000;

interface Node {
	type?: string;
	text?: string;
	attrs?: Record<string, unknown>;
	content?: Node[];
}

const BLOCKS = new Set(['paragraph', 'heading', 'blockquote', 'codeBlock', 'listItem', 'callout', 'horizontalRule', 'image']);

function textOf(node: Node): string {
	if (node.type === 'text') return node.text ?? '';
	if (node.type === 'hardBreak') return '\n';
	if (node.type === 'image') return `[image: ${String(node.attrs?.['alt'] ?? '')}]`;
	if (node.type === 'horizontalRule') return '———';
	const inner = (node.content ?? []).map(textOf);
	return BLOCKS.has(node.type ?? '') || node.type === 'doc' || node.type === 'bulletList' || node.type === 'orderedList'
		? inner.filter((part) => part !== '').join('\n')
		: inner.join('');
}

/** A field's value as a reader would read it. */
export function readable(field: FieldDefinition, value: unknown): string {
	if (value === undefined || value === null || value === '') return '';
	switch (field.type) {
		case 'rich_text':
			return textOf(((value as { doc?: Node }).doc ?? {}) as Node);
		case 'boolean':
			return value === true ? 'Yes' : 'No';
		case 'datetime':
			return typeof value === 'string' ? new Date(value).toLocaleString() : String(value);
		default:
			return String(value);
	}
}

/** Word-level differences, keeping whitespace, by longest common subsequence. */
export function diffWords(before: string, after: string): Piece[] {
	if (before === after) return before === '' ? [] : [{ kind: 'same', text: before }];
	const a = before.split(/(\s+)/).filter((token) => token !== '');
	const b = after.split(/(\s+)/).filter((token) => token !== '');
	let start = 0;
	while (start < a.length && start < b.length && a[start] === b[start]) start += 1;
	let endA = a.length;
	let endB = b.length;
	while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
		endA -= 1;
		endB -= 1;
	}
	const head = a.slice(0, start).join('');
	const tail = a.slice(endA).join('');
	const midA = a.slice(start, endA);
	const midB = b.slice(start, endB);
	const middle: Piece[] = [];
	const n = midA.length;
	const m = midB.length;
	if ((n + 1) * (m + 1) > MAX_CELLS) {
		if (n > 0) middle.push({ kind: 'removed', text: midA.join('') });
		if (m > 0) middle.push({ kind: 'added', text: midB.join('') });
	} else {
		// lengths[i][j]: the longest common subsequence of midA[i..] and midB[j..].
		const width = m + 1;
		const lengths = new Uint32Array((n + 1) * width);
		for (let i = n - 1; i >= 0; i -= 1) {
			for (let j = m - 1; j >= 0; j -= 1) {
				lengths[i * width + j] =
					midA[i] === midB[j]
						? (lengths[(i + 1) * width + j + 1] ?? 0) + 1
						: Math.max(lengths[(i + 1) * width + j] ?? 0, lengths[i * width + j + 1] ?? 0);
			}
		}
		let i = 0;
		let j = 0;
		while (i < n || j < m) {
			if (i < n && j < m && midA[i] === midB[j]) {
				middle.push({ kind: 'same', text: midA[i] ?? '' });
				i += 1;
				j += 1;
			} else if (j < m && (i === n || (lengths[i * width + j + 1] ?? 0) >= (lengths[(i + 1) * width + j] ?? 0))) {
				middle.push({ kind: 'added', text: midB[j] ?? '' });
				j += 1;
			} else {
				middle.push({ kind: 'removed', text: midA[i] ?? '' });
				i += 1;
			}
		}
	}
	const pieces: Piece[] = [];
	for (const piece of [{ kind: 'same', text: head } as Piece, ...middle, { kind: 'same', text: tail } as Piece]) {
		if (piece.text === '') continue;
		const last = pieces.at(-1);
		if (last?.kind === piece.kind) last.text += piece.text;
		else pieces.push({ ...piece });
	}
	return pieces;
}

/** Every field of `definition`, from `before` (null: nothing yet) to `after` (null: taken down). */
export function compare(definition: Definition, before: Content | null, after: Content | null): FieldChange[] {
	return definition.fields.map((field) => {
		const was = before === null ? '' : readable(field, before[field.id]);
		const now = after === null ? '' : readable(field, after[field.id]);
		return { field, before: was, after: now, changed: was !== now, pieces: diffWords(was, now) };
	});
}
