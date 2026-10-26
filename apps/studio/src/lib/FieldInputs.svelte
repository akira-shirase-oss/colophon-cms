<!--
  Inputs for a content type's plain fields (everything but rich text, which has its own
  editor), each with a visible label. Used by the "new entry" form and by the editor.
-->
<script lang="ts">
	import type { FieldDefinition } from './api.ts';
	import type { FormValue } from './content.ts';

	let {
		fields,
		form = $bindable(),
		titleField = '',
		onchange = () => {}
	}: {
		fields: FieldDefinition[];
		form: Record<string, FormValue>;
		titleField?: string;
		onchange?: () => void;
	} = $props();

	const id = $props.id();
</script>

<div class="fields">
	{#each fields as field (field.id)}
		{@const input = `${id}-${field.id}`}
		<div class="field" class:title={field.id === titleField} class:check={field.type === 'boolean'}>
			{#if field.type === 'boolean'}
				<label for={input}>
					<input id={input} type="checkbox" bind:checked={form[field.id] as boolean} {onchange} />
					{field.label}
				</label>
			{:else}
				<label for={input}>{field.label}{field.required ? '' : ' (optional)'}</label>
				{#if field.type === 'text' && field.multiline}
					<textarea id={input} rows="4" maxlength={field.max_chars} bind:value={form[field.id] as string} oninput={onchange}></textarea>
				{:else if field.type === 'text'}
					<input id={input} maxlength={field.max_chars} bind:value={form[field.id] as string} oninput={onchange} />
				{:else if field.type === 'number'}
					<input
						id={input}
						type="number"
						step={field.integer ? 1 : 'any'}
						min={field.min}
						max={field.max}
						bind:value={form[field.id] as string}
						oninput={onchange}
					/>
				{:else if field.type === 'enum'}
					<select id={input} bind:value={form[field.id] as string} {onchange}>
						{#if !field.required}<option value="">None</option>{/if}
						{#each field.values as option (option)}
							<option value={option}>{option}</option>
						{/each}
					</select>
				{:else if field.type === 'datetime'}
					<input id={input} type="datetime-local" step="1" bind:value={form[field.id] as string} oninput={onchange} />
				{/if}
			{/if}
		</div>
	{/each}
</div>

<style>
	.fields {
		display: grid;
		gap: 0.75rem;
	}
	.field {
		display: grid;
		gap: 0.25rem;
		font: var(--md-sys-typescale-body-small);
		color: var(--md-sys-color-on-surface-variant);
	}
	.field.check label {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		font: var(--md-sys-typescale-body-medium);
		color: var(--md-sys-color-on-surface);
	}
	input:not([type='checkbox']),
	textarea,
	select {
		font: var(--md-sys-typescale-body-large);
		color: var(--md-sys-color-on-surface);
		background: transparent;
		border: 1px solid var(--md-sys-color-outline);
		border-radius: var(--md-sys-shape-corner-extra-small, 4px);
		padding: 0.5rem 0.75rem;
	}
	.title input {
		font: var(--md-sys-typescale-headline-small);
	}
	input:focus-visible,
	textarea:focus-visible,
	select:focus-visible {
		outline: 2px solid var(--md-sys-color-primary);
		outline-offset: 1px;
	}
</style>
