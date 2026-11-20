// What changed between two versions of an entry, field by field, word by word: for reviewing a
// changeset (live → pinned) and for comparing an entry's revisions. Rich text is compared as its
// readable text, block by block; the structure check is the validator's job, not the reader's.

import { blockOf, isRecord, itemField } from './content.ts';
import { prettyJson } from './json.ts';
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
	/** Rich text, compared block by block so headings, list items and quotes keep their shape
	 *  (the audit's P2-13); absent for every other field type. */
	blocks?: BlockChange[];
}

/** One block of rich text as a reader meets it: its kind (a heading keeps its level) and its text. */
export interface Block {
	kind: 'paragraph' | 'heading' | 'item' | 'quote' | 'callout' | 'code' | 'rule' | 'image';
	level?: number;
	text: string;
}

/** One block of a comparison: unchanged, added, removed, or changed in place (word by word). */
export interface BlockChange {
	kind: Block['kind'];
	level?: number;
	status: 'same' | 'added' | 'removed' | 'changed';
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

/** A rich-text document's blocks, in reading order: a list item or a quote says so, so the
 *  comparison can draw it as one. */
export function blocksOf(value: unknown): Block[] {
	const doc = ((value as { doc?: Node } | null | undefined)?.doc ?? null) as Node | null;
	const out: Block[] = [];
	const walk = (node: Node, within: Block['kind'] | null) => {
		switch (node.type) {
			case 'paragraph':
				out.push({ kind: within ?? 'paragraph', text: textOf(node) });
				return;
			case 'heading':
				out.push({ kind: 'heading', level: Number(node.attrs?.['level'] ?? 2), text: textOf(node) });
				return;
			case 'codeBlock':
				out.push({ kind: 'code', text: textOf(node) });
				return;
			case 'horizontalRule':
				out.push({ kind: 'rule', text: '———' });
				return;
			case 'image':
				out.push({ kind: 'image', text: textOf(node) });
				return;
			case 'listItem':
				for (const child of node.content ?? []) walk(child, 'item');
				return;
			case 'blockquote':
				for (const child of node.content ?? []) walk(child, 'quote');
				return;
			case 'callout':
				for (const child of node.content ?? []) walk(child, 'callout');
				return;
			default:
				for (const child of node.content ?? []) walk(child, within);
		}
	};
	if (doc !== null) walk(doc, null);
	return out;
}

/** Block-level differences: blocks equal in kind and text stay; runs of removed and added blocks
 *  between them pair up in order as changes when their kinds match, compared word by word. */
export function diffBlocks(before: Block[], after: Block[]): BlockChange[] {
	const key = (block: Block) => `${block.kind}:${block.level ?? ''}:${block.text}`;
	const n = before.length;
	const m = after.length;
	if ((n + 1) * (m + 1) > MAX_CELLS) {
		return [
			...before.map((block): BlockChange => ({ kind: block.kind, level: block.level, status: 'removed', pieces: [{ kind: 'removed', text: block.text }] })),
			...after.map((block): BlockChange => ({ kind: block.kind, level: block.level, status: 'added', pieces: [{ kind: 'added', text: block.text }] }))
		];
	}
	const width = m + 1;
	const lengths = new Uint32Array((n + 1) * width);
	for (let i = n - 1; i >= 0; i -= 1) {
		for (let j = m - 1; j >= 0; j -= 1) {
			lengths[i * width + j] =
				key(before[i] as Block) === key(after[j] as Block)
					? (lengths[(i + 1) * width + j + 1] ?? 0) + 1
					: Math.max(lengths[(i + 1) * width + j] ?? 0, lengths[i * width + j + 1] ?? 0);
		}
	}
	const out: BlockChange[] = [];
	let removed: Block[] = [];
	let added: Block[] = [];
	const flush = () => {
		const pairs = Math.max(removed.length, added.length);
		for (let index = 0; index < pairs; index += 1) {
			const was = removed[index];
			const now = added[index];
			if (was !== undefined && now !== undefined && was.kind === now.kind && was.level === now.level) {
				out.push({ kind: now.kind, level: now.level, status: 'changed', pieces: diffWords(was.text, now.text) });
			} else {
				if (was !== undefined) out.push({ kind: was.kind, level: was.level, status: 'removed', pieces: [{ kind: 'removed', text: was.text }] });
				if (now !== undefined) out.push({ kind: now.kind, level: now.level, status: 'added', pieces: [{ kind: 'added', text: now.text }] });
			}
		}
		removed = [];
		added = [];
	};
	let i = 0;
	let j = 0;
	while (i < n || j < m) {
		const was = before[i];
		const now = after[j];
		if (was !== undefined && now !== undefined && key(was) === key(now)) {
			flush();
			out.push({ kind: now.kind, level: now.level, status: 'same', pieces: now.text === '' ? [] : [{ kind: 'same', text: now.text }] });
			i += 1;
			j += 1;
		} else if (now !== undefined && (was === undefined || (lengths[i * width + j + 1] ?? 0) >= (lengths[(i + 1) * width + j] ?? 0))) {
			added.push(now);
			j += 1;
		} else if (was !== undefined) {
			removed.push(was);
			i += 1;
		}
	}
	flush();
	return out;
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
		case 'json':
			return prettyJson(value);
		// Links are entries' ids: said as how many, not as ids (D-059).
		case 'reference': {
			const count = Array.isArray(value) ? value.length : 1;
			return `${count} linked ${count === 1 ? 'entry' : 'entries'}`;
		}
		// Structures read as their parts: "Question: …; Answer: …", sections by their kind.
		case 'object':
			return isRecord(value) ? parts(field.fields, value) : '';
		case 'list':
			return Array.isArray(value) ? value.map((item) => readable(itemField(field), item)).filter((text) => text !== '').join('; ') : '';
		case 'blocks':
			return Array.isArray(value)
				? value
						.map((item, index) => {
							if (!isRecord(item)) return '';
							const kind = blockOf(field, item);
							return `${kind?.label ?? 'Section'} ${index + 1}: ${parts(kind?.fields ?? [], item)}`;
						})
						.filter((text) => text !== '')
						.join(' · ')
				: '';
		default:
			return String(value);
	}
}

function parts(fields: FieldDefinition[], value: Record<string, unknown>): string {
	return fields
		.map((field) => {
			const text = readable(field, value[field.id]);
			return text === '' ? '' : `${field.label}: ${text}`;
		})
		.filter((text) => text !== '')
		.join('; ');
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
		const change: FieldChange = { field, before: was, after: now, changed: was !== now, pieces: diffWords(was, now) };
		if (field.type === 'rich_text') {
			change.blocks = diffBlocks(before === null ? [] : blocksOf(before[field.id]), after === null ? [] : blocksOf(after[field.id]));
		}
		return change;
	});
}
