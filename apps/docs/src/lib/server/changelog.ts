// The changelog, parsed at build time from the repository's CHANGELOG.md, the only place an entry is
// written (docs/RELEASES.md). The website parses the same file (apps/site/src/lib/server/changelog.ts)
// into the same sections, entries and anchors; this one also keeps each entry's Keep a Changelog
// groups (Added, Changed, …) apart, for the docs' page, its feed and the release notes. Its shape:
//
//   ## <section>                       "Unreleased", or "<milestone> · <name>"
//   ### <step> · <title>               one entry
//   - **<Key>:** <value>               metadata lines right under the heading
//   #### Added | Changed | …           the entry's changes, grouped by type
//
// A malformed entry fails the build, naming the entry.

import { readFileSync } from 'node:fs';
import rehypeStringify from 'rehype-stringify';
import remarkGfm from 'remark-gfm';
import remarkParse from 'remark-parse';
import remarkRehype from 'remark-rehype';
import { unified } from 'unified';
import { DOCS_BASE } from '#lib/markdown/addresses.ts';
import { blobUrl } from '#lib/nav.ts';
import { inRepo } from './repository.ts';

export const TYPES = ['Added', 'Changed', 'Deprecated', 'Removed', 'Fixed', 'Security'] as const;
export type ChangeType = (typeof TYPES)[number];

export interface ChangeGroup {
	type: ChangeType;
	html: string;
	/** The group's Markdown, for the feed's plain-text summary. */
	markdown: string;
}

export interface ChangelogEntry {
	id: string;
	step: string;
	title: string;
	/** "merged", or the in-progress status the file gives ("building"). */
	status: string;
	date: string | null;
	commits: string[];
	branch: string | null;
	/** The Decisions line, rendered (it may have code and links). */
	decisions: string | null;
	buildLog: string | null;
	/** The Proof line, rendered. */
	proof: string | null;
	/** Text before the first group, if any (rendered). */
	introHtml: string;
	groups: ChangeGroup[];
	/** The entry's whole body, rendered: for the feed. */
	html: string;
}

export interface ChangelogSection {
	id: string;
	title: string;
	milestone: string | null;
	unreleased: boolean;
	entries: ChangelogEntry[];
}

export interface Changelog {
	introHtml: string;
	sections: ChangelogSection[];
}

const META = /^- \*\*([A-Za-z ]+):\*\* (.+)$/;

/** The anchor slug both parsers use: lowercase, runs of anything else become one hyphen. */
export function slug(text: string): string {
	return text
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '');
}

