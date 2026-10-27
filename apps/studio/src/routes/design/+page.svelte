<!--
  The design system (D-056), on one page: the tokens, the type, and every component in the states
  the Studio uses. Nothing here talks to the server, so it opens without signing in.
-->
<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import Actions from './Actions.svelte';
	import Data from './Data.svelte';
	import Forms from './Forms.svelte';
	import Overlays from './Overlays.svelte';
	import Section from './Section.svelte';

	const COLOURS = [
		{ group: 'Paper and ink', tokens: ['background', 'card', 'foreground', 'muted-foreground', 'border', 'input'] },
		{ group: 'Actions', tokens: ['primary', 'secondary', 'accent', 'destructive', 'ring'] },
		{ group: 'Accent', tokens: ['brand', 'brand-ink', 'brand-soft', 'glow-1', 'glow-2', 'glow-3'] },
		{ group: 'States', tokens: ['live', 'live-soft', 'pending', 'pending-soft', 'attention', 'attention-soft', 'draft', 'draft-soft'] },
		{ group: 'Charts', tokens: ['chart-1', 'chart-2', 'chart-3', 'chart-4', 'chart-5'] }
	];
	const TYPE = [
		{ name: 'Display 1', class: 'display-1', sample: 'Publish, and prove it' },
		{ name: 'Display 2', class: 'display-2', sample: 'Every release has a receipt' },
		{ name: 'Title', class: 'title', sample: 'Release 42 reached the site' },
		{ name: 'Body', class: 'text-[0.9375rem] leading-relaxed', sample: 'Saved drafts are never lost, and a publish either commits whole or not at all.' },
		{ name: 'Small', class: 'text-sm text-muted-foreground', sample: 'Verified at 1 location, 4 seconds after the release.' },
		{ name: 'Eyebrow', class: 'eyebrow', sample: 'Money in, reads out' },
		{ name: 'Figure', class: 'figure text-5xl', sample: '1,000,195' },
		{ name: 'Mono', class: 'font-mono text-sm', sample: 'sha256:9f2c…e41a' }
	];
	const RADII = ['rounded-md', 'rounded-lg', 'rounded-xl', 'rounded-2xl', 'rounded-3xl', 'rounded-full'];
	const SHADOWS = ['shadow-hairline', 'shadow-soft', 'shadow-float', 'shadow-press'];

	// A small, honest chart for the hero: reads per second through the step 3.4 site load run.
	const points = [62, 58, 60, 55, 57, 52, 54, 49, 45, 47, 40, 36, 38, 30, 26, 28, 20, 14];
	const width = 360;
	const height = 96;
	const step = width / (points.length - 1);
	const line = points.map((y, index) => `${index === 0 ? 'M' : 'L'}${(index * step).toFixed(1)},${y}`).join(' ');
	const area = `${line} L${width},${height} L0,${height} Z`;
	const NAV = ['Foundations', 'Actions', 'Forms', 'Overlays', 'Data'];
</script>

<svelte:head><title>Design system · Colophon Studio</title></svelte:head>

