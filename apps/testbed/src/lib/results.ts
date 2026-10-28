// The committed load-test results (perf/results/history.jsonl), read at build time.

import raw from '../../../../perf/results/history.jsonl?raw';

/** A throughput or steady run of `scripts/load.sh`. */
export interface LoadResult {
	scenario: 'throughput' | 'steady';
	endpoint?: string;
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

/** One step of a stress run: the steady scenario at one rate. */
export interface StressStep {
	rate: number;
	attempt: number;
	rps: number;
	p99_ms: number;
	failed: number;
	checks_failed: number;
	dropped: number;
	held: boolean;
	load_average: number | null;
	/** Since 2026-10-28: the WARN and ERROR lines the step added to the servers' logs. */
	server_logged?: { requests: Record<string, number>; other: Record<string, number> };
}

/** A stress run of `scripts/load.sh` (D-077): rates rising until a step breaks twice. */
export interface StressResult {
	scenario: 'stress';
	endpoint: string;
	date: string;
	commit: string;
	dirty: boolean;
	machine: { id: string; cpu: string; cores: number };
	topology: { servers: number; workers: number; balancer: string | null };
	p99_gate_ms: number;
	step_seconds: number;
	knee_rate: number;
	failure_mode: string;
	healthy_after: boolean;
	steps: StressStep[];
}

const all = raw
	.split('\n')
	.filter((line) => line.trim() !== '')
	.map((line) => JSON.parse(line) as LoadResult | StressResult)
	.reverse();

/** Throughput and steady runs, newest first. */
export const results: LoadResult[] = all.filter((result): result is LoadResult => result.scenario !== 'stress');

/** Stress runs, newest first. */
export const stressResults: StressResult[] = all.filter(
	(result): result is StressResult => result.scenario === 'stress'
);
