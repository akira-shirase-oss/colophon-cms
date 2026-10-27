<!-- A release's receipt, as the Studio shows it (D-051): one label, and "verified" says where. An
     illustration built from the real states, not a screenshot of real data. -->
<script lang="ts">
	import BadgeCheckIcon from '@lucide/svelte/icons/badge-check';
	import CheckIcon from '@lucide/svelte/icons/check';
	import { Badge } from '@colophon/ui/ui/badge';

	let { class: className = '' }: { class?: string } = $props();

	const STEPS = [
		{ label: 'Saved', detail: 'Version 7 of “Spring menu”, autosaved' },
		{ label: 'Published in CMS', detail: 'Release 42, one transaction' },
		{ label: 'Website update pending', detail: 'Built from release 42, swapped in at once' },
		{ label: 'Verified at 1 location', detail: 'The probe saw release 42 on the live page' }
	];
</script>

<figure class="rounded-3xl border border-border/60 bg-card p-5 text-left shadow-float sm:p-6 {className}">
	<div class="flex items-start justify-between gap-3">
		<div class="flex items-center gap-3">
			<span class="figure grid size-10 place-items-center rounded-xl bg-brand-soft text-[0.9375rem] font-semibold text-brand-ink" aria-hidden="true">42</span>
			<div>
				<p class="text-sm font-semibold">Release 42</p>
				<p class="text-xs text-muted-foreground">production · 3 entries</p>
			</div>
		</div>
		<Badge variant="live"><BadgeCheckIcon aria-hidden="true" />Verified at 1 location</Badge>
	</div>

	<ol class="mt-6 grid gap-0">
		{#each STEPS as step, index (step.label)}
			<li class="relative grid grid-cols-[1.75rem_minmax(0,1fr)] gap-3 pb-4 last:pb-0">
				{#if index < STEPS.length - 1}
					<span class="absolute top-7 bottom-0 left-[0.8125rem] w-px bg-border" aria-hidden="true"></span>
				{/if}
				<span
					class="relative grid size-7 place-items-center rounded-full {index === STEPS.length - 1
						? 'bg-live text-white'
						: 'border border-border bg-card text-live-ink'}"
					aria-hidden="true"
				>
					<CheckIcon class="size-3.5" strokeWidth={2.5} />
				</span>
				<div class="pt-0.5">
					<p class="text-sm font-medium">{step.label}</p>
					<p class="text-xs text-muted-foreground">{step.detail}</p>
				</div>
			</li>
		{/each}
	</ol>

	<div class="mt-5 flex flex-wrap items-center justify-between gap-2 border-t border-border pt-4 text-xs text-muted-foreground">
		<span>Evidence kept: release, location, time, body hash</span>
		<code class="font-mono">sha256:9f2c…e41a</code>
	</div>
	<figcaption class="sr-only">An illustration of a release receipt in the Studio: saved, published in the CMS, website update pending, and verified at one location.</figcaption>
</figure>
