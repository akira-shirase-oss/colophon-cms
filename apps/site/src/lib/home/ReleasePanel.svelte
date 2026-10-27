<!--
  The second feature row's picture: one release in the Studio's words. Four facts, the receipt's
  four steps, and the probe from the bento finding the release's marker on the live page (D-051;
  the numbers are an illustration). Decorative: FeatureBlock says it in words.
-->
<script lang="ts">
	import BadgeCheckIcon from '@lucide/svelte/icons/badge-check';
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import HashIcon from '@lucide/svelte/icons/hash';
	import MapPinIcon from '@lucide/svelte/icons/map-pin';
	import PackageCheckIcon from '@lucide/svelte/icons/package-check';
	import ProbeGraphic from '#lib/bento/ProbeGraphic.svelte';
	import { inView } from '#lib/motion.ts';

	const FACTS = [
		{ icon: PackageCheckIcon, value: '42', label: 'Release, production' },
		{ icon: FileTextIcon, value: '3', label: 'Entries in it' },
		{ icon: MapPinIcon, value: '1', label: 'Location verified' },
		{ icon: HashIcon, value: '9f2c…e41a', label: 'Page hash, kept' }
	];
</script>

<div class="pt-8 text-left">
	<div class="px-6">
		<p class="text-lg font-semibold">Release 42</p>
		<div class="mt-5 grid grid-cols-2 gap-x-4 gap-y-5 border-y border-border py-5">
			{#each FACTS as fact (fact.label)}
				{@const Icon = fact.icon}
				<div class="flex items-center gap-3">
					<span class="grid size-11 shrink-0 place-items-center rounded-full bg-muted"><Icon class="size-5" /></span>
					<div class="min-w-0">
						<p class="figure truncate text-[1.375rem] leading-tight font-semibold tracking-[-0.03em]">{fact.value}</p>
						<p class="text-[0.8125rem] text-muted-foreground">{fact.label}</p>
					</div>
				</div>
			{/each}
		</div>
		<p class="mt-5 text-base font-semibold">Receipt</p>
		<ol class="mt-3 flex flex-wrap gap-2 text-[0.75rem] font-medium">
			<li class="rounded-full bg-draft-soft px-2.5 py-1 text-draft-ink">Saved</li>
			<li class="rounded-full bg-draft-soft px-2.5 py-1 text-draft-ink">Published in CMS</li>
			<li class="rounded-full bg-pending-soft px-2.5 py-1 text-pending-ink">Website update pending</li>
			<li class="inline-flex items-center gap-1 rounded-full bg-live-soft px-2.5 py-1 text-live-ink"><BadgeCheckIcon class="size-3" />Verified at 1 location</li>
		</ol>
	</div>
	<div class="stage-dots mt-5 h-52 border-t border-border" {@attach inView()}><ProbeGraphic /></div>
</div>
