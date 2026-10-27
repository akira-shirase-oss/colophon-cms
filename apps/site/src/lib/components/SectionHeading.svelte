<!-- A section's eyebrow, title and lead, after the reference's: uppercase eyebrow, 20px, then the title. -->
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

<div class={align === 'center' ? 'mx-auto max-w-[50rem] text-center' : 'max-w-[42rem]'}>
	{#if eyebrow}<p class="t-eyebrow mb-4 tb:mb-5">{eyebrow}</p>{/if}
	<h2 {id} class="t-h2">{title}</h2>
	{#if children}
		<div class="t-body-lg mt-4 text-pretty">{@render children()}</div>
	{/if}
</div>
