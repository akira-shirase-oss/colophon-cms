// Load results, read at build time from perf/results/*.json: the files `scripts/load.sh` writes,
// passes and misses alike. The docs show these numbers and no others (module 16: no uncited
// speed claims), each with the machine, the commit and what was measured.

import { readdirSync, readFileSync } from 'node:fs';
import { inRepo } from './repository.ts';

export interface LoadResult {
	file: string;
	date: string;
	endpoint: string;
	scenario: string;
	commit: string;
	dirty: boolean;
	machine: { id: string; cpu: string; cores: number; memory_gb: number; os: string };
	k6: string;
	requests: number;
	rps: number;
	failed: number;
	checks_failed: number;
	dropped: number;
	p50: number;
	p99: number;
	thresholds_passed: boolean;
	params: Record<string, number>;
}

interface RawResult {
	date: string;
	endpoint: string;
	scenario: string;
	commit: string;
	dirty: boolean;
	machine: LoadResult['machine'];
	k6: string;
	params: Record<string, number>;
	requests: number;
	rps: number;
	failed: number;
	checks_failed: number;
	dropped: number;
	latency_ms: { p50: number; p99: number };
	thresholds_passed: boolean;
}

/** Every kept result, newest first. */
export function loadResults(): LoadResult[] {
	const dir = inRepo('perf', 'results');
	return readdirSync(dir)
		.filter((file) => file.endsWith('.json'))
		.map((file) => ({ file, raw: JSON.parse(readFileSync(`${dir}/${file}`, 'utf8')) as RawResult }))
		// Stress runs (D-077) and micro-benchmarks (D-074) sit in the same folder with shapes of
		// their own; this page shows throughput and steady runs.
		.filter(({ raw }) => raw.scenario === 'throughput' || raw.scenario === 'steady')
		.map(({ file, raw }) => {
			return {
				file: `perf/results/${file}`,
				date: raw.date,
				endpoint: raw.endpoint,
				scenario: raw.scenario,
				commit: raw.commit,
				dirty: raw.dirty,
				machine: raw.machine,
				k6: raw.k6,
				requests: raw.requests,
				rps: raw.rps,
				failed: raw.failed,
				checks_failed: raw.checks_failed,
				dropped: raw.dropped,
				p50: raw.latency_ms.p50,
				p99: raw.latency_ms.p99,
				thresholds_passed: raw.thresholds_passed,
				params: raw.params
			};
		})
		.sort((a, b) => b.date.localeCompare(a.date));
}
