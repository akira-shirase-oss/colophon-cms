<!-- A toolbar button. `pressed` makes it a toggle (aria-pressed), drawn with the secondary container. -->
<script lang="ts">
	import { Toolbar } from 'bits-ui';
	import type { Snippet } from 'svelte';

	let {
		pressed,
		title,
		onclick,
		children
	}: { pressed?: boolean; title?: string; onclick?: () => void; children: Snippet } = $props();
</script>

<Toolbar.Button class="toolbar-button" aria-pressed={pressed} {title} {onclick}>{@render children()}</Toolbar.Button>

<style>
	:global(.toolbar-button) {
		position: relative;
		isolation: isolate;
		min-width: 2.5rem;
		height: 2.5rem;
		padding: 0 0.625rem;
		border: 0;
		border-radius: var(--md-sys-shape-corner-full);
		background: transparent;
		color: var(--md-sys-color-on-surface-variant);
		font: var(--md-sys-typescale-label-large);
		cursor: pointer;
		overflow: hidden;
	}
	:global(.toolbar-button::before) {
		content: '';
		position: absolute;
		inset: 0;
		z-index: -1;
		background: currentColor;
		opacity: 0;
	}
	:global(.toolbar-button:hover::before) {
		opacity: var(--md-sys-state-hover-opacity);
	}
	:global(.toolbar-button:active::before) {
		opacity: var(--md-sys-state-pressed-opacity);
	}
	:global(.toolbar-button[aria-pressed='true']) {
		background: var(--md-sys-color-secondary-container);
		color: var(--md-sys-color-on-secondary-container);
	}
</style>
