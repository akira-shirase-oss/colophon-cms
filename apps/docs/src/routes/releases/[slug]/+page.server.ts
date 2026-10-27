import { readFileSync } from 'node:fs';
import { error } from '@sveltejs/kit';
import type { Capability } from '#lib/nav.ts';
import { inRepo } from '#lib/server/repository.ts';
import { releaseNotes, renderRepoDoc } from '#lib/server/repository-docs.ts';
import type { EntryGenerator, PageServerLoad } from './$types';

// One page per milestone's release notes (docs/releases/<milestone>.md), with the milestone's
// section of CHANGELOG.md where the file says `<!-- changelog:P1 -->`.
export const entries: EntryGenerator = () => releaseNotes().map(({ slug }) => ({ slug }));

/** Done when the notes say "Released YYYY-MM-DD" (the template's line); otherwise the milestone is in progress. */
function statusOf(markdown: string, milestone: string): Capability {
	return /^\**Released:?\**\s*\d{4}-\d{2}-\d{2}/m.test(markdown) ? { kind: 'done', at: milestone } : { kind: 'building', at: milestone };
}

export const load: PageServerLoad = async ({ params }) => {
	const rendered = await renderRepoDoc(`/releases/${params.slug}/`);
	if (!rendered) error(404, 'No such release notes');
	const capability = statusOf(readFileSync(inRepo(rendered.doc.source), 'utf8'), params.slug.toUpperCase());
	return { title: rendered.title, html: rendered.html, toc: rendered.toc, source: rendered.doc.source, capability };
};
