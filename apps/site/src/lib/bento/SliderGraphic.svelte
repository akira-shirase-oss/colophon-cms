<!--
  Time travel (P4; planned): read the site as it was at any release; rolling back is a new
  release. A real range control: drag or use the arrow keys. While in view and untouched it scrubs
  slowly by itself (not under reduced motion). The pages shown are an illustration.
-->
<script lang="ts">
	import { inView, reducedMotion, type RunState } from '#lib/motion.ts';

	const LATEST = 42;
	const FIRST = 38;
	const PAGES: Record<number, { soup: string; hours: string }> = {
		38: { soup: 'leek and potato', hours: '9 to 5' },
		39: { soup: 'leek and potato', hours: '8 to 6' },
		40: { soup: 'pea and mint', hours: '8 to 6' },
		41: { soup: 'pea and mint', hours: '8 to 6' },
		42: { soup: 'pea and mint', hours: '8 to 9' }
	};
	const PATH = [42, 41, 40, 39, 38, 39, 40];

	let release = $state(40);
	let touched = false;
	let timer: ReturnType<typeof setInterval> | undefined;
	let step = 0;

	function run(state: RunState) {
		clearInterval(timer);
		if (state !== 'on' || touched || reducedMotion()) return;
		timer = setInterval(() => {
			step = (step + 1) % PATH.length;
			release = PATH[step] ?? LATEST;
		}, 1400);
	}
	$effect(() => () => clearInterval(timer));
	const page = $derived(PAGES[release] ?? PAGES[LATEST]!);
	const id = $props.id();
</script>

<div class="flex h-full flex-col justify-center gap-3 p-5" {@attach inView(run)}>
	<div class="flex items-center justify-between text-xs">
		<label for="{id}-release" class="font-medium">Viewing release <span class="figure">{release}</span></label>
		<span class="text-muted-foreground">latest: {LATEST}</span>
	</div>
	<input
		id="{id}-release"
		type="range"
		min={FIRST}
		max={LATEST}
		step="1"
		bind:value={release}
		style="--value: {release}"
		oninput={() => {
			touched = true;
			clearInterval(timer);
		}}
		class="range w-full"
		aria-describedby="{id}-note"
	/>
	<div class="figure flex justify-between px-0.5 font-mono text-[0.625rem] text-muted-foreground" aria-hidden="true">
		{#each Object.keys(PAGES) as key (key)}<span>{key}</span>{/each}
	</div>
	<div class="rounded-xl border border-border bg-card px-3 py-2 font-mono text-[0.75rem] leading-relaxed shadow-hairline" aria-hidden="true">
		{#key release}
			<p class="swap"><span class="text-muted-foreground">GET</span> /content/v1/…/articles?generation={release}</p>
			<p class="swap">"soup": "{page.soup}", "hours": "{page.hours}"</p>
		{/key}
	</div>
	<p id="{id}-note" class="sr-only">A preview of a planned feature: the page as it was at the chosen release.</p>
</div>

<style>
	.swap {
		animation: swap 260ms ease both;
	}
	@keyframes swap {
		from {
			opacity: 0.2;
		}
	}
	/* The range in the design system's colours: a quiet track, the part before the thumb in the accent. */
	.range {
		appearance: none;
		height: 1.5rem;
		background: transparent;
		cursor: pointer;
		border-radius: 999px;
		--fill: calc((var(--value) - 38) / 4 * 100%);
	}
	.range:focus-visible {
		outline: none;
		box-shadow: 0 0 0 4px color-mix(in oklch, var(--ring) 25%, transparent);
	}
	.range::-webkit-slider-runnable-track {
		height: 0.375rem;
		border-radius: 999px;
		background: linear-gradient(to right, var(--brand) var(--fill), var(--muted) var(--fill));
	}
	.range::-moz-range-track {
		height: 0.375rem;
		border-radius: 999px;
		background: var(--muted);
	}
	.range::-moz-range-progress {
		height: 0.375rem;
		border-radius: 999px;
		background: var(--brand);
	}
	.range::-webkit-slider-thumb {
		appearance: none;
		margin-top: -0.4375rem;
		width: 1.25rem;
		height: 1.25rem;
		border-radius: 999px;
		background: var(--card);
		border: 2px solid var(--brand);
		box-shadow: var(--shadow-soft);
	}
	.range::-moz-range-thumb {
		width: 1rem;
		height: 1rem;
		border-radius: 999px;
		background: var(--card);
		border: 2px solid var(--brand);
	}
	@media (prefers-reduced-motion: reduce) {
		.swap {
			animation: none;
		}
	}
</style>
