<!-- A labelled specimen: what it is, above the thing itself. -->
<script lang="ts">
	import type { Snippet } from 'svelte';

	let { label, class: className = '', children }: { label: string; class?: string; children: Snippet } = $props();
</script>

<div class="space-y-3 {className}">
	<p class="eyebrow">{label}</p>
	{@render children()}
</div>
