import { parityCounts } from '#lib/data/compare.ts';
import { summaries } from '#lib/server/blog.ts';
import { entries } from '#lib/server/changelog.ts';
import { failedCount, figures, resultCount, runs } from '#lib/server/perf.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = () => {
	const changelog = entries();
	return {
		figures,
		resultCount,
		failedCount,
		runs,
		parity: parityCounts(),
		merged: changelog.filter((entry) => entry.status === 'merged' && /^\d/.test(entry.step)).length,
		latest: changelog.find((entry) => entry.status === 'merged') ?? null,
		posts: summaries().slice(0, 3)
	};
};
