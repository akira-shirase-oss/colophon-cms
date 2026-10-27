// The changelog, parsed at build time from the repository's CHANGELOG.md: the file is the only
// place an entry is written. Its shape (see the file's header):
//
//   ## <section>                       "Unreleased", or "<milestone> · <name>"
//   ### <step> · <title>               one entry
//   - **<Key>:** <value>               metadata lines right under the heading
//   <markdown>                         the entry's body, up to the next heading
import source from '../../../../../CHANGELOG.md?raw';
import { renderInline, renderMarkdown } from './markdown.ts';

export interface ChangelogEntry {
	id: string;
	step: string;
	title: string;
	/** "merged", or the in-progress status the file gives ("building", "merging"). */
	status: string;
	date: string | null;
	commits: string[];
	branch: string | null;
	decisions: string | null;
	buildLog: string | null;
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

function slug(text: string): string {
	return text
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '');
}

function parseEntry(heading: string, lines: string[]): ChangelogEntry {
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
	if (merged && (!date || commits.length === 0)) {
		throw new Error(`CHANGELOG.md: "${heading}": Merged needs a date and at least one commit`);
	}
	const buildLog = meta.get('Build log')?.replace(/^<|>$/g, '') ?? null;
	return {
		id: `step-${slug(step)}`,
		step,
		title,
		status,
		date,
		commits,
		branch: meta.get('Branch')?.replace(/`/g, '') ?? null,
		decisions: meta.get('Decisions') ?? null,
		buildLog,
		html: renderMarkdown(lines.slice(index).join('\n'))
	};
}

export function parseChangelog(text: string): Changelog {
	const lines = text.split(/\r?\n/);
	const intro: string[] = [];
	const sections: ChangelogSection[] = [];
	let section: ChangelogSection | null = null;
	let heading: string | null = null;
	let body: string[] = [];

	const flushEntry = () => {
		if (heading !== null && section) section.entries.push(parseEntry(heading, body));
		heading = null;
		body = [];
	};

	for (const line of lines) {
		if (line.startsWith('# ')) continue;
		if (line.startsWith('## ')) {
			flushEntry();
			const title = line.slice(3).trim();
			const unreleased = title === 'Unreleased';
			section = {
				id: slug(title),
				title,
				milestone: unreleased ? null : (title.split(' · ')[0] ?? null),
				unreleased,
				entries: []
			};
			sections.push(section);
		} else if (line.startsWith('### ')) {
			flushEntry();
			if (!section) throw new Error('CHANGELOG.md: an entry before any section');
			heading = line.slice(4).trim();
		} else if (heading !== null) {
			body.push(line);
		} else if (!section) {
			intro.push(line);
		}
	}
	flushEntry();
	return { introHtml: renderMarkdown(intro.join('\n')), sections };
}

export const changelog: Changelog = parseChangelog(source);

/** Every entry in the file's order, for the sitemap and the home page. */
export function entries(): ChangelogEntry[] {
	return changelog.sections.flatMap((section) => section.entries);
}

export { renderInline };
