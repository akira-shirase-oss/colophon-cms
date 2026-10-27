<!--
  One bento cell: a live graphic on a quiet stage, then the status, a title and one sentence. The
  graphic is decorative (aria-hidden) and `alt` says what it shows. Its animations run only while
  the card is in view (inView), and a spotlight and a glowing edge follow the pointer (pointerVars).
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import StatusBadge from '#lib/components/StatusBadge.svelte';
	import { inView, pointerVars } from '#lib/motion.ts';
	import type { StatusKind } from '#lib/status.ts';

	let {
		kind,
		at = '',
		title,
		alt,
		href = '',
		class: className = '',
		stage = 'h-52',
		interactive = false,
		graphic,
		children
	}: {
		kind: StatusKind;
		at?: string;
		title: string;
		/** What the graphic shows, for people who can't see it. */
		alt: string;
		href?: string;
		class?: string;
		/** The stage's height classes. */
		stage?: string;
		/** The graphic has its own controls, so it isn't hidden from assistive technology. */
		interactive?: boolean;
		graphic: Snippet;
		children: Snippet;
	} = $props();
</script>

<article class="spot flex flex-col overflow-hidden rounded-3xl border border-border/70 bg-card shadow-soft {className}" data-testid="bento-card" {@attach pointerVars} {@attach inView()}>
	<div class="stage-dots relative overflow-hidden border-b border-border/70 {stage}" aria-hidden={interactive ? undefined : 'true'}>
		{@render graphic()}
	</div>
	{#if !interactive}<p class="sr-only">Illustration: {alt}</p>{/if}
	<div class="flex flex-1 flex-col gap-2 p-6">
		<div><StatusBadge {kind} {at} /></div>
		<h3 class="title mt-1">
			{#if href}<a {href} class="outline-none after:absolute after:inset-0 after:content-[''] focus-visible:underline">{title}</a>{:else}{title}{/if}
		</h3>
		<div class="text-[0.9375rem] leading-relaxed text-pretty text-muted-foreground">{@render children()}</div>
	</div>
</article>
