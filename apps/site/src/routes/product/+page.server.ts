import { failedCount, figures, resultCount, runs } from '#lib/server/perf.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = () => ({ figures, resultCount, failedCount, runs });
