<!--
  Inputs for a content type's plain fields (everything but rich text, which has its own
  editor), each with a visible label. In the editor each field is a row: label left, field right,
  a dotted rule under; `stacked` puts the label above, for dialogs.
-->
<script lang="ts">
	import { Input } from '@colophon/ui/ui/input';
	import { Textarea } from '@colophon/ui/ui/textarea';
	import { Button } from '@colophon/ui/ui/button';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import type { FieldDefinition } from './api.ts';
	import type { FormValue } from './content.ts';
	import FieldInputs from './FieldInputs.svelte';

	let {
		fields,
		form = $bindable(),
		titleField = '',
		onchange = () => {},
		stacked = false
	}: {
		fields: FieldDefinition[];
		form: Record<string, FormValue>;
		titleField?: string;
		onchange?: () => void;
		stacked?: boolean;
	} = $props();

	const id = $props.id();

	/** Lowercase letters and digits joined by single hyphens — the slug's shape. */
	function kebab(text: string): string {
		return text
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, '-')
			.replace(/^-+|-+$/g, '');
	}

	/** The native input type a list item's kind uses. */
	function itemInput(of: string): string {
		switch (of) {
			case 'number':
				return 'number';
			case 'date':
				return 'date';
			case 'time':
				return 'time';
			case 'datetime':
				return 'datetime-local';
			case 'email':
				return 'email';
			case 'url':
				return 'url';
			default:
				return 'text';
		}
	}

	function hint(field: FieldDefinition): string {
		const parts: string[] = [];
		if (!field.required) parts.push('Optional.');
		if (field.type === 'text') parts.push(`Up to ${field.max_chars.toLocaleString()} characters.`);
		if (field.type === 'number') {
			parts.push(field.integer ? 'A whole number.' : 'A number.');
			if (field.min !== undefined && field.max !== undefined) parts.push(`From ${field.min} to ${field.max}.`);
		}
		if (field.type === 'datetime') parts.push('Your local time.');
		return parts.join(' ');
	}
	const select =
		'h-9 w-full rounded-md border border-input bg-card px-3 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40';

	// Display-only: a `when` field shows only while the named field's value is one of `in`
	// (a yes/no reads "true"/"false"). A hidden field keeps whatever it stored.
	const shown = (field: FieldDefinition) =>
		field.when === undefined || field.when.in.includes(String(form[field.when.field] ?? ''));
</script>

