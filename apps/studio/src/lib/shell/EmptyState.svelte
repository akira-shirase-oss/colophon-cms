<script lang="ts">
	import type { Component, Snippet } from 'svelte';

	let { icon: Icon, title, children }: { icon: Component; title: string; children?: Snippet } = $props();
</script>

<div class="flex flex-col items-center rounded-xl border border-dashed border-border px-6 py-12 text-center">
	<div class="mb-4 flex size-10 items-center justify-center rounded-lg border border-border bg-card shadow-xs">
		<Icon class="size-5 text-muted-foreground" aria-hidden="true" />
	</div>
	<p class="font-medium">{title}</p>
	{#if children}<div class="mt-1 max-w-sm text-sm text-muted-foreground">{@render children()}</div>{/if}
</div>
