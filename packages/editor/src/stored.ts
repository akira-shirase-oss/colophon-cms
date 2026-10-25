// The canonical conversion between the editor's JSON and the stored colophon/rich-text v1
// value (docs/formats/rich-text-v1.md, "Canonical form"). Stored -> editor -> stored is the
// identity for every valid document; the fixtures in fixtures/rich-text/v1 prove it.

import type { JSONContent } from '@tiptap/core';
import { RICH_TEXT_SCHEMA, RICH_TEXT_VERSION } from './schema.ts';

export interface StoredMark {
	type: string;
	attrs?: Record<string, string>;
}

export interface StoredNode {
	type: string;
	attrs?: Record<string, string | number>;
	text?: string;
	marks?: StoredMark[];
	content?: StoredNode[];
}

export interface RichTextValue {
	schema: typeof RICH_TEXT_SCHEMA;
	version: typeof RICH_TEXT_VERSION;
	doc: StoredNode;
}

/** Thrown when editor output contains something the stored format has no place for. Never
 *  expected (the editor's schema is the format's), and never silently dropped if it happens. */
export class UnrepresentableContent extends Error {
	override name = 'UnrepresentableContent';
}

const NODES = new Set([
	'doc',
	'paragraph',
	'heading',
	'bulletList',
	'orderedList',
	'listItem',
	'blockquote',
	'codeBlock',
	'horizontalRule',
	'image',
	'callout',
	'text',
	'hardBreak'
]);
const MARKS = new Set(['link', 'bold', 'code', 'italic', 'strike']);

/** The stored attributes of a node: only those the format defines, defaults left out. */
function storedAttrs(node: JSONContent): Record<string, string | number> | undefined {
	const attrs = node.attrs ?? {};
	switch (node.type) {
		case 'heading':
			return { level: attrs['level'] as number };
		case 'orderedList': {
			const start = attrs['start'] as number | undefined;
			return start === undefined || start === 1 ? undefined : { start };
		}
		case 'codeBlock': {
			const language = attrs['language'] as string | null | undefined;
			return language === null || language === undefined || language === '' ? undefined : { language };
		}
		case 'image':
			return { assetId: attrs['assetId'] as string, alt: attrs['alt'] as string };
		case 'callout':
			return { tone: attrs['tone'] as string };
		default:
			return undefined;
	}
}

function storedMark(mark: { type: string; attrs?: Record<string, unknown> }): StoredMark {
	if (!MARKS.has(mark.type)) throw new UnrepresentableContent(`mark "${mark.type}"`);
	if (mark.type === 'link') return { type: 'link', attrs: { href: String(mark.attrs?.['href'] ?? '') } };
	return { type: mark.type };
}

function storedNode(node: JSONContent): StoredNode {
	const type = node.type ?? '';
	if (!NODES.has(type)) throw new UnrepresentableContent(`node "${type}"`);
	// Key order is the canonical order: type, attrs, text, marks, content.
	const out: StoredNode = { type };
	const attrs = storedAttrs(node);
	if (attrs !== undefined) out.attrs = attrs;
	if (type === 'text') out.text = node.text ?? '';
	// Marks on a line break carry no meaning (the editor copies them from the surrounding text).
	if (type !== 'hardBreak' && node.marks !== undefined && node.marks.length > 0) {
		out.marks = node.marks.map(storedMark);
	}
	if (node.content !== undefined && node.content.length > 0) out.content = node.content.map(storedNode);
	return out;
}

/** Editor JSON (`editor.getJSON()`) to the stored value. */
export function toStored(doc: JSONContent): RichTextValue {
	if (doc.type !== 'doc') throw new UnrepresentableContent(`root "${doc.type ?? ''}"`);
	return { schema: RICH_TEXT_SCHEMA, version: RICH_TEXT_VERSION, doc: storedNode(doc) };
}

/** The stored value's document, for the editor. The editor fills in its own defaults. */
export function toEditor(value: RichTextValue): JSONContent {
	if (value.schema !== RICH_TEXT_SCHEMA || value.version !== RICH_TEXT_VERSION) {
		throw new UnrepresentableContent(`format ${String(value.schema)} v${String(value.version)}`);
	}
	return value.doc as JSONContent;
}

/** The canonical serialization: what is stored, and what "byte-for-byte identical" compares. */
export function serialize(value: RichTextValue): string {
	return JSON.stringify(value);
}

/** Depth, node count and canonical size, for the editor's status line. Rust enforces the limits. */
export function measure(value: RichTextValue): { depth: number; nodes: number; bytes: number } {
	let nodes = 0;
	const walk = (node: StoredNode, depth: number): number => {
		nodes += 1;
		return Math.max(depth, ...(node.content ?? []).map((child) => walk(child, depth + 1)));
	};
	const depth = walk(value.doc, 1);
	return { depth, nodes, bytes: new TextEncoder().encode(serialize(value)).length };
}
