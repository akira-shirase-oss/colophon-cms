<!--
  Every committed load result (perf/results/*.json), oldest first: one bar per run, its height the
  requests a second on a log scale, red when it missed its own thresholds. Pointing at a bar shows
  that run. The chart is a picture (aria-hidden); the sentence under it says what it shows.
-->
<script lang="ts">
	import { inView } from '#lib/motion.ts';

	interface Run {
		file: string;
		endpoint: string;
		scenario: string;
		date: string;
		rps: number;
		p99: number;
		ok: boolean;
	}
	let { runs }: { runs: Run[] } = $props();
	const number = new Intl.NumberFormat('en-US');
	const top = $derived(Math.log10(Math.max(...runs.map((run) => run.rps))));
	const floor = 2; // 100 requests a second at the bottom
	let shown = $state<Run | null>(null);
	const latest = $derived(runs[runs.length - 1] ?? null);
	const view = $derived(shown ?? latest);
	const missed = $derived(runs.filter((run) => !run.ok).length);
</script>

<figure class="rounded-3xl border border-border/70 bg-card p-5 shadow-soft sm:p-6" {@attach inView()}>
	<figcaption class="flex flex-wrap items-baseline justify-between gap-2">
		<span class="flex flex-col">
			<span class="text-[0.9375rem] font-semibold">Every load run, kept</span>
			<span class="font-mono text-xs text-muted-foreground">bar height: requests a second, log scale</span>
		</span>
		<span class="flex items-center gap-3 font-mono text-xs text-muted-foreground">
			<span class="inline-flex items-center gap-1.5"><i class="size-2 rounded-sm bg-chart-2"></i>{runs.length - missed} passed</span>
			<span class="inline-flex items-center gap-1.5"><i class="size-2 rounded-sm bg-attention"></i>{missed} missed their thresholds</span>
		</span>
	</figcaption>
	<!-- Pointing at a bar shows its run below; the same facts are in the summary for assistive technology. -->
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div
		class="mt-5 flex h-36 items-end gap-[3px]"
		aria-hidden="true"
		onpointerover={(event) => {
			const index = (event.target as HTMLElement).dataset['index'];
			if (index !== undefined) shown = runs[Number(index)] ?? null;
		}}
		onpointerleave={() => (shown = null)}
	>
		{#each runs as run, index (run.file)}
			<span
				class="bar relative h-full flex-1"
				style="--h: {Math.max((Math.log10(run.rps) - floor) / (top - floor), 0.04)}; --i: {index}"
				data-index={index}
				data-ok={run.ok}
				data-shown={shown === run}
			></span>
		{/each}
	</div>
	<p class="mt-4 flex min-h-10 flex-wrap items-baseline gap-x-3 gap-y-1 font-mono text-xs" aria-hidden="true">
		{#if view}
			<span class="font-semibold">{view.endpoint} · {view.scenario}</span>
			<span>{number.format(view.rps)} req/s</span>
			<span>p99 {view.p99.toFixed(1)} ms</span>
			<span class={view.ok ? 'text-live-ink' : 'text-attention-ink'}>{view.ok ? 'thresholds passed' : 'missed its thresholds'}</span>
			<span class="text-muted-foreground">{view.date.slice(0, 10)}</span>
		{/if}
	</p>
	<p class="sr-only">
		{runs.length} load runs from 25 to 27 October 2026, between {number.format(Math.min(...runs.map((run) => run.rps)))} and
		{number.format(Math.max(...runs.map((run) => run.rps)))} requests a second; {missed} missed their own thresholds and are kept.
	</p>
</figure>

<style>
	.bar::before {
		content: '';
		position: absolute;
		inset: 0;
		border-radius: 3px 3px 1px 1px;
		background: var(--chart-2);
		transform-origin: bottom;
		transform: scaleY(var(--h));
		opacity: 0.85;
		transition: opacity 150ms ease;
	}
	.bar[data-ok='false']::before {
		background: var(--attention);
	}
	.bar[data-shown='true']::before {
		opacity: 1;
		background: var(--brand);
	}
	.bar:hover::before {
		opacity: 1;
	}
	:global([data-run]) .bar::before {
		animation: rise 700ms cubic-bezier(0.2, 0.8, 0.2, 1) both;
		animation-delay: calc(var(--i) * 18ms);
	}
	:global([data-run='armed']) .bar::before {
		animation-play-state: paused;
	}
	@keyframes rise {
		from {
			transform: scaleY(0);
		}
	}
</style>
