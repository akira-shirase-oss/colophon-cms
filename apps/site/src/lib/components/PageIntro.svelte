<!--
  The top of an inner page, after the reference's blog index: a sky-to-sand band with a small
  label, the page's one h1 and a lead, centred; then, inside the band, whatever the page shows
  first (a figure, a featured post).
-->
<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		badge = '',
		title,
		children,
		aside,
		below
	}: { badge?: string; title: string; children?: Snippet; aside?: Snippet; below?: Snippet } = $props();
</script>

<header class="hero-ground pt-32 pb-16 tb:pt-[9.5rem] dt:pt-40 dt:pb-20">
	<div class="shell text-center">
		{#if badge}<p class="chip">{badge}</p>{/if}
		<h1 class="t-h1 mx-auto mt-4 max-w-[49.5rem]">{title}</h1>
		{#if children}
			<div class="t-lead mx-auto mt-4 max-w-[43.75rem] space-y-3 text-pretty">{@render children()}</div>
		{/if}
		{#if aside}<div class="mt-10 flex justify-center">{@render aside()}</div>{/if}
	</div>
	{#if below}<div class="shell mt-16">{@render below()}</div>{/if}
</header>
