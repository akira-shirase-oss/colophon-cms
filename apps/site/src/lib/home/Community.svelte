<!--
  After the reference's "stay in the loop": where the project lives and talks. No follower counts
  (W3): each card names its address instead.
-->
<script lang="ts">
	import BookOpenIcon from '@lucide/svelte/icons/book-open';
	import BrandIcon from '#lib/components/BrandIcon.svelte';
	import Pill from '#lib/components/Pill.svelte';
	import { reveal } from '#lib/motion.ts';
	import { BUILD_LOG, GITHUB, X_ACCOUNT } from '#lib/site.ts';

	const PLACES = [
		{
			id: 'github',
			title: 'GitHub',
			where: 'colophon-cms',
			text: 'The code, the plan, every decision and every load result, failures included.',
			action: 'Read the code',
			href: GITHUB
		},
		{
			id: 'log',
			title: 'Build log',
			where: '404human.dev/colophon',
			text: 'Every step written up as it happened, on the maintainer’s own site.',
			action: 'Read the build log',
			href: BUILD_LOG
		},
		{
			id: 'x',
			title: 'X',
			where: '@akirashiraseoss',
			text: 'Short updates from the project’s account as steps land.',
			action: 'Follow on X',
			href: X_ACCOUNT
		}
	] as const;
</script>

<section aria-labelledby="loop" class="section shell" {@attach reveal}>
	<div class="mx-auto max-w-[50rem] text-center">
		<p class="t-eyebrow mb-5">Community</p>
		<h2 id="loop" class="t-h2">Follow along</h2>
	</div>
	<ul class="mt-14 grid gap-6 dt:grid-cols-3">
		{#each PLACES as place (place.id)}
			<li class="glass flex flex-col gap-8 rounded-3xl p-8">
				<div class="flex items-start justify-between gap-4">
					<span class="grid size-16 place-items-center rounded-2xl {place.id === 'x' ? 'bg-[#000] text-white' : 'bg-card text-foreground shadow-[var(--shadow-icon)]'}">
						{#if place.id === 'log'}<BookOpenIcon class="size-7" aria-hidden="true" />{:else}<BrandIcon name={place.id} size={28} />{/if}
					</span>
					<span class="t-eyebrow pt-2 text-right text-muted-foreground normal-case">{place.where}</span>
				</div>
				<div>
					<h3 class="t-h5">{place.title}</h3>
					<p class="t-body-lg mt-2">{place.text}</p>
				</div>
				<div class="mt-auto"><Pill label={place.action} href={place.href} variant="outline" /></div>
			</li>
		{/each}
	</ul>
</section>