/** Links in the file are written for GitHub: repository paths go to GitHub, the rest stay. */
function repositoryLinks() {
	return (tree: { type: string; url?: string; children?: unknown[] }) => {
		const visit = (node: { type: string; url?: string; children?: unknown[] }) => {
			if ((node.type === 'link' || node.type === 'definition') && node.url && !/^[a-z][a-z0-9+.-]*:/i.test(node.url) && !node.url.startsWith('#')) {
				const [target = '', hash] = node.url.split('#', 2);
				const clean = target.replace(/^\.\//, '');
				node.url = clean === 'CHANGELOG.md' ? `${DOCS_BASE}/changelog/${hash ? `#${hash}` : ''}` : `${blobUrl(clean)}${hash ? `#${hash}` : ''}`;
			}
			(node.children as (typeof node)[] | undefined)?.forEach(visit);
		};
		visit(tree);
	};
}

/** One line of Markdown to HTML without its paragraph, for a metadata value. */
export function renderInline(markdown: string): string {
	return render(markdown).trim().replace(/^<p>([\s\S]*)<\/p>$/, '$1');
}

/** Markdown to HTML, raw HTML escaped (the file is the project's own, but nothing in it needs HTML). */
export function render(markdown: string): string {
	return String(
		unified()
			.use(remarkParse)
			.use(remarkGfm)
			.use(repositoryLinks)
			.use(remarkRehype)
			.use(rehypeStringify)
			.processSync(markdown)
	);
}

function parseEntry(heading: string, lines: string[], section: string): Omit<ChangelogEntry, 'id'> & { baseId: string } {
	const [step = '', ...rest] = heading.split(' · ');
	const title = rest.join(' · ');
	if (!title) throw new Error(`CHANGELOG.md: "### ${heading}" needs "<step> · <title>"`);
	const meta = new Map<string, string>();
	let index = 0;
	while (index < lines.length && (lines[index] ?? '').trim() === '') index++;
	for (; index < lines.length; index++) {
		const match = META.exec(lines[index] ?? '');
		if (!match) break;
		meta.set(match[1] ?? '', match[2] ?? '');
	}
	const merged = meta.get('Merged') ?? null;
	const date = merged ? (/^(\d{4}-\d{2}-\d{2})/.exec(merged)?.[1] ?? null) : null;
	const commits = merged ? [...merged.matchAll(/`([0-9a-f]{7,40})`/g)].map((match) => match[1] ?? '') : [];
	const status = merged ? 'merged' : (meta.get('Status') ?? '').trim();
	if (!status) throw new Error(`CHANGELOG.md: "${heading}" needs a Merged or a Status line`);
	if (merged && (!date || commits.length === 0)) throw new Error(`CHANGELOG.md: "${heading}": Merged needs a date and at least one commit`);
	if (!merged && section !== 'Unreleased') throw new Error(`CHANGELOG.md: "${heading}" isn't merged, so it belongs under "## Unreleased"`);

	const body = lines.slice(index);
	const intro: string[] = [];
	const groups: { type: ChangeType; lines: string[] }[] = [];
	for (const line of body) {
		if (line.startsWith('#### ')) {
			const type = line.slice(5).trim();
			if (!(TYPES as readonly string[]).includes(type)) {
				throw new Error(`CHANGELOG.md: "${heading}" has "#### ${type}"; the types are ${TYPES.join(', ')}`);
			}
			groups.push({ type: type as ChangeType, lines: [] });
		} else if (groups.length > 0) {
			groups.at(-1)?.lines.push(line);
		} else {
			intro.push(line);
		}
	}
	return {
		baseId: `step-${slug(step)}`,
		step,
		title,
		status,
		date,
		commits,
		branch: meta.get('Branch')?.replace(/`/g, '') ?? null,
		decisions: meta.has('Decisions') ? renderInline(meta.get('Decisions') ?? '') : null,
		buildLog: meta.get('Build log')?.replace(/^<|>$/g, '') ?? null,
		proof: meta.has('Proof') ? renderInline(meta.get('Proof') ?? '') : null,
		introHtml: intro.join('\n').trim() ? render(intro.join('\n')) : '',
		groups: groups.map((group) => ({ type: group.type, markdown: group.lines.join('\n').trim(), html: render(group.lines.join('\n')) })),
		html: render(body.join('\n'))
	};
}

export function parseChangelog(text: string): Changelog {
	const lines = text.split(/\r?\n/);
	const intro: string[] = [];
	const sections: ChangelogSection[] = [];
	const raw: { section: ChangelogSection; heading: string; body: string[] }[] = [];
	let section: ChangelogSection | null = null;
	let current: { section: ChangelogSection; heading: string; body: string[] } | null = null;

	for (const line of lines) {
		if (line.startsWith('# ')) continue;
		if (line.startsWith('## ')) {
			current = null;
			const title = line.slice(3).trim();
			const unreleased = title === 'Unreleased';
			section = { id: slug(title), title, milestone: unreleased ? null : (title.split(' · ')[0] ?? null), unreleased, entries: [] };
			sections.push(section);
		} else if (line.startsWith('### ')) {
			if (!section) throw new Error('CHANGELOG.md: an entry before any section');
			current = { section, heading: line.slice(4).trim(), body: [] };
			raw.push(current);
		} else if (current) {
			current.body.push(line);
		} else if (!section) {
			intro.push(line);
		}
	}

	// Anchors: `step-<step>` (`#step-3-5`), as the website makes them; a step that appears twice
	// ("Fix") gets its title too, so every anchor is unique.
	const parsed = raw.map(({ section, heading, body }) => ({ section, entry: parseEntry(heading, body, section.title) }));
	const count = new Map<string, number>();
	for (const { entry } of parsed) count.set(entry.baseId, (count.get(entry.baseId) ?? 0) + 1);
	for (const { section, entry } of parsed) {
		const { baseId, ...rest } = entry;
		const id = (count.get(baseId) ?? 0) > 1 ? `${baseId}-${slug(entry.title)}` : baseId;
		section.entries.push({ id, ...rest });
	}
	return { introHtml: render(intro.join('\n')), sections };
}

let cached: Changelog | undefined;

/** CHANGELOG.md, parsed once per build. */
export function changelog(): Changelog {
	cached ??= parseChangelog(readFileSync(inRepo('CHANGELOG.md'), 'utf8'));
	return cached;
}

/** One milestone's section ("P1"), for its release notes. */
export function milestoneSection(milestone: string): ChangelogSection | undefined {
	return changelog().sections.find((section) => section.milestone?.toLowerCase() === milestone.toLowerCase());
}
