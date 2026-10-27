// Tree plugins shared by the two Markdown pipelines (mdsvex's bundled remark and unified 11): ids
// on headings, the page's table of contents, and tables that scroll on a phone instead of
// widening the page. They walk plain objects, so the two versions of the syntax tree both fit.

import GithubSlugger from 'github-slugger';

/** One heading in a page's table of contents. */
export interface TocEntry {
	depth: 2 | 3;
	id: string;
	text: string;
}

interface Node {
	type: string;
	depth?: number;
	value?: string;
	tagName?: string;
	properties?: Record<string, unknown>;
	children?: Node[];
	data?: { hProperties?: Record<string, unknown> } & Record<string, unknown>;
}

interface File {
	data: Record<string, unknown> & { fm?: Record<string, unknown> };
}

const ENTITIES: Record<string, string> = { '&#123;': '{', '&#125;': '}', '&lt;': '<', '&gt;': '>', '&amp;': '&' };

/** A node's text, as a reader sees it. mdsvex escapes braces and angle brackets in inline code. */
export function textOf(node: Node): string {
	if (node.type === 'text' || node.type === 'inlineCode') {
		return (node.value ?? '').replace(/&#123;|&#125;|&lt;|&gt;|&amp;/g, (entity) => ENTITIES[entity] ?? entity);
	}
	// Inline components (`<Status … />`) are labels, not part of the heading's words.
	if (node.type === 'html') return '';
	return (node.children ?? []).map(textOf).join('');
}

function walk(node: Node, visit: (node: Node) => void) {
	visit(node);
	for (const child of node.children ?? []) walk(child, visit);
}

/**
 * A remark plugin: every heading gets the id GitHub would give it (so links into the design
 * documents keep working), and the h2s and h3s become the table of contents, stored on the file
 * (`file.data.toc`) and, for mdsvex, in the page's frontmatter, which its layout receives.
 */
export function headings(options: { frontmatter?: boolean } = {}) {
	return (tree: Node, file: File) => {
		const slugger = new GithubSlugger();
		const toc: TocEntry[] = [];
		walk(tree, (node) => {
			if (node.type !== 'heading') return;
			const text = textOf(node).trim();
			const id = slugger.slug(text);
			node.data = { ...node.data, hProperties: { ...node.data?.hProperties, id } };
			if (node.depth === 2 || node.depth === 3) toc.push({ depth: node.depth, id, text });
		});
		file.data['toc'] = toc;
		if (options.frontmatter) {
			file.data.fm = { ...file.data.fm, toc };
		}
	};
}

const WRAPPER = 'table-scroll';

/**
 * A rehype plugin: each table sits in a box that scrolls sideways on a narrow screen, so the
 * page itself never does (WCAG 1.4.10, reflow). The page makes a box focusable only when it
 * actually overflows, so wide tables can be scrolled from the keyboard without adding a tab stop
 * for every table that fits.
 */
export function scrollingTables() {
	return (tree: Node) => {
		walk(tree, (node) => {
			const classes = node.properties?.['className'];
			if (!node.children || (Array.isArray(classes) && classes.includes(WRAPPER))) return;
			node.children = node.children.map((child) =>
				child.type === 'element' && child.tagName === 'table'
					? { type: 'element', tagName: 'div', properties: { className: [WRAPPER] }, children: [child] }
					: child
			);
		});
	};
}
