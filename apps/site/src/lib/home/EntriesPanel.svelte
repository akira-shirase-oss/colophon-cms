<!--
  The first feature row's picture: the Studio's list of articles, searched by title, each with its
  state in the Studio's own words and its revision, over a review diff of the newest change (the
  real states of step 3.5; the entries are an illustration). Decorative: FeatureBlock says it in words.
-->
<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import SearchIcon from '@lucide/svelte/icons/search';
	import { inView } from '#lib/motion.ts';

	const ROWS = [
		{ title: 'Spring menu', state: 'Edited since it went live', tone: 'pending', revision: 7 },
		{ title: 'Opening hours', state: 'Readers see this version', tone: 'live', revision: 3 },
		{ title: 'About us', state: 'Readers see this version', tone: 'live', revision: 12 },
		{ title: 'Summer terrace', state: 'Not published', tone: 'draft', revision: 2 },
		{ title: 'Catering', state: 'Not published', tone: 'draft', revision: 1 }
	] as const;
</script>

<div class="pt-8 text-left" {@attach inView()}>
	<div class="px-6">
		<p class="text-lg font-semibold">Articles</p>
		<div class="mt-4 flex gap-3">
			<span class="flex h-11 flex-1 items-center gap-2 rounded-full border border-border px-4 text-[0.875rem] text-muted-foreground">
				<SearchIcon class="size-4" />Search titles…
			</span>
			<span class="flex h-11 items-center gap-1.5 rounded-full border border-border px-4 text-[0.875rem]">All states<ChevronDownIcon class="size-4" /></span>
		</div>
	</div>
	<div class="mt-5 grid grid-cols-[minmax(0,1.5fr)_minmax(0,1.6fr)_4.5rem] border-y border-border bg-background px-6 py-3 text-[0.75rem] text-muted-foreground">
		<span>Title</span><span>State</span><span class="text-right">Revision</span>
	</div>
	<ul class="divide-y divide-border">
		{#each ROWS as row, index (row.title)}
			<li class="entry grid grid-cols-[minmax(0,1.5fr)_minmax(0,1.6fr)_4.5rem] items-center px-6 py-3 text-[0.875rem]" style="--i: {index}">
				<span class="truncate font-medium">{row.title}</span>
				<span><span class="state inline-flex max-w-full items-center gap-1.5 truncate rounded-full px-2 py-0.5 text-[0.75rem] font-medium" data-tone={row.tone}><i></i>{row.state}</span></span>
				<span class="figure text-right text-muted-foreground">{row.revision}</span>
			</li>
		{/each}
	</ul>
	<div class="mx-6 mt-5 overflow-hidden rounded-xl border border-border">
		<p class="border-b border-border bg-background px-3 py-2 text-[0.75rem] text-muted-foreground">Spring menu · compare revision 6 → 7</p>
		<div class="font-mono text-[0.75rem] leading-6">
			<p class="bg-attention-soft px-3 text-attention-ink">− "soup": "leek and potato"</p>
			<p class="bg-live-soft px-3 text-live-ink">+ "soup": "pea and mint"</p>
		</div>
	</div>
</div>

<style>
	.state i {
		width: 0.375rem;
		height: 0.375rem;
		border-radius: 999px;
		flex-shrink: 0;
	}
	.state[data-tone='live'] {
		background: var(--live-soft);
		color: var(--live-ink);
	}
	.state[data-tone='live'] i {
		background: var(--live);
	}
	.state[data-tone='pending'] {
		background: var(--pending-soft);
		color: var(--pending-ink);
	}
	.state[data-tone='pending'] i {
		background: var(--pending);
	}
	.state[data-tone='draft'] {
		background: var(--draft-soft);
		color: var(--draft-ink);
	}
	.state[data-tone='draft'] i {
		background: var(--draft);
	}
	/* The search is typed: rows settle in one after another, once. */
	:global([data-run]) .entry {
		animation: entry-in 500ms cubic-bezier(0.22, 1, 0.36, 1) both;
		animation-delay: calc(var(--i) * 80ms + 200ms);
	}
	:global([data-run='armed']) .entry {
		animation-play-state: paused;
	}
	@keyframes entry-in {
		from {
			opacity: 0;
			transform: translateY(6px);
		}
	}
</style>
