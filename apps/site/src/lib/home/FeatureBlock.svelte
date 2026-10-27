<!--
  One of the reference's alternating feature rows: a 588px picture beside a 420px column with an
  eyebrow, a title, a lead, a pill, and four outlined tags at its foot. In the picture, a panel sits
  40px in from a sky-to-sand ground and rises 60px into place as the row scrolls through the
  screen (transform only; at rest without JavaScript or with reduced motion).
-->
<script lang="ts">
	import type { Component, Snippet } from 'svelte';
	import Pill from '#lib/components/Pill.svelte';
	import { reveal, scrollLinked } from '#lib/motion.ts';

	let {
		id,
		eyebrow,
		title,
		action,
		tags,
		flip = false,
		lead,
		picture,
		alt
	}: {
		id: string;
		eyebrow: string;
		title: string;
		action: { label: string; href: string };
		tags: { label: string; icon: Component<{ class?: string }> }[];
		/** The picture on the right (the reference's second row). */
		flip?: boolean;
		lead: Snippet;
		picture: Snippet;
		/** What the picture shows, for people who can't see it. */
		alt: string;
	} = $props();

	let rise = $state<number | null>(null);
	const follow = scrollLinked((progress) => (rise = 1 - progress));
</script>

<div class="row flex flex-col gap-10 dt:gap-16 {flip ? 'dt:flex-row-reverse' : 'dt:flex-row'}" {@attach reveal}>
	<figure class="picture relative order-2 overflow-hidden rounded-3xl dt:order-none dt:w-[36.75rem] dt:shrink-0" {@attach follow}>
		<div class="panel absolute inset-x-5 top-10 bottom-0 overflow-hidden rounded-[20px] bg-card tb:inset-x-10 tb:bottom-10" style={rise === null ? undefined : `--rise: ${rise.toFixed(4)}`} aria-hidden="true">
			{@render picture()}
		</div>
		<figcaption class="sr-only">Illustration: {alt}</figcaption>
	</figure>

	<div class="flex flex-col justify-between gap-10 dt:w-[26.25rem]">
		<div class="flex flex-col items-start gap-10">
			<div class="flex flex-col gap-4">
				<p class="t-eyebrow">{eyebrow}</p>
				<h2 {id} class="t-h2">{title}</h2>
				<div class="t-body-lg text-pretty [&_strong]:font-semibold [&_strong]:text-foreground">{@render lead()}</div>
			</div>
			<Pill label={action.label} href={action.href} class="max-tb:w-full" />
		</div>
		<ul class="grid grid-cols-2 gap-4" aria-label="What it covers">
			{#each tags as tag (tag.label)}
				{@const Icon = tag.icon}
				<li class="tag"><Icon class="size-6 shrink-0" />{tag.label}</li>
			{/each}
		</ul>
	</div>
</div>

<style>
	.picture {
		height: 436px;
		background: var(--visual-ground);
	}
	@media (min-width: 50.625rem) {
		.picture {
			height: 695px;
		}
	}
	.panel {
		transform: translateY(calc(var(--rise, 0) * 60px));
	}
</style>
