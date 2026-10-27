// The design documents (`docs/*.md`, `docs/modules/*.md`, `docs/formats/*.md`, `docs/brand/`),
// rendered at build time from the files themselves: nothing is copied, so the site can't drift
// from them. Links between them stay on this site; links to anything else in the repository go
// to GitHub. Their headings get GitHub's ids, so `#anchors` written for GitHub still land.

import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import rehypeStringify from 'rehype-stringify';
import remarkGfm from 'remark-gfm';
import remarkParse from 'remark-parse';
import remarkRehype from 'remark-rehype';
import { unified } from 'unified';
import mark from '#lib/colophon-mark.svg';
import { highlight } from '#lib/markdown/highlight.ts';
import { headings, scrollingTables, textOf, type TocEntry } from '#lib/markdown/structure.ts';
import { blobUrl, REPOSITORY } from '#lib/nav.ts';
import { inRepo } from './repository.ts';

export interface DesignDoc {
	/** The address under /design/, without slashes at either end: `architecture`, `modules/04-api`. */
	slug: string;
	href: string;
	title: string;
	/** The file, from the repository's root. */
	source: string;
	group: 'Project' | 'Modules' | 'Formats' | 'Brand';
}

const GROUPS: { dir: string; group: DesignDoc['group'] }[] = [
	{ dir: 'docs', group: 'Project' },
	{ dir: 'docs/modules', group: 'Modules' },
	{ dir: 'docs/formats', group: 'Formats' },
	{ dir: 'docs/brand', group: 'Brand' }
];

function slugFor(source: string): string {
	const relative = source.replace(/^docs\//, '').replace(/\.md$/, '');
	// A folder's README is the folder's page: docs/modules/README.md is /design/modules/.
	const slug = relative.endsWith('README') ? relative.slice(0, -'README'.length).replace(/\/$/, '') : relative;
	return slug.toLowerCase().replaceAll('_', '-');
}

function firstHeading(markdown: string): string {
	const line = markdown.split('\n').find((text) => text.startsWith('# '));
	return line ? line.slice(2).replaceAll('`', '').trim() : '';
}

let listed: DesignDoc[] | undefined;

/** Every design document, in the order the design index shows them. */
export function designDocs(): DesignDoc[] {
	listed ??= GROUPS.flatMap(({ dir, group }) =>
		readdirSync(inRepo(dir))
			.filter((file) => file.endsWith('.md'))
			.sort((a, b) => (a === 'README.md' ? -1 : b === 'README.md' ? 1 : a.localeCompare(b)))
			.map((file) => {
				const source = `${dir}/${file}`;
				const slug = slugFor(source);
				return { slug, href: `/design/${slug}/`, title: firstHeading(readFileSync(inRepo(source), 'utf8')), source, group };
			})
	);
	return listed;
}

interface MdNode {
	type: string;
	depth?: number;
	url?: string;
	value?: string;
	lang?: string | null;
	meta?: string | null;
	children?: MdNode[];
}

/** Where a link written in `source` should go on this site, or on GitHub. */
function rewrite(url: string, source: string, docs: Map<string, DesignDoc>): string {
	if (/^[a-z][a-z0-9+.-]*:/i.test(url) || url.startsWith('#')) return url;
	const [target = '', hash] = url.split('#', 2);
	const resolved = path.posix.normalize(path.posix.join(path.posix.dirname(source), target));
	const suffix = hash === undefined ? '' : `#${hash}`;
	const doc = docs.get(resolved) ?? docs.get(path.posix.join(resolved, 'README.md'));
	if (doc) return `${doc.href}${suffix}`;
	// The mark is the one image the documents use; this site has its own copy.
	if (resolved === 'docs/brand/colophon-mark.svg') return mark;
	const kind = target.endsWith('/') ? 'tree' : 'blob';
	return kind === 'tree' ? `${REPOSITORY}/tree/main/${resolved}${suffix}` : `${blobUrl(resolved)}${suffix}`;
}

function linksAndTitle(source: string, docs: Map<string, DesignDoc>) {
	return (tree: MdNode, file: { data: Record<string, unknown> }) => {
		const visit = (node: MdNode) => {
			if ((node.type === 'link' || node.type === 'image' || node.type === 'definition') && node.url) {
				node.url = rewrite(node.url, source, docs);
			}
			node.children?.forEach(visit);
		};
		visit(tree);
		// The document's own title becomes the page's one h1, drawn by the page.
		const index = tree.children?.findIndex((node) => node.type === 'heading' && node.depth === 1) ?? -1;
		if (index >= 0 && tree.children) {
			const [heading] = tree.children.splice(index, 1);
			if (heading) file.data['title'] = textOf(heading).trim();
		}
	};
}

function codeBlocks() {
	return async (tree: MdNode) => {
		const blocks: MdNode[] = [];
		const visit = (node: MdNode) => {
			if (node.type === 'code') blocks.push(node);
			node.children?.forEach(visit);
		};
		visit(tree);
		await Promise.all(
			blocks.map(async (node) => {
				const html = await highlight(node.value ?? '', node.lang ?? '', node.meta ?? '');
				node.type = 'html';
				node.value = html;
			})
		);
	};
}

export interface RenderedDoc {
	doc: DesignDoc;
	title: string;
	html: string;
	toc: TocEntry[];
}

/** One design document as HTML, or undefined when there is no such document. */
export async function renderDesignDoc(slug: string): Promise<RenderedDoc | undefined> {
	const all = designDocs();
	// A rest parameter can arrive with the trailing slash every address here ends in.
	const wanted = slug.replace(/^\/+|\/+$/g, '');
	const doc = all.find((candidate) => candidate.slug === wanted);
	if (!doc) return undefined;
	const bySource = new Map(all.map((candidate) => [candidate.source, candidate]));
	const file = await unified()
		.use(remarkParse)
		.use(remarkGfm)
		.use(linksAndTitle, doc.source, bySource)
		.use(headings)
		.use(codeBlocks)
		.use(remarkRehype, { allowDangerousHtml: true })
		.use(scrollingTables)
		.use(rehypeStringify, { allowDangerousHtml: true })
		.process(readFileSync(inRepo(doc.source), 'utf8'));
	return {
		doc,
		title: typeof file.data['title'] === 'string' ? file.data['title'] : doc.title,
		html: String(file),
		toc: (file.data['toc'] as TocEntry[] | undefined) ?? []
	};
}
