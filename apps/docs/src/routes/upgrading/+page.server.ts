import { readFileSync } from 'node:fs';
import { inRepo } from '#lib/server/repository.ts';
import { upgradeGuides } from '#lib/server/repository-docs.ts';
import type { PageServerLoad } from './$types';

// The upgrade guides, newest first, with the migration range each covers (from its title).
export const load: PageServerLoad = () => ({
	guides: upgradeGuides().map((doc) => {
		const markdown = readFileSync(inRepo(doc.source), 'utf8');
		const range = /(\d{4})\D+(\d{4})/.exec(doc.title) ?? /(\d{4})\D+(\d{4})/.exec(markdown);
		return { href: doc.href, title: doc.title, source: doc.source, unreleased: doc.slug === 'unreleased', range: range ? `${range[1]}–${range[2]}` : null };
	})
});
