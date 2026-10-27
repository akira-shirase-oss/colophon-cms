<!-- One load result, with what it measured, where, when and on which commit, and a link to its file. -->
<script lang="ts">
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import { Badge } from '@colophon/ui/ui/badge';
	import { GITHUB_COMMIT } from '#lib/site.ts';
	import type { Figure } from '#lib/server/perf.ts';

	let { figure, emphasis = false }: { figure: Figure; emphasis?: boolean } = $props();
	const number = new Intl.NumberFormat('en-US');
	const result = $derived(figure.result);
	const p99 = $derived(result.latency_ms.p99.toFixed(1));
</script>

<article class="flex h-full flex-col rounded-3xl border border-border/70 bg-card p-6 shadow-soft">
	<div class="flex items-start justify-between gap-3">
		<h3 class="text-[0.9375rem] font-semibold">{figure.title}</h3>
		{#if !result.thresholds_passed}
			<Badge variant="attention" title="The run missed its own k6 thresholds; kept as evidence">Missed its thresholds</Badge>
		{:else if figure.regression === 'missed'}
			<Badge variant="attention" title="Its thresholds passed, but it was slower than the machine's baseline allows; kept as evidence">Over the regression gate</Badge>
		{:else}
			<Badge variant="live" title={figure.regression === 'passed' ? 'Its thresholds passed, with no regression against the baseline' : 'Its thresholds passed; there was no baseline to compare with'}>Passed</Badge>
		{/if}
	</div>
	<p class="mt-4 flex flex-wrap items-baseline gap-x-2">
		<span class="figure {emphasis ? 'text-5xl' : 'text-4xl'}">{number.format(result.rps)}</span>
		<span class="text-sm text-muted-foreground">requests a second</span>
	</p>
	<dl class="mt-4 grid grid-cols-3 gap-3 border-t border-border pt-4 text-sm">
		<div>
			<dt class="eyebrow">Requests</dt>
			<dd class="figure mt-1">{number.format(result.requests)}</dd>
		</div>
		<div>
			<dt class="eyebrow">p99</dt>
			<dd class="figure mt-1">{p99} ms</dd>
		</div>
		<div>
			<dt class="eyebrow">Failed</dt>
			<dd class="figure mt-1">{number.format(result.failed)}</dd>
		</div>
	</dl>
	<p class="mt-4 text-sm leading-relaxed text-muted-foreground">{figure.measured}</p>
	<p class="mt-auto pt-4 text-xs text-muted-foreground">
		{result.date.slice(0, 10)} · commit <a class="font-mono underline underline-offset-2 hover:text-foreground" href={GITHUB_COMMIT(result.commit)}>{result.commit}</a> ·
		<span class="font-mono">{result.machine.id}</span> ·
		<a class="inline-flex items-center gap-0.5 underline underline-offset-2 hover:text-foreground" href={figure.url}>raw result<ArrowUpRightIcon class="size-3" aria-hidden="true" /></a>
	</p>
</article>