<div class="grid gap-10 pb-16 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.05fr)] lg:items-center">
	<div class="space-y-6">
		<Badge variant="brand">Design system · D-056</Badge>
		<h1 class="display-1 max-w-xl">Quiet paper, black ink, one warm accent.</h1>
		<p class="max-w-lg text-[1.0625rem] leading-relaxed text-pretty text-muted-foreground">
			The Studio's tokens, type and every shadcn-svelte component, restyled. White sheets float on soft shadows; actions are black pills; the
			orange only glows. Every text colour passes WCAG AA, and AAA when the system asks for more contrast.
		</p>
		<nav class="flex flex-wrap gap-2" aria-label="Sections">
			{#each NAV as name (name)}
				<Button variant="outline" size="sm" href="#{name.toLowerCase()}">{name}</Button>
			{/each}
		</nav>
	</div>

	<div class="glow relative overflow-hidden rounded-[2rem] p-6 sm:p-10" aria-label="A release receipt, as an example">
		<div class="relative mx-auto max-w-sm rounded-3xl border border-border/60 bg-card p-6 shadow-float">
			<div class="flex items-start justify-between gap-3">
				<div class="flex items-center gap-3">
					<span class="grid size-9 place-items-center rounded-xl bg-brand-soft text-sm font-semibold text-brand-ink">42</span>
					<div>
						<p class="text-sm font-semibold">Release 42</p>
						<p class="text-xs text-muted-foreground">production · gen 42</p>
					</div>
				</div>
				<Badge variant="live"><span class="size-1.5 rounded-full bg-live"></span>Live</Badge>
			</div>
			<p class="mt-6 flex items-baseline gap-1">
				<span class="text-xl text-muted-foreground">↗</span>
				<span class="figure text-5xl">19,013</span>
				<span class="text-sm text-muted-foreground">reads/s</span>
			</p>
			<p class="mt-1 text-sm"><span class="font-medium text-live-ink">1,000,195 reads</span> <span class="text-muted-foreground">under release churn</span></p>
			<div class="relative mt-5">
				<svg viewBox="0 0 {width} {height}" class="h-24 w-full" preserveAspectRatio="none" aria-hidden="true">
					<defs>
						<linearGradient id="hero-fill" x1="0" x2="0" y1="0" y2="1">
							<stop offset="0%" stop-color="var(--brand)" stop-opacity="0.22" />
							<stop offset="100%" stop-color="var(--brand)" stop-opacity="0" />
						</linearGradient>
					</defs>
					<path d={area} fill="url(#hero-fill)" />
					<path d={line} fill="none" stroke="var(--brand)" stroke-width="2" stroke-linejoin="round" vector-effect="non-scaling-stroke" />
				</svg>
				<span class="absolute top-0 left-[38%] -translate-x-1/2 rounded-md bg-primary px-2 py-1 text-[0.6875rem] font-medium text-primary-foreground shadow-press">
					p99 6.9 ms
				</span>
			</div>
			<div class="mt-4 grid grid-cols-2 border-t border-border pt-4 text-sm">
				<div>
					<p class="eyebrow">Failed</p>
					<p class="figure mt-1 text-lg">4</p>
				</div>
				<div class="border-l border-border pl-4">
					<p class="eyebrow">Checks failed</p>
					<p class="figure mt-1 text-lg">0</p>
				</div>
			</div>
		</div>
		<div class="relative mx-auto -mt-5 flex w-fit items-center gap-3 rounded-full border border-border/60 bg-card py-2 pr-4 pl-2 text-sm shadow-soft">
			<span class="grid size-7 place-items-center rounded-full bg-primary text-primary-foreground"><CheckIcon class="size-4" /></span>
			Verified at 1 location
			<span class="figure font-medium">+4 s</span>
		</div>
	</div>
</div>

<Section id="foundations" title="Foundations" lead="Tokens in OKLCH, in styles.css. Use the token, never the value: light, dark and high contrast change underneath.">
	<div class="grid gap-10">
		{#each COLOURS as { group, tokens } (group)}
			<div>
				<p class="eyebrow mb-3">{group}</p>
				<div class="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-6">
					{#each tokens as token (token)}
						<div class="overflow-hidden rounded-2xl border border-border bg-card shadow-hairline">
							<div class="h-16" style="background: var(--{token})"></div>
							<p class="px-3 py-2 font-mono text-xs">--{token}</p>
						</div>
					{/each}
				</div>
			</div>
		{/each}

		<div>
			<p class="eyebrow mb-3">Type: Inter, its display cut at large sizes</p>
			<div class="divide-y divide-border overflow-hidden rounded-3xl border border-border bg-card shadow-hairline">
				{#each TYPE as item (item.name)}
					<div class="grid gap-2 px-6 py-5 sm:grid-cols-[9rem_minmax(0,1fr)] sm:items-baseline">
						<p class="text-xs text-muted-foreground">{item.name}</p>
						<p class={item.class}>{item.sample}</p>
					</div>
				{/each}
			</div>
		</div>

		<div class="grid gap-10 md:grid-cols-2">
			<div>
				<p class="eyebrow mb-3">Radius</p>
				<div class="flex flex-wrap gap-3">
					{#each RADII as radius (radius)}
						<div class="grid size-20 place-items-center border border-border bg-card text-[0.6875rem] text-muted-foreground shadow-hairline {radius}">
							{radius.replace('rounded-', '')}
						</div>
					{/each}
				</div>
			</div>
			<div>
				<p class="eyebrow mb-3">Elevation</p>
				<div class="flex flex-wrap gap-4">
					{#each SHADOWS as shadow (shadow)}
						<div class="grid h-20 w-28 place-items-center rounded-2xl bg-card text-[0.6875rem] text-muted-foreground {shadow}">
							{shadow.replace('shadow-', '')}
						</div>
					{/each}
				</div>
			</div>
		</div>
	</div>
</Section>

<Section id="actions" title="Actions" lead="Black pills do the one thing a page is for; outlined pills do the rest. Badges say state, never decoration.">
	<Actions />
</Section>

<Section id="forms" title="Forms" lead="Boxed white fields with a hairline, a soft ring on focus, and errors said in words beside the field.">
	<Forms />
</Section>

<Section id="overlays" title="Overlays" lead="Dialogs, sheets and menus float on the same soft shadow as cards, with generous radius and quiet motion.">
	<Overlays />
</Section>

<Section id="data" title="Data and layout" lead="Tables, lists, cards and navigation: hairline rules, tabular numbers, and room to breathe.">
	<Data />
</Section>

<p class="pt-6 text-sm text-muted-foreground">
	Every component lives in <code class="font-mono">packages/ui/src/lib/components/ui</code>.
	<a class="inline-flex items-center gap-0.5 font-medium text-brand-ink underline underline-offset-4" href="https://shadcn-svelte.com/docs/components">
		shadcn-svelte docs<ArrowUpRightIcon class="size-3.5" />
	</a>
</p>
