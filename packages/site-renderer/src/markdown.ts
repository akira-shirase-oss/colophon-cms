// The markdown field's render subset (W.1): paragraphs on blank lines, `**bold**`, `*italic*`,
// `code` and `[text](href)`. Everything else stays literal text — the field stores a string, so
// nothing is ever lost, and the output is escaped before any marker is honoured, so content can
// never smuggle markup. Links run the same policy the rich-text renderer's do (D-022).

import { isAllowedHref } from '@colophon/editor/schema';
import { escapeAttribute, escapeText } from './escape.ts';

const INLINE = /(\*\*([^*]+)\*\*|\*([^*]+)\*|`([^`]+)`|\[([^\]]+)\]\(([^)\s]+)\))/g;

/** One line of inline marks, on already-escaped text. */
function inline(escaped: string): string {
	return escaped.replaceAll(INLINE, (whole, _mark, bold, italic, code, text, href) => {
		if (bold !== undefined) return `<strong>${bold}</strong>`;
		if (italic !== undefined) return `<em>${italic}</em>`;
		if (code !== undefined) return `<code>${code}</code>`;
		if (isAllowedHref(href)) return `<a href="${escapeAttribute(href)}">${text}</a>`;
		return whole;
	});
}

/** A markdown string to HTML paragraphs. Empty input renders nothing. */
export function renderMarkdown(source: string): string {
	return source
		.split(/\n{2,}/)
		.map((paragraph) => paragraph.trim())
		.filter((paragraph) => paragraph !== '')
		.map((paragraph) => `<p>${inline(escapeText(paragraph))}</p>`)
		.join('\n');
}
