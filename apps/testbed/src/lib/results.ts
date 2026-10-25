// The committed load-test results (perf/results/history.jsonl), read at build time.

import raw from '../../../../perf/results/history.jsonl?raw';

export interface LoadResult {
	scenario: 'throughput' | 'steady';
	date: string;
	commit: string;
	dirty: boolean;
	machine: { id: string; cpu: string; cores: number; memory_gb: number; os: string };
	params: { total?: number; vus: number; rate?: number; duration?: string };
	requests: number;
	rps: number;
	failed: number;
	checks_failed: number;
	dropped: number;
	latency_ms: { avg: number; p50: number; p90: number; p95: number; p99: number; p999: number; max: number };
	database_transactions_during_run: number;
	server_warnings: number;
	thresholds_passed: boolean;
}

export const results: LoadResult[] = raw
	.split('\n')
	.filter((line) => line.trim() !== '')
	.map((line) => JSON.parse(line) as LoadResult)
	.reverse();
