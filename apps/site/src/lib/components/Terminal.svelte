<!-- A terminal pane: commands and their notes, which arrive one line at a time when it scrolls into view. -->
<script lang="ts">
	import { inView } from '#lib/motion.ts';

	let { title, lines }: { title: string; lines: ({ cmd: string } | { out: string })[] } = $props();
</script>

<div class="overflow-hidden rounded-2xl border border-border bg-background font-mono text-[0.8125rem] leading-relaxed" {@attach inView()}>
	<p class="flex items-center gap-2 border-b border-border px-4 py-2.5 font-sans text-xs text-muted-foreground">
		<span class="flex gap-1.5" aria-hidden="true"><i class="size-2 rounded-full bg-border"></i><i class="size-2 rounded-full bg-border"></i><i class="size-2 rounded-full bg-border"></i></span>
		{title}
	</p>
	<!-- Long commands scroll sideways on a phone; a scrollable region must be reachable by keyboard. -->
	<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
	<div class="space-y-1 overflow-x-auto px-4 py-4 outline-none focus-visible:ring-4 focus-visible:ring-ring/25" tabindex="0" role="region" aria-label={title}>
		{#each lines as line, index (index)}
			{#if 'cmd' in line}
				<p class="ln reveal whitespace-pre" style="--i: {index}"><span class="text-muted-foreground" aria-hidden="true">$ </span>{line.cmd}</p>
			{:else}
				<p class="ln reveal whitespace-pre text-muted-foreground" style="--i: {index}">{line.out}</p>
			{/if}
		{/each}
	</div>
</div>

<style>
	:global([data-run]) .ln {
		animation: line-in 300ms ease both;
		animation-delay: calc(var(--i) * 220ms);
	}
	@keyframes line-in {
		from {
			opacity: 0;
			transform: translateY(3px);
		}
	}
</style>
