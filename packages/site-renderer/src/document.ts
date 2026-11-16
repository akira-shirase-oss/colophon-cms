// The page document (W.1): nav, a full HTML shell, and the composition one page build makes —
// title, meta, the theme stylesheet as a link (the static build) or inline (the builder's live
// canvas), navigation, rendered sections. The marker is the release's receipt, in the one form
// probes accept (docs/modules/15-web-blog.md; D-052).

import { escapeAttribute, escapeText } from './escape.ts';
import { UnrenderableSite } from './errors.ts';

export interface NavItem {
	label: string;
	href: string;
}

/** The site header's menu, from items already resolved to hrefs (page ids are the caller's). */
export function renderNav(items: NavItem[]): string {
	if (items.length === 0) return '';
	const links = items.map((item) => `<li><a href="${escapeAttribute(item.href)}">${escapeText(item.label)}</a></li>`).join('\n');
	return `<nav class="site-nav"><ul class="site-nav-list">\n${links}\n</ul></nav>`;
}

export interface PageParts {
	title: string;
	/** The page's search-listing description, when it has one. */
	description?: string;
	/** Extra <head> markup: the stylesheet link or inline style, the release marker. */
	head?: string;
	/** renderNav's output, when the site has a menu. */
	nav?: string;
	/** renderSections' output — the page's body. */
	body: string;
}

/** A full HTML document. Callers supply `head`; the body is nav plus the rendered sections. */
export function pageHtml(parts: PageParts): string {
	if (parts.title === '') throw new UnrenderableSite('a page needs a title');
	return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${escapeText(parts.title)}</title>
${parts.description === undefined || parts.description === '' ? '' : `<meta name="description" content="${escapeAttribute(parts.description)}">\n`}${parts.head ?? ''}</head>
<body>
${parts.nav ?? ''}
<main>
${parts.body}
</main>
</body>
</html>
`;
}
