import { readFileSync } from 'node:fs';
import { inRepo } from '#lib/server/repository.ts';
import { releaseNotes } from '#lib/server/repository-docs.ts';
import type { PageServerLoad } from './$types';

// The release notes, newest first, each with its first paragraph and whether it is out.
export const load: PageServerLoad = () => ({
	notes: releaseNotes().map((doc) => {
		const markdown = readFileSync(inRepo(doc.source), 'utf8');
		const released = /^\**Released:?\**\s*(\d{4}-\d{2}-\d{2})/m.exec(markdown)?.[1] ?? null;
		return { href: doc.href, title: doc.title, source: doc.source, milestone: doc.slug.toUpperCase(), released };
	})
});
