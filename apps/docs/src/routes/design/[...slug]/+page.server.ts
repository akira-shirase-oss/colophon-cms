import { error } from '@sveltejs/kit';
import { designDocs, renderRepoDoc } from '#lib/server/repository-docs.ts';
import type { EntryGenerator, PageServerLoad } from './$types';

// One page per design document, rendered from the file at build time.
export const entries: EntryGenerator = () => designDocs().map(({ slug }) => ({ slug }));

export const load: PageServerLoad = async ({ params }) => {
	const rendered = await renderRepoDoc(`/design/${params.slug.replace(/^\/+|\/+$/g, '')}/`);
	if (!rendered) error(404, 'No such design document');
	return { title: rendered.title, html: rendered.html, toc: rendered.toc, source: rendered.doc.source, group: rendered.doc.group };
};
