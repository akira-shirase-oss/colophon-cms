// mdsvex for the guide pages (`+page.md`): each page is wrapped in DocArticle, which draws the
// title, the capability label, the table of contents and the page's footer from the site map.

import { fileURLToPath } from 'node:url';
import { mdsvex, type MdsvexOptions } from 'mdsvex';
import { highlight } from './highlight.ts';
import { headings, scrollingTables } from './structure.ts';

// The plugins walk plain objects (structure.ts), so they fit mdsvex's bundled remark as well as
// unified 11; only the two versions' type declarations disagree.
const remarkPlugins = [[headings, { frontmatter: true }]] as unknown as NonNullable<MdsvexOptions['remarkPlugins']>;
const rehypePlugins = [scrollingTables] as unknown as NonNullable<MdsvexOptions['rehypePlugins']>;

export function markdown() {
	return mdsvex({
		extensions: ['.md'],
		layout: { _: fileURLToPath(new URL('../components/DocArticle.svelte', import.meta.url)) },
		highlight: {
			// The block is built at build time; Svelte gets it as one string, so braces and angle
			// brackets in code are never read as markup.
			highlighter: async (code: string, lang: string | null | undefined, meta: string | null | undefined) =>
				`{@html ${JSON.stringify(await highlight(code, lang ?? '', meta ?? ''))}}`
		},
		remarkPlugins,
		rehypePlugins
	});
}
