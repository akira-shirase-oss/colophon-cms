<!--
  One bento cell, after the reference's: a sand card with 24px corners and 32px of padding. A wide
  cell has a title, a live graphic and a sentence; a narrow cell has an icon in a white circle, a
  title and a sentence. Every cell carries its status. A graphic is decorative (aria-hidden) and
  `alt` says what it shows; its animations run only while the card is in view (inView).
-->
<script lang="ts">
	import type { Component, Snippet } from 'svelte';
	import StatusBadge from '#lib/components/StatusBadge.svelte';
	import { inView } from '#lib/motion.ts';
	import type { StatusKind } from '#lib/status.ts';

	let {
		kind,
		at = '',
		title,
		alt = '',
		class: className = '',
		stage = 'h-52 tb:h-[253px] dt:h-60',
		icon,
		graphic,
		children
	}: {
		kind: StatusKind;
		at?: string;
		title: string;
		/** What the graphic shows, for people who can't see it. */
		alt?: string;
		class?: string;
		/** The graphic's height classes. */
		stage?: string;
		/** A narrow cell's icon, instead of a graphic. */
		icon?: Component<{ class?: string }>;
		graphic?: Snippet;
		children: Snippet;
	} = $props();
</script>

<article class="sand flex flex-col rounded-3xl p-8 {className}" data-testid="bento-card" {@attach inView()}>
	{#if graphic}
		<div class="flex flex-col-reverse items-start gap-3 tb:flex-row tb:justify-between tb:gap-4">
			<h3 class="t-h5 max-w-[28rem]">{title}</h3>
			<span class="shrink-0 tb:pt-1.5"><StatusBadge {kind} {at} /></span>
		</div>
		<div class="stage-dots bento-stage relative mt-6 -mx-4 overflow-hidden {stage} tb:mt-16" aria-hidden="true">
			{@render graphic()}
		</div>
		<p class="sr-only">Illustration: {alt}</p>
		<div class="t-body-lg mt-8 text-pretty [&_strong]:font-semibold [&_strong]:text-foreground">{@render children()}</div>
	{:else}
		{@const Icon = icon}
		<div class="flex items-start justify-between gap-4">
			<span class="grid size-14 place-items-center rounded-full bg-card shadow-[var(--shadow-icon)]">{#if Icon}<Icon class="size-6" />{/if}</span>
			<StatusBadge {kind} {at} />
		</div>
		<h3 class="t-h6 mt-8 tb:mt-[4.25rem]">{title}</h3>
		<div class="t-body mt-4 text-pretty">{@render children()}</div>
	{/if}
</article>

<style>
	.bento-stage {
		background: none;
	}
</style>
