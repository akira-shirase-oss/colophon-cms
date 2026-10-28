// Pages rendered at build time from the repository's own Markdown, so nothing is copied and nothing
// can drift: the design documents (`docs/*.md`, `docs/modules/`, `docs/formats/`, `docs/brand/`),
// the release notes (`docs/releases/`) and the upgrade guides (`docs/upgrading/`). Links between
// them stay on this site; links to anything else in the repository go to GitHub. Their headings get
// GitHub's ids, so `#anchors` written for GitHub still land. A release note's
// `<!-- changelog:P1 -->` marker becomes that milestone's section of CHANGELOG.md.

import { existsSync, readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import GithubSlugger from 'github-slugger';
import rehypeStringify from 'rehype-stringify';
import remarkGfm from 'remark-gfm';
import remarkParse from 'remark-parse';
import remarkRehype from 'remark-rehype';
import { unified } from 'unified';
import mark from '#lib/colophon-mark.svg';
import { DOCS_BASE } from '#lib/markdown/addresses.ts';
import { escapeHtml, highlight } from '#lib/markdown/highlight.ts';
import { headings, scrollingTables, taskLists, textOf, type TocEntry } from '#lib/markdown/structure.ts';
import { blobUrl, REPOSITORY } from '#lib/nav.ts';
import { changelog, milestoneSection, type ChangelogSection } from './changelog.ts';
import { inRepo } from './repository.ts';

export type RepoGroup = 'Project' | 'Modules' | 'Formats' | 'Brand' | 'Releases' | 'Upgrading';

export interface RepoDoc {
	/** The address under its section, without slashes at either end: `architecture`, `modules/04-api`, `p1`. */
	slug: string;
	/** The address inside the docs, without the base path: `/design/architecture/`, `/releases/p1/`. */
	href: string;
	title: string;
	/** The file, from the repository's root. */
	source: string;
	group: RepoGroup;
}

const GROUPS: { dir: string; group: RepoGroup; section: string; newestFirst?: boolean }[] = [
	{ dir: 'docs', group: 'Project', section: 'design' },
	{ dir: 'docs/modules', group: 'Modules', section: 'design' },
	{ dir: 'docs/formats', group: 'Formats', section: 'design' },
	{ dir: 'docs/brand', group: 'Brand', section: 'design' },
	{ dir: 'docs/releases', group: 'Releases', section: 'releases', newestFirst: true },
	{ dir: 'docs/upgrading', group: 'Upgrading', section: 'upgrading', newestFirst: true }
];

function slugFor(source: string, dir: string, section: string): string {
	const relative = section === 'design' ? source.replace(/^docs\//, '').replace(/\.md$/, '') : source.slice(dir.length + 1).replace(/\.md$/, '');
	// A folder's README is the folder's page: docs/modules/README.md is /design/modules/.
	const slug = relative.endsWith('README') ? relative.slice(0, -'README'.length).replace(/\/$/, '') : relative;
	return slug.toLowerCase().replaceAll('_', '-');
}

function firstHeading(markdown: string): string {
	const line = markdown.split('\n').find((text) => text.startsWith('# '));
	return line ? line.slice(2).replaceAll('`', '').trim() : '';
}

/** Newest first for release notes and guides: "unreleased" before p3-roles-schedules before p3 before p2. */
function newest(a: string, b: string): number {
	if (a.startsWith('unreleased')) return -1;
	if (b.startsWith('unreleased')) return 1;
	return b.localeCompare(a, 'en', { numeric: true });
}

let listed: RepoDoc[] | undefined;

/** Every page made from the repository's Markdown, in the order the navigation shows them. */
export function repoDocs(): RepoDoc[] {
	listed ??= GROUPS.flatMap(({ dir, group, section, newestFirst }) =>
		existsSync(inRepo(dir))
			? readdirSync(inRepo(dir))
					.filter((file) => file.endsWith('.md'))
					.sort((a, b) => (a === 'README.md' ? -1 : b === 'README.md' ? 1 : newestFirst ? newest(a, b) : a.localeCompare(b)))
					.map((file) => {
						const source = `${dir}/${file}`;
						const slug = slugFor(source, dir, section);
						const href = `/${section}/${slug}/`;
						return { slug, href, title: firstHeading(readFileSync(inRepo(source), 'utf8')), source, group };
					})
			: []
	);
	return listed;
}

/** The design documents only. */
export function designDocs(): RepoDoc[] {
	return repoDocs().filter((doc) => doc.group !== 'Releases' && doc.group !== 'Upgrading');
}

export function releaseNotes(): RepoDoc[] {
	return repoDocs().filter((doc) => doc.group === 'Releases');
}

export function upgradeGuides(): RepoDoc[] {
	return repoDocs().filter((doc) => doc.group === 'Upgrading');
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

/** GitHub's anchor for a CHANGELOG.md heading, mapped to the docs changelog's own anchor. */
function changelogAnchors(): Map<string, string> {
	const slugger = new GithubSlugger();
	const map = new Map<string, string>();
	for (const section of changelog().sections) {
		map.set(slugger.slug(section.title), section.id);
		for (const entry of section.entries) map.set(slugger.slug(`${entry.step} · ${entry.title}`), entry.id);
	}
	return map;
}

/** Where a link written in `source` should go on this site, or on GitHub. */
function rewrite(url: string, source: string, docs: Map<string, RepoDoc>): string {
	if (/^[a-z][a-z0-9+.-]*:/i.test(url) || url.startsWith('#')) return url;
	const [target = '', hash] = url.split('#', 2);
	const resolved = path.posix.normalize(path.posix.join(path.posix.dirname(source), target));
	const suffix = hash === undefined ? '' : `#${hash}`;
	const doc = docs.get(resolved) ?? docs.get(path.posix.join(resolved, 'README.md'));
	if (doc) return `${DOCS_BASE}${doc.href}${suffix}`;
	if (resolved === 'CHANGELOG.md') {
		const anchor = hash === undefined ? undefined : changelogAnchors().get(hash);
		return `${DOCS_BASE}/changelog/${anchor ? `#${anchor}` : ''}`;
	}
	// The mark is the one image the documents use; this site has its own copy.
	if (resolved === 'docs/brand/colophon-mark.svg') return mark;
	const kind = target.endsWith('/') ? 'tree' : 'blob';
	return kind === 'tree' ? `${REPOSITORY}/tree/main/${resolved}${suffix}` : `${blobUrl(resolved)}${suffix}`;
}

function linksAndTitle(source: string, docs: Map<string, RepoDoc>) {
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

/** A milestone's changelog section as HTML, each entry linked to its anchor on the changelog page. */
function sectionHtml(section: ChangelogSection): string {
	const parts = section.entries.map((entry) => {
		const when = entry.date ? `Merged ${entry.date}` : `Building on ${entry.branch ?? 'a branch'}`;
		const groups = entry.groups.map((group) => `<p class="changelog-type">${group.type}</p>${group.html}`).join('');
		return (
			`<div class="changelog-embed-entry">` +
			`<p class="changelog-embed-title"><a href="${DOCS_BASE}/changelog/#${entry.id}">${escapeHtml(`${entry.step} · ${entry.title}`)}</a></p>` +
			`<p class="changelog-embed-meta">${escapeHtml(when)}${entry.decisions ? ` · ${entry.decisions}` : ''}</p>` +
			`${entry.introHtml}${groups}</div>`
		);
	});
	return `<div class="changelog-embed" data-changelog-section="${escapeHtml(section.milestone ?? section.title)}">${parts.join('')}</div>`;
}

const MARKER = /^<!--\s*changelog:\s*([A-Za-z0-9.]+)\s*-->$/;

/** `<!-- changelog:P1 -->` becomes P1's section of CHANGELOG.md. A marker naming no section fails the build. */
function changelogSections(source: string) {
	return (tree: MdNode) => {
		const visit = (node: MdNode) => {
			if (node.type === 'html') {
				const match = MARKER.exec((node.value ?? '').trim());
				if (match) {
					const section = milestoneSection(match[1] ?? '');
					if (!section) throw new Error(`${source}: <!-- changelog:${match[1] ?? ''} --> names no section of CHANGELOG.md`);
					node.value = sectionHtml(section);
				}
			}
			node.children?.forEach(visit);
		};
		visit(tree);
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
	doc: RepoDoc;
	title: string;
	html: string;
	toc: TocEntry[];
}

/** One page as HTML, by its address (`/design/architecture/`, `/releases/p1/`), or undefined. */
export async function renderRepoDoc(href: string): Promise<RenderedDoc | undefined> {
	const all = repoDocs();
	const doc = all.find((candidate) => candidate.href === href);
	if (!doc) return undefined;
	const bySource = new Map(all.map((candidate) => [candidate.source, candidate]));
	const file = await unified()
		.use(remarkParse)
		.use(remarkGfm)
		.use(linksAndTitle, doc.source, bySource)
		.use(changelogSections, doc.source)
		.use(headings)
		.use(codeBlocks)
		.use(remarkRehype, { allowDangerousHtml: true })
		.use(scrollingTables)
		.use(taskLists)
		.use(rehypeStringify, { allowDangerousHtml: true })
		.process(readFileSync(inRepo(doc.source), 'utf8'));
	return {
		doc,
		title: typeof file.data['title'] === 'string' ? file.data['title'] : doc.title,
		html: String(file),
		toc: (file.data['toc'] as TocEntry[] | undefined) ?? []
	};
}
