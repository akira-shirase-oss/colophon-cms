<!-- An M3 basic dialog (Bits UI Dialog): focus is trapped inside and returns on close; Escape closes. -->
<script lang="ts">
	import { Dialog } from 'bits-ui';
	import type { Snippet } from 'svelte';

	let {
		open = $bindable(false),
		title,
		description,
		children
	}: { open: boolean; title: string; description?: string; children: Snippet } = $props();
</script>

<Dialog.Root bind:open>
	<Dialog.Portal>
		<Dialog.Overlay class="dialog-scrim" />
		<Dialog.Content class="dialog">
			<Dialog.Title class="dialog-title">{title}</Dialog.Title>
			{#if description}<Dialog.Description class="dialog-description">{description}</Dialog.Description>{/if}
			{@render children()}
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>

<style>
	:global(.dialog-scrim) {
		position: fixed;
		inset: 0;
		background: color-mix(in srgb, var(--md-sys-color-scrim) 32%, transparent);
	}
	:global(.dialog) {
		position: fixed;
		top: 50%;
		left: 50%;
		transform: translate(-50%, -50%);
		width: min(35rem, calc(100vw - 2rem));
		padding: 1.5rem;
		border-radius: var(--md-sys-shape-corner-extra-large);
		background: var(--md-sys-color-surface-container-high);
		color: var(--md-sys-color-on-surface);
		box-shadow: var(--md-sys-elevation-level3);
	}
	:global(.dialog-title) {
		margin: 0 0 1rem;
		font: var(--md-sys-typescale-headline-small);
	}
	:global(.dialog-description) {
		margin: 0 0 1rem;
		font: var(--md-sys-typescale-body-medium);
		color: var(--md-sys-color-on-surface-variant);
	}
</style>
