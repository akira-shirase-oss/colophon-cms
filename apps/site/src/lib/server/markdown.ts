// Markdown to HTML at build time, the way the build log's Zola site does it: CommonMark with smart
// punctuation (curly quotes, en and em dashes, ellipses), raw HTML escaped rather than passed
// through. Only trusted files from this repository go through it, and only while prerendering.
import MarkdownIt from 'markdown-it';

const markdown = new MarkdownIt({ html: false, linkify: false, typographer: true });

// A relative link in a repository file (`docs/ROADMAP.md`) means a file in the repository, not a
// page of this site: point it at the file on GitHub. Links in the build log are all absolute.
const REPOSITORY_FILES = 'https://github.com/akira-shirase-oss/colophon-cms/blob/main/';
markdown.core.ruler.push('repository_links', (state) => {
	for (const block of state.tokens) {
		for (const token of block.children ?? []) {
			if (token.type !== 'link_open') continue;
			const href = token.attrGet('href') ?? '';
			if (href && !/^([a-z][a-z0-9+.-]*:|#|\/)/i.test(href)) {
				token.attrSet('href', REPOSITORY_FILES + href.replace(/^\.\//, ''));
			}
		}
	}
});

export function renderMarkdown(source: string): string {
	return markdown.render(source);
}

export function renderInline(source: string): string {
	return markdown.renderInline(source);
}