<div class={stacked ? 'grid gap-4' : ''}>
	{#each fields as field (field.id)}
		{@const input = `${id}-${field.id}`}
		{#if shown(field)}
		<div
			class={stacked
				? 'grid gap-1.5'
				: 'rule-dotted grid gap-x-10 gap-y-2 py-5 first:pt-1 sm:grid-cols-[minmax(0,13rem)_minmax(0,1fr)]'}
		>
			{#if field.type === 'boolean'}
				<span class="text-sm font-medium" class:sr-only={stacked} aria-hidden="true">{field.label}</span>
				<label for={input} class="flex items-center gap-2.5 text-sm">
					<input id={input} type="checkbox" class="size-4 accent-primary" bind:checked={form[field.id] as boolean} {onchange} />
					{field.label}
				</label>
			{:else}
				<div class="space-y-0.5">
					<label for={input} class="text-sm font-medium">{field.label}{field.required ? '' : ' (optional)'}</label>
					{#if !stacked && hint(field)}<p class="text-xs text-muted-foreground">{hint(field)}</p>{/if}
				</div>
				{#if field.type === 'text' && field.multiline}
					<Textarea id={input} rows={4} maxlength={field.max_chars} bind:value={form[field.id] as string} oninput={onchange} />
				{:else if field.type === 'text'}
					<Input
						id={input}
						maxlength={field.max_chars}
						class={field.id === titleField ? 'h-10 text-base! font-medium' : ''}
						bind:value={form[field.id] as string}
						oninput={onchange}
					/>
				{:else if field.type === 'number'}
					<Input
						id={input}
						type="number"
						class="max-w-48 tabular"
						step={field.integer ? 1 : 'any'}
						min={field.min}
						max={field.max}
						bind:value={form[field.id] as string}
						oninput={onchange}
					/>
				{:else if field.type === 'enum'}
					<select id={input} class="{select} max-w-72" bind:value={form[field.id] as string} {onchange}>
						{#if !field.required}<option value="">None</option>{/if}
						{#each field.values as option (option)}
							<option value={option}>{option}</option>
						{/each}
					</select>
				{:else if field.type === 'datetime'}
					<Input id={input} type="datetime-local" step="1" class="max-w-72" bind:value={form[field.id] as string} oninput={onchange} />
				{:else if field.type === 'date'}
					<Input id={input} type="date" class="max-w-48" bind:value={form[field.id] as string} oninput={onchange} />
				{:else if field.type === 'time'}
					<Input id={input} type="time" step="1" class="max-w-40" bind:value={form[field.id] as string} oninput={onchange} />
				{:else if field.type === 'email'}
					<Input id={input} type="email" class="max-w-72" bind:value={form[field.id] as string} oninput={onchange} />
				{:else if field.type === 'url'}
					<Input id={input} type="url" class="max-w-72" bind:value={form[field.id] as string} oninput={onchange} />
				{:else if field.type === 'markdown'}
					<Textarea id={input} rows={6} maxlength={field.max_chars} class="max-w-xl font-mono" bind:value={form[field.id] as string} oninput={onchange} />
				{:else if field.type === 'color'}
					<Input id={input} class="w-36 font-mono" maxlength={7} placeholder="#1a6fdd" bind:value={form[field.id] as string} oninput={onchange} />
				{:else if field.type === 'slug'}
					<div class="flex items-center gap-2">
						<Input id={input} class="max-w-72 font-mono" placeholder="my-first-post" bind:value={form[field.id] as string} oninput={onchange} />
						{#if field.of}
							<Button
								variant="ghost"
								size="sm"
								type="button"
								onclick={() => {
									form[field.id] = kebab(String(form[field.of ?? ''] ?? ''));
									onchange();
								}}
							>
								From {field.of}
							</Button>
						{/if}
					</div>
				{:else if field.type === 'json'}
					<Textarea id={input} rows={8} class="max-w-xl font-mono" placeholder={'{"key": "value"}'} bind:value={form[field.id] as string} oninput={onchange} />
				{:else if field.type === 'object'}
					<fieldset class="space-y-4 rounded-lg border border-border p-4" aria-labelledby={`${input}-legend`}>
						<legend id={`${input}-legend`} class="px-1 text-sm text-muted-foreground">{field.label}</legend>
						<FieldInputs fields={field.fields} form={form[field.id] as Record<string, FormValue>} {onchange} stacked />
					</fieldset>
				{:else if field.type === 'list'}
					{@const list = form[field.id] as FormValue[]}
					<div class="space-y-2">
						{#each list as _item, index (index)}
							<div class="flex items-center gap-2">
								{#if field.of === 'boolean'}
									<label class="flex items-center gap-2 text-sm">
										<input type="checkbox" class="size-4 accent-primary" bind:checked={list[index] as boolean} {onchange} /> Item {index + 1}
									</label>
								{:else}
									<Input
										type={itemInput(field.of)}
										class="{field.of === 'number' ? 'tabular ' : ''}{field.of === 'color' ? 'w-36 font-mono ' : ''}max-w-72"
										bind:value={list[index] as string}
										oninput={onchange}
										aria-label="Item {index + 1}"
									/>
								{/if}
								<Button
									variant="ghost"
									size="icon"
									type="button"
									aria-label="Remove item {index + 1}"
									onclick={() => {
										list.splice(index, 1);
										onchange();
									}}
								>
									<Trash2Icon class="size-4" />
								</Button>
							</div>
						{/each}
						{#if list.length < field.max_items}
							<Button
								variant="outline"
								size="sm"
								type="button"
								onclick={() => {
									list.push(field.of === 'boolean' ? false : '');
									onchange();
								}}
							>
								<PlusIcon class="size-4" /> Add an item
							</Button>
						{/if}
					</div>
				{/if}
			{/if}
		</div>
		{/if}
	{/each}
</div>
