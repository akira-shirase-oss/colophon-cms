// The site's pages (docs/modules/15-web-blog.md; D-052): plain HTML built from one release. Every
// page names that release in a marker probes read (colophon_domain::receipt::marker, exactly this
// form), so a page proves which release it was built from (invariant B1). No inline scripts or
// styles: the pages work under a strict Content Security Policy, like the renderer's output.

import { renderToHtml } from '@colophon/content-renderer';

/** One published article, as the public API answers. */
export interface Article {
	id: string;
	title: string;
	/** Its URL name, when its content type has a slug field and it sets one (D-054). */
	slug?: string;
	/** The canonical content: the article type's fields. */
	content: { title?: unknown; body?: unknown };
	/** The release it was published in. */
	published_in: number;
}

/** What every page is built from: the release, and when the site was built. */
export interface Release {
	generation: number;
	builtAt: Date;
}

export function escapeText(text: string): string {
	return text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
}

function escapeAttribute(text: string): string {
	return escapeText(text).replaceAll('"', '&quot;').replaceAll("'", '&#39;');
}

/** The marker every page carries, in the one form probes accept. */
export function marker(generation: number): string {
	return `<meta name="colophon-generation" content="${generation}">`;
}

function page(release: Release, title: string, main: string): string {
	return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
${marker(release.generation)}
<title>${escapeText(title)}</title>
<link rel="stylesheet" href="/style.css">
</head>
<body>
<header><a href="/">Home</a> <a href="/colophon/">Colophon</a></header>
<main>
${main}
</main>
<footer>Release ${release.generation}, built ${release.builtAt.toISOString()}.</footer>
</body>
</html>
`;
}

/** `/articles/{slug}/`, or `/articles/{id}/` for an article without a slug. */
export function articlePath(article: Article): string {
	return `/articles/${encodeURIComponent(article.slug ?? article.id)}/`;
}

export function home(release: Release, articles: Article[]): string {
	const items = articles
		.map((article) => `<li><a href="${escapeAttribute(articlePath(article))}">${escapeText(article.title)}</a></li>`)
		.join('\n');
	const list = articles.length === 0 ? '<p>Nothing published yet.</p>' : `<ul class="articles">\n${items}\n</ul>`;
	return page(release, 'Home', `<h1>Articles</h1>\n${list}`);
}

/** An article's page; its body goes through the shared renderer, or the build fails (it never guesses). */
export function article(release: Release, item: Article): string {
	const body = renderToHtml(item.content.body as Parameters<typeof renderToHtml>[0], {
		assetUrl: (assetId) => `/assets/${encodeURIComponent(assetId)}`
	});
	return page(release, item.title, `<article>\n<h1>${escapeText(item.title)}</h1>\n${body}\n</article>`);
}

/** The colophon: which release this site is, and what each article was published in. */
export function colophon(release: Release, articles: Article[]): string {
	const rows = articles
		.map((item) => `<li>${escapeText(item.title)}: release ${item.published_in}</li>`)
		.join('\n');
	return page(
		release,
		'Colophon',
		`<h1>Colophon</h1>
<p>This site is release <strong>${release.generation}</strong> of its content, built ${release.builtAt.toISOString()} from
<a href="https://github.com/akira-shirase-oss/colophon-cms">Colophon</a>. Every page says which release it was built from.</p>
<ul class="published">
${rows}
</ul>`
	);
}

export function notFound(release: Release): string {
	return page(release, 'Not found', '<h1>Not found</h1>\n<p>Nothing is published here.</p>');
}

export const STYLE = `body { font: 1rem/1.6 system-ui, sans-serif; max-width: 40rem; margin: 2rem auto; padding: 0 1rem; }
header, footer { color: #555; font-size: 0.875rem; }
header a { margin-right: 1rem; }
`;
