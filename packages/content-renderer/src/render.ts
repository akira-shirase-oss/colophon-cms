// The renderer (docs/modules/12-content-renderer.md): a stored colophon/rich-text v1 document
// to HTML. One implementation, used by the Studio preview and the public site alike, so the
// two can't drift (invariant R1).
//
// It is strict on purpose. Documents reach it only after the Rust validator accepted them, so
// anything outside the format is a bug: it throws `UnrenderableContent` rather than guess
// (R3), and it never emits markup it didn't build itself (R2). The output needs no inline
// styles or scripts, so it works under a restrictive Content Security Policy.

import {
	ASSET_ID,
	CALLOUT_TONES,
	CODE_LANGUAGE,
	HEADING_LEVELS,
	isAllowedHref,
	isValidAlt,
	RICH_TEXT_SCHEMA,
	RICH_TEXT_VERSION
} from '@colophon/editor/schema';
import type { RichTextValue, StoredMark, StoredNode } from '@colophon/editor/stored';

/** Recorded with every release, so a change to the output is a visible version change. */
export const RENDERER_VERSION = 1;

export class UnrenderableContent extends Error {
	override name = 'UnrenderableContent';
}

export interface RenderOptions {
	/** The public URL of a media-library asset. Images are served from the asset origin. */
	assetUrl: (assetId: string) => string;
}

function escapeText(text: string): string {
	return text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
}

function escapeAttribute(text: string): string {
	return escapeText(text).replaceAll('"', '&quot;').replaceAll("'", '&#39;');
}

function refuse(what: string): never {
	throw new UnrenderableContent(what);
}

/** The list of child nodes, refusing anything that isn't a list of objects. */
function children(nodes: unknown): StoredNode[] {
	if (nodes === undefined) return [];
	if (!Array.isArray(nodes)) refuse('content that is not a list');
	for (const node of nodes) {
		if (typeof node !== 'object' || node === null || typeof (node as StoredNode).type !== 'string') {
			refuse('a node that is not an object with a type');
		}
	}
	return nodes as StoredNode[];
}

function stringAttr(node: StoredNode, name: string): string {
	const value = node.attrs?.[name];
	return typeof value === 'string' ? value : refuse(`${node.type}.${name} is missing`);
}

/** Opening and closing tags for one mark. Links outside the site get rel=noopener noreferrer. */
function markTags(mark: StoredMark): [string, string] {
	if (typeof mark !== 'object' || mark === null) refuse('a mark that is not an object');
	switch (mark.type) {
		case 'bold':
			return ['<strong>', '</strong>'];
		case 'italic':
			return ['<em>', '</em>'];
		case 'strike':
			return ['<s>', '</s>'];
		case 'code':
			return ['<code>', '</code>'];
		case 'link': {
			const href = mark.attrs?.['href'];
			if (typeof href !== 'string' || !isAllowedHref(href)) refuse('a link that is not allowed');
			const external = /^https?:/i.test(href);
			return [`<a href="${escapeAttribute(href)}"${external ? ' rel="noopener noreferrer"' : ''}>`, '</a>'];
		}
		default:
			return refuse(`mark "${mark.type}"`);
	}
}

function inline(nodes: unknown): string {
	let out = '';
	for (const node of children(nodes)) {
		if (node.type === 'hardBreak') {
			out += '<br>';
		} else if (node.type === 'text') {
			if (typeof node.text !== 'string' || node.text === '') refuse('an empty text node');
			let html = escapeText(node.text);
			// Marks are stored in canonical order (link, bold, code, italic, strike): wrap so the
			// first is outermost.
			if (node.marks !== undefined && !Array.isArray(node.marks)) refuse('marks that are not a list');
			for (const mark of [...(node.marks ?? [])].reverse()) {
				const [open, close] = markTags(mark);
				html = `${open}${html}${close}`;
			}
			out += html;
		} else {
			refuse(`"${node.type}" inside a paragraph`);
		}
	}
	return out;
}

function blocks(nodes: unknown, options: RenderOptions): string {
	return children(nodes)
		.map((node) => block(node, options))
		.join('\n');
}

function block(node: StoredNode, options: RenderOptions): string {
	switch (node.type) {
		case 'paragraph':
			return `<p>${inline(node.content)}</p>`;
		case 'heading': {
			const level = node.attrs?.['level'];
			if (!HEADING_LEVELS.includes(level as 2)) refuse(`heading level ${String(level)}`);
			return `<h${String(level)}>${inline(node.content)}</h${String(level)}>`;
		}
		case 'bulletList':
			return `<ul>\n${blocks(node.content, options)}\n</ul>`;
		case 'orderedList': {
			const start = node.attrs?.['start'];
			if (start !== undefined && (!Number.isInteger(start) || (start as number) < 2)) refuse('an ordered list start');
			return `<ol${start === undefined ? '' : ` start="${String(start)}"`}>\n${blocks(node.content, options)}\n</ol>`;
		}
		case 'listItem':
			return `<li>${blocks(node.content, options)}</li>`;
		case 'blockquote':
			return `<blockquote>\n${blocks(node.content, options)}\n</blockquote>`;
		case 'codeBlock': {
			const language = node.attrs?.['language'];
			if (language !== undefined && (typeof language !== 'string' || !CODE_LANGUAGE.test(language))) {
				refuse('a code block language');
			}
			const text = children(node.content)
				.map((child) => (child.type === 'text' && !child.marks ? (child.text ?? '') : refuse('marks in a code block')))
				.join('');
			const className = language === undefined ? '' : ` class="language-${language}"`;
			return `<pre><code${className}>${escapeText(text)}</code></pre>`;
		}
		case 'horizontalRule':
			return '<hr>';
		case 'image': {
			const assetId = stringAttr(node, 'assetId');
			const alt = stringAttr(node, 'alt');
			if (!ASSET_ID.test(assetId) || !isValidAlt(alt)) refuse('an image');
			const src = options.assetUrl(assetId);
			return `<figure><img src="${escapeAttribute(src)}" alt="${escapeAttribute(alt)}" loading="lazy" decoding="async"></figure>`;
		}
		case 'callout': {
			const tone = stringAttr(node, 'tone');
			if (!(CALLOUT_TONES as readonly string[]).includes(tone)) refuse(`callout tone "${tone}"`);
			return `<aside class="callout callout-${tone}" role="note">\n${blocks(node.content, options)}\n</aside>`;
		}
		default:
			return refuse(`"${node.type}" as a block`);
	}
}

/** Renders a stored document. Throws `UnrenderableContent` for anything outside the format. */
export function renderToHtml(value: RichTextValue, options: RenderOptions): string {
	if (typeof value !== 'object' || value === null) refuse('a value that is not an object');
	if (value.schema !== RICH_TEXT_SCHEMA || value.version !== RICH_TEXT_VERSION) {
		refuse(`format ${String(value.schema)} v${String(value.version)}`);
	}
	const doc: unknown = value.doc;
	if (typeof doc !== 'object' || doc === null || (doc as StoredNode).type !== 'doc') refuse('no document');
	const content = children((doc as StoredNode).content);
	if (content.length === 0) refuse('a document without blocks');
	return blocks(content, options);
}
