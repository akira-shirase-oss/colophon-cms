<!-- M3 primary tabs (Bits UI Tabs): arrow keys move between tabs. Bits keeps every panel in the
     page and hides the inactive ones, so their content stays mounted. -->
<script lang="ts">
	import { Tabs } from 'bits-ui';
	import type { Snippet } from 'svelte';

	let {
		value = $bindable(),
		label,
		items
	}: { value: string; label: string; items: { value: string; label: string; panel: Snippet }[] } = $props();
</script>

<Tabs.Root bind:value class="tabs">
	<Tabs.List class="tab-list" aria-label={label}>
		{#each items as item (item.value)}
			<Tabs.Trigger value={item.value} class="tab">{item.label}</Tabs.Trigger>
		{/each}
	</Tabs.List>
	{#each items as item (item.value)}
		<Tabs.Content value={item.value} class="tab-panel">{@render item.panel()}</Tabs.Content>
	{/each}
</Tabs.Root>

<style>
	:global(.tab-list) {
		display: flex;
		border-bottom: 1px solid var(--md-sys-color-surface-variant);
	}
	:global(.tab) {
		position: relative;
		flex: 1;
		height: 3rem;
		border: 0;
		background: transparent;
		color: var(--md-sys-color-on-surface-variant);
		font: var(--md-sys-typescale-title-small);
		letter-spacing: var(--md-sys-typescale-title-small-tracking);
		cursor: pointer;
	}
	:global(.tab:hover) {
		background: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
	}
	:global(.tab[data-state='active']) {
		color: var(--md-sys-color-primary);
	}
	:global(.tab[data-state='active']::after) {
		content: '';
		position: absolute;
		left: 25%;
		right: 25%;
		bottom: 0;
		height: 3px;
		border-radius: 3px 3px 0 0;
		background: var(--md-sys-color-primary);
	}
	:global(.tab-panel) {
		padding-top: 0.75rem;
	}
</style>
