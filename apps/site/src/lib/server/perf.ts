// Load-test results, read at build time from perf/results/*.json: the site shows no number that
// isn't in one of these files. Each figure names the file it came from, what that run measured,
// the machine and the commit.
export interface PerfResult {
	scenario: string;
	endpoint: string;
	date: string;
	commit: string;
	dirty: boolean;
	machine: { id: string; cpu: string; cores: number; memory_gb: number; os: string };
	k6: string;
	params: Record<string, number | string>;
	requests: number;
	rps: number;
	failed: number;
	checks_failed: number;
	dropped: number;
	latency_ms: { avg: number; p50: number; p90: number; p95: number; p99: number; p999: number; max: number };
	thresholds_passed: boolean;
}

/**
 * A result file records whether k6's thresholds passed, but not the regression check against the
 * machine's baseline (scripts/load.sh prints that verdict). For the figures shown, the verdict comes
 * from the commit that added the file; 'none' when there was no baseline to compare with.
 */
export type Regression = 'passed' | 'missed' | 'none';

export interface Figure {
	file: string;
	url: string;
	title: string;
	measured: string;
	regression: Regression;
	result: PerfResult;
}

const files = import.meta.glob('../../../../../perf/results/*.json', {
	eager: true,
	import: 'default'
}) as Record<string, PerfResult>;

const REPOSITORY = 'https://github.com/akira-shirase-oss/colophon-cms/blob/main/perf/results/';

function load(file: string, regression: Regression, title: string, measured: string): Figure {
	const entry = Object.entries(files).find(([path]) => path.endsWith(`/${file}`));
	if (!entry) throw new Error(`perf/results/${file} is missing: the site only shows numbers from these files`);
	return { file, url: REPOSITORY + file, title, measured, regression, result: entry[1] };
}

/** How many committed load results there are, failures included. */
export const resultCount = Object.keys(files).length;
/** How many missed their own k6 thresholds (runs kept for missing the regression gate come on top). */
export const failedCount = Object.values(files).filter((result) => !result.thresholds_passed).length;

/** The figures the site shows, each with what its run measured (from the commit that added it). */
export const figures = {
	// 63457ce: the site's first run; no baseline yet.
	site: load(
		'2026-10-26T193403Z-site-throughput-apple-m2-pro-12c.json',
		'none',
		'Reading the site while it was republished',
		'Page reads from the reference site’s runner while a release was published every two seconds, 27 in all, each built and swapped in underneath the readers. Every page read whole, with exactly one release marker. The 4 failed requests were the setup’s polls for the first build, before anything was live.'
	),
	// d7624a9: "no regression against the baseline".
	entriesFresh: load(
		'2026-10-27T140343Z-entries-throughput-apple-m2-pro-12c.json',
		'passed',
		'Saving and reading drafts, fresh database',
		'Signed-in requests on drafts, on a fresh database: half reads, a fifth saves with If-Match and a fresh idempotency key, the rest lists, title searches and revision reads. Each virtual user is its article’s only writer, so every read must return exactly its last accepted save; every check passed.'
	),
	// d7624a9: "p99 183.8 ms ... over the gate".
	entriesLarge: load(
		'2026-10-27T135247Z-entries-throughput-apple-m2-pro-12c.json',
		'missed',
		'The same, on a 5 GB database',
		'The same run on a local database grown to 5 GB by earlier load runs (2.4 M idempotency records). It missed the regression gate, and is kept: saves slow down as that table grows, which milestone R addresses.'
	),
	// eb6206e: "No regression against the 3.1 baseline either time".
	delivery: load(
		'2026-10-26T163022Z-delivery-throughput-apple-m2-pro-12c.json',
		'passed',
		'Public reads through the cache',
		'Anonymous reads of the latest release, of pinned past releases, and of an entry taken down (refused, never cached), through the release-keyed cache.'
	),
	// 74a5644; there is no publishing throughput baseline (perf/baselines), so no comparison.
	publishing: load(
		'2026-10-26T185935Z-publishing-throughput-apple-m2-pro-12c.json',
		'none',
		'Publishing, with receipts',
		'Each iteration creates an article, pins it into a publish request, submits, approves and publishes it (five requests), while the worker announces every release to a webhook, purges a CDN and probes the public API. Every announcement and purge carried a distinct key, and every listed release was verified at one location.'
	)
} satisfies Record<string, Figure>;

/** Every result, oldest first, for the chart of runs. */
export const runs = Object.entries(files)
	.map(([path, result]) => ({
		file: path.split('/').pop() ?? path,
		endpoint: result.endpoint,
		scenario: result.scenario,
		date: result.date,
		rps: result.rps,
		p99: result.latency_ms.p99,
		ok: result.thresholds_passed
	}))
	.sort((a, b) => a.date.localeCompare(b.date));
