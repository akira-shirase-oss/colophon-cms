<!-- A list with nothing in it yet: what will be here, and the one action that fills it (D-059, rule 4). -->
<script lang="ts">
	import type { Component, Snippet } from 'svelte';
	import PixelIcon from '#lib/proposal/PixelIcon.svelte';

	let {
		icon: Icon,
		title,
		children,
		action,
		testid
	}: { icon: Component; title: string; children?: Snippet; action?: Snippet; testid?: string } = $props();
</script>

<!-- Studio v2 proposal: the website's pixel icon (it wiggles on hover or tap) on the section ground, and the one
     line in Geist Pixel. `icon` stays for callers; the pixel icon replaces it on this branch. -->
<div class="flex flex-col items-center rounded-card border border-border/70 bg-[var(--surface-section)] px-6 py-12 text-center" data-testid={testid} data-icon={Icon.name}>
	<PixelIcon name="empty" size="2.5rem" wiggle class="mb-4" />
	<!-- The title in Geist Pixel, the product's own face (D-210); the explanation under it stays Inter. -->
	<p class="font-title text-2xl">{title}</p>
	{#if children}<div class="mt-1 max-w-md text-sm text-pretty text-muted-foreground">{@render children()}</div>{/if}
	{#if action}<div class="mt-5 flex flex-wrap justify-center gap-2">{@render action()}</div>{/if}
</div>
