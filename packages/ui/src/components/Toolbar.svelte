<!-- A toolbar with one tab stop: arrow keys move between its buttons (Bits UI Toolbar). -->
<script lang="ts">
	import { Toolbar } from 'bits-ui';
	import type { Snippet } from 'svelte';

	let { label, children }: { label: string; children: Snippet } = $props();
</script>

<Toolbar.Root class="toolbar" aria-label={label} loop>
	{@render children()}
</Toolbar.Root>

<style>
	:global(.toolbar) {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.125rem;
		padding: 0.25rem;
		background: var(--md-sys-color-surface-container);
	}
</style>
