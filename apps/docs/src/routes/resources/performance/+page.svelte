<!--
  Performance: how load tests run, and every result kept in perf/results/, read from the files when
  the site is built. The only performance figures on this site are these, each with its machine,
  commit and what was measured (module 16: no uncited speed claims).
-->
<script lang="ts">
	import PageFrame from '#lib/components/PageFrame.svelte';
	import { blobUrl, pageAt } from '#lib/nav.ts';
	import { href } from '#lib/paths.ts';
	import type { LoadResult } from '#lib/server/perf.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const entry = pageAt('/resources/performance/');
	const number = new Intl.NumberFormat('en-GB');
	const ms = (value: number) => `${value < 10 ? value.toFixed(2) : value < 100 ? value.toFixed(1) : number.format(Math.round(value))} ms`;

	const latest = $derived.by(() => {
		const seen = new Map<string, LoadResult>();
		for (const result of data.results) {
			const key = `${result.endpoint} ${result.scenario}`;
			if (!seen.has(key)) seen.set(key, result);
		}
		return [...seen.values()].sort((a, b) => a.endpoint.localeCompare(b.endpoint) || a.scenario.localeCompare(b.scenario));
	});
	const machines = $derived([...new Map(data.results.map((result) => [result.machine.id, result.machine])).values()]);
	const missed = $derived(data.results.filter((result) => !result.thresholds_passed).length);

	const SCENARIOS: [string, string][] = [
		['health', 'the health checks, no database'],
		['rich-text', 'rich-text validation, no database'],
		['session', 'signed-in requests: a session lookup and row-level security on each'],
		['entries', 'drafts: reads, lists and If-Match saves, each read checked against the last save'],
		['content-types', 'the same for a seven-field type, plus reads of its definition'],
		['cli', 'requests authenticated by a bearer token, as the CLI sends them'],
		['members', 'member lists, invitation lookups, role changes and new invitations'],
		['projects', 'refused project creations and /me'],
		['changesets', 'reading, listing, reviewing and creating changesets'],
		['delivery', 'anonymous /content/v1 reads, latest and pinned'],
		['publishing', 'whole publishes, five requests each, with the worker announcing every release to a local receiver'],
		['studio', "the Studio's static files"],
		['site', "the reference site's pages, while a release is published every two seconds"]
	];

	const toc = [
		{ depth: 2 as const, id: 'running-a-load-test', text: 'Running a load test' },
		{ depth: 2 as const, id: 'what-each-scenario-measures', text: 'What each scenario measures' },
		{ depth: 2 as const, id: 'latest-results', text: 'Latest results' },
		{ depth: 2 as const, id: 'every-kept-result', text: 'Every kept result' }
	];
</script>

{#snippet table(rows: LoadResult[], caption: string)}
	<div class="table-scroll">
		<table>
			<caption class="sr-only">{caption}</caption>
			<thead>
				<tr>
					<th scope="col">Scenario</th>
					<th scope="col">Requests</th>
					<th scope="col">Rate</th>
					<th scope="col">p50</th>
					<th scope="col">p99</th>
					<th scope="col">Failed</th>
					<th scope="col">k6 thresholds</th>
					<th scope="col">Run</th>
				</tr>
			</thead>
			<tbody>
				{#each rows as row (row.file)}
					<tr>
						<td class="whitespace-nowrap"><code>{row.endpoint}</code> {row.scenario}</td>
						<td class="tabular whitespace-nowrap">{number.format(row.requests)}</td>
						<td class="tabular whitespace-nowrap">{number.format(row.rps)}/s</td>
						<td class="tabular whitespace-nowrap">{ms(row.p50)}</td>
						<td class="tabular whitespace-nowrap">{ms(row.p99)}</td>
						<td class="tabular whitespace-nowrap">{number.format(row.failed)}{row.checks_failed > 0 ? ` (${number.format(row.checks_failed)} checks)` : ''}</td>
						<td>{row.thresholds_passed ? 'passed' : 'missed'}</td>
						<td class="whitespace-nowrap">
							<a href={blobUrl(row.file)} rel="external">{row.date.slice(0, 16).replace('T', ' ')}</a>, <code>{row.commit}</code>
						</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
{/snippet}

<PageFrame title={entry?.title ?? 'Performance'} description={entry?.description} capability={entry?.capability ?? { kind: 'done' }} source={entry?.source ?? ''} {toc}>
	<blockquote>
		<p>
			<strong>Laptop numbers are evidence about the code on that machine, not production capacity.</strong> Every figure on this page comes from a file in
			<code>perf/results/</code>, with the machine it ran on and the commit it measured. The load generator (k6) ran on the same machine as the server and the
			database, competing with them for the same cores.
		</p>
	</blockquote>

	<p>
		Every feature that serves requests ships with a k6 scenario in <code>perf/k6/</code> (D-018). A full run sends at least a million requests against a release build
		and the local stack. Its summary is kept whatever happened, misses included, and a run that misses a threshold or regresses against the machine's baseline fails.
		CI runs short smoke versions with absolute thresholds, because shared runners are noisy.
	</p>

	<h2 id="running-a-load-test">Running a load test</h2>
	<p>It needs the <a href={href('/operations/local-stack/')}>local stack</a>, the pinned Rust toolchain, Docker, <code>jq</code>, <code>curl</code> and k6:</p>
	{@html data.install}
	{@html data.usage}
	<p>
		<code>throughput</code> sends 1,000,000 requests back to back; <code>steady</code> holds a fixed rate for 60 seconds. <code>--baseline</code> also saves the run as the
		machine's baseline, which later runs are compared with: a throughput run more than 25% slower, or any run whose p99 is more than 1.5 times the baseline's plus 1 ms,
		fails. Overrides: <code>TOTAL</code>, <code>VUS</code>, <code>RATE</code>, <code>DURATION</code>, <code>P99_MS</code>, and <code>KEEP=0</code> to keep no result file.
		The script's header in <a href={blobUrl('scripts/load.sh')} rel="external">scripts/load.sh</a> explains each scenario's default rate and threshold.
	</p>

	<h2 id="what-each-scenario-measures">What each scenario measures</h2>
	<ul>
		{#each SCENARIOS as [name, what] (name)}
			<li><code>{name}</code>: {what}.</li>
		{/each}
	</ul>

	<h2 id="latest-results">Latest results</h2>
	<p>
		The newest kept run of each scenario. "k6 thresholds" is whether the run met the absolute thresholds its scenario sets in <code>perf/k6/</code> (typically failed requests, checks and a p99
		limit, plus dropped iterations for steady runs); the comparison with the baseline is printed by the script and recorded in the commit that kept the file, not in the file.
	</p>
	{#each machines as machine (machine.id)}
		<p>
			Machine <code>{machine.id}</code>: {machine.cpu}, {machine.cores} cores, {machine.memory_gb} GB, {machine.os}.
		</p>
	{/each}
	{@render table(latest, 'The newest kept load run of each scenario')}

	<h2 id="every-kept-result">Every kept result</h2>
	<p>
		All {data.results.length} result files in <code>perf/results</code>, newest first, including the {missed} runs that missed their thresholds. Some runs were repeated after a
		fix; both are kept, and the commit messages say why.
	</p>
	{@render table(data.results, 'Every kept load run, newest first')}
</PageFrame>
