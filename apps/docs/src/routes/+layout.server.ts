import { DESIGN, PAGES } from '#lib/nav.ts';
import { designDocs, releaseNotes, upgradeGuides } from '#lib/server/repository-docs.ts';
import type { RepoLinks } from '#lib/types.ts';
import type { LayoutServerLoad } from './$types';

// The pages made from the repository's files are listed at build time, and the reading order (for
// previous and next) runs through the guides, with each milestone's release notes after the
// release notes page and each upgrade guide after the guides page, then the design documents.
export const load: LayoutServerLoad = () => {
	const link = ({ href, title }: { href: string; title: string }) => ({ href, title });
	const repo: RepoLinks = {
		releases: releaseNotes().map(link),
		upgrades: upgradeGuides().map(link),
		design: designDocs().map(({ href, title, group, source }) => ({ href, title, group: group as RepoLinks['design'][number]['group'], source }))
	};
	const nested: Record<string, { href: string; title: string }[]> = { '/releases/': repo.releases, '/upgrading/': repo.upgrades };
	const order = [
		...PAGES.flatMap((page) => [link(page), ...(nested[page.href] ?? [])]),
		{ href: DESIGN.href, title: 'About the design documents' },
		...repo.design.map(link)
	];
	return { repo, order };
};
