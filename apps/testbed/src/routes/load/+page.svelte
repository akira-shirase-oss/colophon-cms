<script lang="ts">
	import { results } from '#lib/results.ts';

	const number = new Intl.NumberFormat('en');

	function params(result: (typeof results)[number]): string {
		return result.scenario === 'throughput'
			? `${number.format(result.params.total ?? 0)} requests, ${result.params.vus} VUs`
			: `${number.format(result.params.rate ?? 0)}/s for ${result.params.duration}`;
	}
</script>

<h1>Load tests</h1>
<p class="lede">
	Every committed run of <code>scripts/load.sh</code>, newest first, failures included. These numbers come from the machine
	named on each row with k6 on the same machine: they are evidence about the code, not a production capacity claim.
</p>

<div class="table">
	<table data-testid="results">
		<thead>
			<tr>
				<th>When (UTC)</th>
				<th>Scenario</th>
				<th class="num">Requests</th>
				<th class="num">Per second</th>
				<th class="num">p50 ms</th>
				<th class="num">p99 ms</th>
				<th class="num">Failed</th>
				<th class="num">Not started</th>
				<th class="num">DB queries</th>
				<th>Result</th>
				<th>Machine · commit</th>
			</tr>
		</thead>
		<tbody>
			{#each results as result (result.date)}
				<tr data-scenario={result.scenario} data-passed={result.thresholds_passed}>
					<td>{result.date.replace('T', ' ').replace('Z', '')}</td>
					<td>{result.scenario}<br /><small>{params(result)}</small></td>
					<td class="num">{number.format(result.requests)}</td>
					<td class="num">{number.format(result.rps)}</td>
					<td class="num">{result.latency_ms.p50}</td>
					<td class="num">{result.latency_ms.p99}</td>
					<td class="num">{number.format(result.failed)}</td>
					<td class="num">{number.format(result.dropped)}</td>
					<td class="num">{result.database_transactions_during_run}</td>
					<td><span class="pill {result.thresholds_passed ? 'ok' : 'bad'}">{result.thresholds_passed ? 'passed' : 'failed'}</span></td>
					<td><small>{result.machine.cpu}, {result.machine.cores} cores · <code>{result.commit}</code></small></td>
				</tr>
			{/each}
		</tbody>
	</table>
</div>

<style>
	.lede {
		color: var(--muted);
		max-width: 46rem;
	}
	.table {
		overflow-x: auto;
		border: 1px solid var(--line);
		border-radius: 10px;
		background: var(--card);
	}
	table {
		border-collapse: collapse;
		width: 100%;
		font-size: 0.9rem;
	}
	th,
	td {
		padding: 0.55rem 0.7rem;
		border-bottom: 1px solid var(--line);
		text-align: left;
		vertical-align: top;
		white-space: nowrap;
	}
	th {
		color: var(--muted);
		font-weight: 600;
	}
	.num {
		text-align: right;
		font-variant-numeric: tabular-nums;
	}
	small {
		color: var(--muted);
	}
	.pill {
		border-radius: 999px;
		padding: 0.1rem 0.55rem;
		font-weight: 600;
		font-size: 0.8rem;
	}
	.ok {
		color: var(--ok);
		background: var(--ok-bg);
	}
	.bad {
		color: var(--bad);
		background: var(--bad-bg);
	}
</style>
