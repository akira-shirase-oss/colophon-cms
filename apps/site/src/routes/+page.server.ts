import { PARITY, TEASER } from '#lib/data/compare.ts';
import { summaries } from '#lib/server/blog.ts';
import { entries } from '#lib/server/changelog.ts';
import { failedCount, figures, resultCount } from '#lib/server/perf.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = () => {
	const changelog = entries();
	return {
		figures,
		resultCount,
		failedCount,
		merged: changelog.filter((entry) => entry.status === 'merged' && /^\d/.test(entry.step)).length,
		latest: changelog.find((entry) => entry.status === 'merged') ?? null,
		posts: summaries().slice(0, 3),
		parity: TEASER.map((feature) => {
			const row = PARITY.find((candidate) => candidate.feature === feature);
			if (!row) throw new Error(`no parity row named ${feature}`);
			return row;
		})
	};
};
