<!-- One section of the design system page: a title, a line on when to use it, and the specimens. -->
<script lang="ts">
	import type { Snippet } from 'svelte';

	let { id, title, lead, children }: { id: string; title: string; lead: string; children: Snippet } = $props();
</script>

<section {id} class="scroll-mt-8 border-t border-border py-14 first:border-t-0 first:pt-0" aria-labelledby="{id}-title">
	<div class="mb-8 max-w-2xl space-y-2">
		<h2 id="{id}-title" class="display-2">{title}</h2>
		<p class="text-[0.9375rem] text-pretty text-muted-foreground">{lead}</p>
	</div>
	{@render children()}
</section>
