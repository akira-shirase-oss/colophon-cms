// The changelog's Atom feed (RFC 4287): one entry per merged step, newest first, built from
// CHANGELOG.md at build time. Steps still on branches aren't in it: a feed announces what happened.
// Absolute addresses use COLOPHON_SITE_ORIGIN, as the website's feed does (lib/markdown/addresses.ts).

import { DOCS_BASE, PUBLIC_ORIGIN } from '#lib/markdown/addresses.ts';
import { escapeHtml } from '#lib/markdown/highlight.ts';
import { changelog } from '#lib/server/changelog.ts';
import type { RequestHandler } from './$types';

export const prerender = true;

/** Entry ids that never change when the host does (RFC 4151 tag URIs, on the build log's domain). */
const tag = (id: string) => `tag:404human.dev,2026-10-25:colophon/changelog/${id}`;

export const GET: RequestHandler = () => {
	const page = `${PUBLIC_ORIGIN}${DOCS_BASE}/changelog/`;
	const absolute = (html: string) => html.replace(/(href|src)="\//g, `$1="${PUBLIC_ORIGIN}/`);
	const merged = changelog()
		.sections.flatMap((section) => section.entries.map((entry) => ({ section, entry })))
		.filter(({ entry }) => entry.date !== null)
		.sort((a, b) => (b.entry.date ?? '').localeCompare(a.entry.date ?? ''));
	const updated = merged[0]?.entry.date ?? '2026-10-25';
	const entries = merged.map(({ section, entry }) => {
		const body = [
			entry.introHtml,
			...entry.groups.map((group) => `<h4>${group.type}</h4>${group.html}`),
			`<p>Merged ${entry.date ?? ''}: ${entry.commits.map((commit) => `<code>${commit}</code>`).join(', ')}${entry.decisions ? `. Decisions: ${entry.decisions}` : ''}.</p>`,
			entry.buildLog ? `<p><a href="${escapeHtml(entry.buildLog)}">The build-log post</a></p>` : ''
		].join('');
		return [
			'\t<entry>',
			`\t\t<title>${escapeHtml(`${entry.step} · ${entry.title}`)}</title>`,
			`\t\t<link rel="alternate" type="text/html" href="${page}#${entry.id}"/>`,
			`\t\t<id>${tag(entry.id)}</id>`,
			`\t\t<updated>${entry.date ?? updated}T00:00:00Z</updated>`,
			`\t\t<category term="${escapeHtml(section.milestone ?? section.title)}"/>`,
			`\t\t<content type="html">${escapeHtml(absolute(body))}</content>`,
			'\t</entry>'
		].join('\n');
	});
	const xml = [
		'<?xml version="1.0" encoding="utf-8"?>',
		'<feed xmlns="http://www.w3.org/2005/Atom">',
		'\t<title>Colophon changelog</title>',
		'\t<subtitle>Every step of Colophon CMS merged into main, newest first, from CHANGELOG.md.</subtitle>',
		`\t<link rel="self" type="application/atom+xml" href="${page}feed.xml"/>`,
		`\t<link rel="alternate" type="text/html" href="${page}"/>`,
		`\t<id>${tag('feed')}</id>`,
		`\t<updated>${updated}T00:00:00Z</updated>`,
		'\t<author><name>Colophon CMS</name></author>',
		...entries,
		'</feed>',
		''
	].join('\n');
	return new Response(xml, { headers: { 'content-type': 'application/atom+xml; charset=utf-8' } });
};
