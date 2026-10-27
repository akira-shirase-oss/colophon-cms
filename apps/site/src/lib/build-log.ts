// The build log's posts: where they come from and what each is called. Shared by the import
// script (scripts/import-blog.ts, run by Node straight from source) and the site's loader, so
// only erasable TypeScript here.

/** Where every post was first published; each copy here names its original as canonical. */
export const BUILD_LOG_ORIGIN = 'https://404human.dev/colophon/';

/** The front matter of a post, between the `+++` lines (Zola, TOML). */
export function splitFrontMatter(source: string): { frontMatter: string; body: string } {
	const match = /^\+\+\+[ \t]*\r?\n([\s\S]*?)\r?\n\+\+\+[ \t]*(?:\r?\n|$)/.exec(source);
	if (!match) throw new Error('no TOML front matter between +++ lines');
	return { frontMatter: match[1] ?? '', body: source.slice(match[0].length) };
}

/**
 * A post's slug the way Zola makes it: the front matter's `slug` when there is one, otherwise the
 * file name without `.md` and without a leading `YYYY-MM-DD-` or `YYYY-MM-DD_` date, slugified
 * (lowercase ASCII letters and digits, anything else a single hyphen).
 */
export function slugOf(fileName: string, frontMatterSlug?: unknown): string {
	const raw =
		typeof frontMatterSlug === 'string' && frontMatterSlug.length > 0
			? frontMatterSlug
			: fileName.replace(/\.md$/, '').replace(/^\d{4}-\d{2}-\d{2}[-_]/, '');
	return raw
		.normalize('NFKD')
		.replace(/[̀-ͯ]/g, '')
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '');
}

/** The address of a post's original on the build log. */
export function canonicalOf(slug: string): string {
	return `${BUILD_LOG_ORIGIN}${slug}/`;
}

/** The section's own page, which isn't a post. */
export const SECTION_INDEX = '_index.md';
