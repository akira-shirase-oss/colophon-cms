<!-- An M3 outlined text field with a visible label and an optional error, announced to screen readers. -->
<script lang="ts">
	import type { HTMLInputAttributes } from 'svelte/elements';

	let {
		label,
		value = $bindable(''),
		error = '',
		...rest
	}: { label: string; value?: string; error?: string } & Omit<HTMLInputAttributes, 'value'> = $props();

	const id = $props.id();
</script>

<div class="field">
	<label for={id}>{label}</label>
	<input
		{id}
		bind:value
		aria-invalid={error === '' ? undefined : true}
		aria-describedby={error === '' ? undefined : `${id}-error`}
		{...rest}
	/>
	{#if error}<p id="{id}-error" class="error" role="alert">{error}</p>{/if}
</div>

<style>
	.field {
		display: grid;
		gap: 0.25rem;
		flex: 1;
		min-width: 14rem;
	}
	label {
		font: var(--md-sys-typescale-body-small);
		color: var(--md-sys-color-on-surface-variant);
	}
	input {
		height: 3rem;
		padding: 0 1rem;
		border: 1px solid var(--md-sys-color-outline);
		border-radius: var(--md-sys-shape-corner-extra-small);
		background: transparent;
		color: var(--md-sys-color-on-surface);
		font: var(--md-sys-typescale-body-large);
	}
	input:focus-visible {
		outline: 2px solid var(--md-sys-color-primary);
		outline-offset: -1px;
	}
	input[aria-invalid='true'] {
		border-color: var(--md-sys-color-error);
	}
	.error {
		margin: 0;
		font: var(--md-sys-typescale-body-small);
		color: var(--md-sys-color-error);
	}
</style>
