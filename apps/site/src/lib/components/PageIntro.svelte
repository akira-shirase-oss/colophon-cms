<!-- The top of an inner page: a badge, the page's one h1, and a lead, over a faint warmth. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { Badge } from '@colophon/ui/ui/badge';

	let { badge = '', title, children, aside }: { badge?: string; title: string; children?: Snippet; aside?: Snippet } = $props();
</script>

<header class="haze border-b border-border">
	<div class="shell grid gap-8 py-14 sm:py-20 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-end">
		<div class="max-w-3xl space-y-5">
			{#if badge}<Badge variant="brand">{badge}</Badge>{/if}
			<h1 class="display-1">{title}</h1>
			{#if children}
				<div class="max-w-2xl space-y-3 text-[1.0625rem] leading-relaxed text-pretty text-muted-foreground">{@render children()}</div>
			{/if}
		</div>
		{#if aside}<div>{@render aside()}</div>{/if}
	</div>
</header>
