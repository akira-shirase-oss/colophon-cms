<!-- An M3 small top app bar: a leading area, the title, and trailing actions. -->
<script lang="ts">
	import type { Snippet } from 'svelte';

	let { leading, title, trailing }: { leading?: Snippet; title: Snippet; trailing?: Snippet } = $props();
</script>

<header class="top-app-bar">
	{#if leading}{@render leading()}{/if}
	<div class="title">{@render title()}</div>
	{#if trailing}<div class="trailing">{@render trailing()}</div>{/if}
</header>

<style>
	.top-app-bar {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		min-height: 4rem;
		padding: 0 1rem;
		background: var(--md-sys-color-surface-container);
		color: var(--md-sys-color-on-surface);
	}
	.title {
		font: var(--md-sys-typescale-title-large);
		display: flex;
		align-items: center;
		gap: 0.75rem;
		min-width: 0;
	}
	.trailing {
		margin-left: auto;
		display: flex;
		align-items: center;
		gap: 0.25rem;
	}
</style>
