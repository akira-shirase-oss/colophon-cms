import { error } from '@sveltejs/kit';
import type { Capability } from '#lib/nav.ts';
import { renderRepoDoc, upgradeGuides } from '#lib/server/repository-docs.ts';
import type { EntryGenerator, PageServerLoad } from './$types';

// One page per upgrade guide (docs/upgrading/<range>.md; the template is docs/templates/upgrade-guide.md).
export const entries: EntryGenerator = () => upgradeGuides().map(({ slug }) => ({ slug }));

export const load: PageServerLoad = async ({ params }) => {
	const rendered = await renderRepoDoc(`/upgrading/${params.slug}/`);
	if (!rendered) error(404, 'No such upgrade guide');
	// The guide for migrations still on branches is Building; the others cover what is on main.
	const capability: Capability = params.slug === 'unreleased' ? { kind: 'building', at: 'branches' } : { kind: 'done', at: params.slug.toUpperCase() };
	return { title: rendered.title, html: rendered.html, toc: rendered.toc, source: rendered.doc.source, capability };
};
