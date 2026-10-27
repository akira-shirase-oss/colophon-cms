<!--
  Load results from perf/results (passed in from the server load): requests a second for each
  run, drawn to one scale, with its p99. The bars grow once when the card comes into view.
-->
<script lang="ts">
	let { rows }: { rows: { label: string; rps: number; p99: number; ok: boolean }[] } = $props();
	const number = new Intl.NumberFormat('en-US');
	const max = $derived(Math.max(...rows.map((row) => row.rps)));
</script>

<div class="flex h-full flex-col justify-center gap-2.5 p-5">
	{#each rows as row, index (row.label)}
		<div class="grid grid-cols-[7.5rem_minmax(0,1fr)_4.5rem] items-center gap-3 text-xs sm:grid-cols-[9rem_minmax(0,1fr)_5rem]">
			<span class="truncate text-muted-foreground">{row.label}</span>
			<span class="h-3 overflow-hidden rounded-full bg-muted">
				<span class="bar reveal block h-full origin-left rounded-full {row.ok ? 'bg-chart-1' : 'bg-attention'}" style="--w: {Math.max(row.rps / max, 0.015)}; --i: {index}"></span>
			</span>
			<span class="figure text-right"><span class="font-medium">{number.format(row.rps)}</span><span class="text-muted-foreground">/s</span></span>
		</div>
	{/each}
	<p class="mt-1 flex flex-wrap justify-between gap-x-3 font-mono text-[0.6875rem] text-muted-foreground">
		<span>requests a second · one laptop</span><span>p99 {rows.map((row) => `${row.p99.toFixed(0)}`).join(' / ')} ms</span>
	</p>
</div>

<style>
	.bar {
		transform: scaleX(var(--w));
	}
	:global([data-run]) .bar {
		animation: grow 1s cubic-bezier(0.2, 0.8, 0.2, 1) both;
		animation-delay: calc(var(--i) * 120ms);
	}
	@keyframes grow {
		from {
			transform: scaleX(0);
		}
	}
</style>
