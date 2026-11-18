// One page of the site. With the preview cookie it reads the draft through `/preview/v1` (the
// token as a bearer credential) and answers `private, no-store`, `noindex`, framable only by the
// Studio; without it, it reads the published entry from `/content/v1` and lets a shared cache
// keep the page for a minute.

import { error } from '@sveltejs/kit';
import {
	ApiError,
	Client,
	previewClient,
	previewHeaders,
	previewTokenFromRequest,
	type PreviewEntry,
	type PreviewSession,
	type PublishedEntryView
} from '@colophon-cms/sdk';
import type { PageServerLoad } from './$types';
import { CMS, STUDIO_ORIGIN, sitePrefix } from '../../../site.server.ts';

interface Shown {
	title: string;
	paragraphs: string[];
	revision: string | null;
	/** `draft` / `changeset` in preview mode; `published` otherwise. */
	source: string;
	hidden: string[];
}

/** The text of each top-level block of a `colophon/rich-text` document. */
function paragraphsOf(value: unknown): string[] {
	const doc = (value as { doc?: { content?: unknown } } | null)?.doc;
	const blocks = Array.isArray(doc?.content) ? (doc.content as unknown[]) : [];
	const textOf = (node: unknown): string => {
		if (typeof node !== 'object' || node === null) return '';
		const { text, content } = node as { text?: unknown; content?: unknown };
		if (typeof text === 'string') return text;
		return Array.isArray(content) ? content.map(textOf).join('') : '';
	};
	return blocks.map(textOf).filter((line) => line !== '');
}

function shown(row: PublishedEntryView | PreviewEntry, source: string, hidden: string[] = []): Shown {
	const content = (row.content ?? {}) as Record<string, unknown>;
	const title = typeof content['title'] === 'string' ? content['title'] : (row.title ?? '(untitled)');
	return { title, paragraphs: paragraphsOf(content['body']), revision: row.revision_id ?? null, source, hidden };
}

function banner(session: PreviewSession) {
	const ends = new Date(session.expiresAt);
	return {
		kind: session.scope.kind,
		endsAt: session.expiresAt,
		endsAtText: `${ends.toISOString().slice(11, 16)} UTC`
	};
}

export const load: PageServerLoad = async ({ params, request, setHeaders }) => {
	const here = `${sitePrefix(params.project)}${encodeURIComponent(params.collection)}/${encodeURIComponent(params.id)}`;
	const exit = `${sitePrefix(params.project)}exit?path=${encodeURIComponent(here)}`;
	const token = previewTokenFromRequest(request);

	// The reason a preview the cookie names stopped working, shown instead of the draft.
	let ended: string | null = null;
	if (token !== null) {
		const reader = previewClient({ baseUrl: CMS, project: params.project }).reader(token);
		try {
			const [{ data }, session] = await Promise.all([reader.get(params.collection, params.id), reader.session()]);
			setHeaders(previewHeaders({ studioOrigin: STUDIO_ORIGIN }));
			return {
				mode: 'preview' as const,
				entry: shown(data, data.preview.source, data.preview.hidden_fields ?? []),
				session: banner(session),
				ended,
				exit,
				studioOrigin: STUDIO_ORIGIN
			};
		} catch (failure) {
			if (!(failure instanceof ApiError) || failure.retryable) error(502, 'Colophon did not answer the preview read');
			ended = failure.reason ?? failure.code;
		}
	}

	// A request that carried a preview cookie stays private even when it shows the published
	// page: it says the preview ended, which no other visitor should be served.
	setHeaders({ 'Cache-Control': token === null ? 'public, max-age=0, s-maxage=60' : 'private, no-store' });
	try {
		const { data } = await new Client({ baseUrl: CMS }).delivery(params.project).get(params.collection, params.id);
		return { mode: 'published' as const, entry: shown(data, 'published'), session: null, ended, exit, studioOrigin: STUDIO_ORIGIN };
	} catch (failure) {
		if (failure instanceof ApiError && failure.status === 404) error(404, 'Not published');
		error(502, 'Colophon did not answer the published read');
	}
};
