import { highlight } from '#lib/markdown/highlight.ts';
import { loadResults } from '#lib/server/perf.ts';
import type { PageServerLoad } from './$types';

const USAGE = `scripts/load.sh health throughput
scripts/load.sh entries steady
scripts/load.sh delivery throughput --baseline`;

// The kept load results, read from perf/results/ when the site is built, and the usage block,
// highlighted like every other code block (and checked like every runnable one).
export const load: PageServerLoad = async () => ({
	results: loadResults(),
	usage: await highlight(USAGE, 'sh', 'runnable'),
	install: await highlight('scripts/install-k6.sh', 'sh', 'runnable')
});
