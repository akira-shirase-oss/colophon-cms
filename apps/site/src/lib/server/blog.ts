// The build log, read at build time from src/content/blog/*.md (copied there unchanged by
// scripts/import-blog.ts). Front matter is Zola's TOML; the body is Markdown.
import { parse } from 'smol-toml';
import { SECTION_INDEX, canonicalOf, slugOf, splitFrontMatter } from '#lib/build-log.ts';
import { renderMarkdown } from './markdown.ts';

export type PostKind = 'release' | 'experiment' | 'failure';

export interface PostSummary {
	slug: string;
	file: string;
	title: string;
	shortTitle: string;
	description: string;
	/** ISO 8601, from the front matter. */
	date: string;
	kind: PostKind;
	milestone: string;
	step: string;
	canonical: string;
}

export interface Post extends PostSummary {
	glance: { goal: string; built: string; proof: string[]; code: string | null } | null;
	html: string;
}

const files = import.meta.glob('/src/content/blog/*.md', {
	query: '?raw',
	import: 'default',
	eager: true
}) as Record<string, string>;

function text(value: unknown, fallback = ''): string {
	return typeof value === 'string' ? value : fallback;
}

function record(value: unknown): Record<string, unknown> {
	return value !== null && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {};
}

function read(path: string, source: string): Post {
	const file = path.split('/').pop() ?? path;
	const { frontMatter, body } = splitFrontMatter(source);
	const meta = parse(frontMatter) as Record<string, unknown>;
	const extra = record(meta['extra']);
	const summary = record(extra['summary']);
	const date = meta['date'];
	if (typeof meta['title'] !== 'string' || !(date instanceof Date)) {
		throw new Error(`${file}: front matter needs a title and a date`);
	}
	const kind = text(extra['kind'], 'experiment');
	const slug = slugOf(file, meta['slug']);
	return {
		slug,
		file,
		title: meta['title'],
		shortTitle: text(extra['short_title'], meta['title']),
		description: text(meta['description']),
		date: date.toISOString(),
		kind: kind === 'release' || kind === 'failure' ? kind : 'experiment',
		milestone: text(extra['milestone']),
		step: text(extra['step']),
		canonical: canonicalOf(slug),
		glance:
			'goal' in summary
				? {
						goal: text(summary['goal']),
						built: text(summary['built']),
						proof: Array.isArray(summary['proof']) ? summary['proof'].map((item) => String(item)) : [],
						code: typeof summary['code'] === 'string' ? summary['code'] : null
					}
				: null,
		html: renderMarkdown(body)
	};
}

/** Every post, newest first. */
export const posts: Post[] = Object.entries(files)
	.filter(([path]) => !path.endsWith(`/${SECTION_INDEX}`))
	.map(([path, source]) => read(path, source))
	.sort((a, b) => b.date.localeCompare(a.date));

export function summaries(): PostSummary[] {
	return posts.map(({ html: _html, glance: _glance, ...summary }) => summary);
}
