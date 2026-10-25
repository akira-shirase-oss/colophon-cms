// Parses rendered HTML back into a DOM and checks it against an allowlist: the renderer's
// output may contain only these elements and attributes, and no URL that could run code.

import { isAllowedHref } from '@colophon/editor/schema';

const ELEMENTS = new Set(['P', 'H2', 'H3', 'H4', 'UL', 'OL', 'LI', 'BLOCKQUOTE', 'PRE', 'CODE', 'HR', 'FIGURE', 'IMG', 'ASIDE', 'STRONG', 'EM', 'S', 'A', 'BR']);
const ATTRIBUTES: Record<string, Set<string>> = {
	A: new Set(['href', 'rel']),
	IMG: new Set(['src', 'alt', 'loading', 'decoding']),
	OL: new Set(['start']),
	CODE: new Set(['class']),
	ASIDE: new Set(['class', 'role'])
};

/** Problems found in `html`; empty when it is safe. */
export function unsafeParts(html: string, assetOrigin: string): string[] {
	const template = document.createElement('template');
	template.innerHTML = html;
	const problems: string[] = [];
	for (const element of template.content.querySelectorAll('*')) {
		if (!ELEMENTS.has(element.tagName)) problems.push(`element <${element.tagName.toLowerCase()}>`);
		for (const attribute of element.attributes) {
			if (!ATTRIBUTES[element.tagName]?.has(attribute.name)) {
				problems.push(`attribute ${attribute.name} on <${element.tagName.toLowerCase()}>`);
			}
		}
		const href = element.getAttribute('href');
		if (href !== null && !isAllowedHref(href)) problems.push(`href ${href}`);
		const src = element.getAttribute('src');
		if (src !== null && !src.startsWith(assetOrigin)) problems.push(`src ${src}`);
	}
	return problems;
}
