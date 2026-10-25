<!-- An M3 common button: filled, tonal, outlined or text. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLButtonAttributes } from 'svelte/elements';

	let {
		variant = 'filled',
		children,
		...rest
	}: { variant?: 'filled' | 'tonal' | 'outlined' | 'text'; children: Snippet } & HTMLButtonAttributes = $props();
</script>

<button class="button {variant}" {...rest}>{@render children()}</button>

<style>
	.button {
		position: relative;
		isolation: isolate;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: 0.5rem;
		height: 2.5rem;
		padding: 0 1.5rem;
		border: 1px solid transparent;
		border-radius: var(--md-sys-shape-corner-full);
		font: var(--md-sys-typescale-label-large);
		letter-spacing: var(--md-sys-typescale-label-large-tracking);
		cursor: pointer;
		overflow: hidden;
		transition: box-shadow var(--md-sys-motion-duration-short4) var(--md-sys-motion-easing-standard);
	}
	/* The state layer: the element's own text colour at the state's opacity. */
	.button::before {
		content: '';
		position: absolute;
		inset: 0;
		z-index: -1;
		background: currentColor;
		opacity: 0;
		transition: opacity var(--md-sys-motion-duration-short2) var(--md-sys-motion-easing-standard);
	}
	.button:hover::before {
		opacity: var(--md-sys-state-hover-opacity);
	}
	.button:focus-visible::before {
		opacity: var(--md-sys-state-focus-opacity);
	}
	.button:active::before {
		opacity: var(--md-sys-state-pressed-opacity);
	}
	.filled {
		background: var(--md-sys-color-primary);
		color: var(--md-sys-color-on-primary);
	}
	.filled:hover {
		box-shadow: var(--md-sys-elevation-level1);
	}
	.tonal {
		background: var(--md-sys-color-secondary-container);
		color: var(--md-sys-color-on-secondary-container);
	}
	.outlined {
		background: transparent;
		border-color: var(--md-sys-color-outline);
		color: var(--md-sys-color-primary);
	}
	.text {
		background: transparent;
		color: var(--md-sys-color-primary);
		padding: 0 0.75rem;
	}
	.button:disabled {
		cursor: not-allowed;
		background: color-mix(in srgb, var(--md-sys-color-on-surface) 12%, transparent);
		color: color-mix(in srgb, var(--md-sys-color-on-surface) 38%, transparent);
		border-color: transparent;
		box-shadow: none;
	}
	.button:disabled::before {
		opacity: 0;
	}
</style>
