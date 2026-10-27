import { DESIGN, PAGES } from '#lib/nav.ts';
import { designDocs } from '#lib/server/design-docs.ts';
import type { DesignDocLink } from '#lib/types.ts';
import type { LayoutServerLoad } from './$types';

// The design documents are listed from the repository's files at build time, and the reading
// order (for previous and next) runs through the guides and then the design documents.
export const load: LayoutServerLoad = () => {
	const design: DesignDocLink[] = designDocs().map(({ href, title, group, source }) => ({ href, title, group, source }));
	const order = [
		...PAGES.map(({ href, title }) => ({ href, title })),
		{ href: DESIGN.href, title: 'About the design documents' },
		...design.map(({ href, title }) => ({ href, title }))
	];
	return { design, order };
};
