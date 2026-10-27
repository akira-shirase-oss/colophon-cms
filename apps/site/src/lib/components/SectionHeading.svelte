<!-- A section's eyebrow, title and lead. -->
<script lang="ts">
	import type { Snippet } from 'svelte';

	let {
		id,
		eyebrow = '',
		title,
		align = 'left',
		children
	}: { id: string; eyebrow?: string; title: string; align?: 'left' | 'center'; children?: Snippet } = $props();
</script>

<div class="max-w-2xl space-y-3 {align === 'center' ? 'mx-auto text-center' : ''}">
	{#if eyebrow}<p class="eyebrow text-brand-ink">{eyebrow}</p>{/if}
	<h2 {id} class="display-2">{title}</h2>
	{#if children}
		<div class="text-[1.0625rem] leading-relaxed text-pretty text-muted-foreground">{@render children()}</div>
	{/if}
</div>
