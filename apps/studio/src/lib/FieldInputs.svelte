<!--
  Inputs for a content type's plain fields (everything but rich text, which has its own
  editor), each with a visible label. In the editor each field is a row: label left, field right,
  a dotted rule under; `stacked` puts the label above, for dialogs.
-->
<script lang="ts">
	import { Input } from '@colophon/ui/ui/input';
	import { Textarea } from '@colophon/ui/ui/textarea';
	import { Button } from '@colophon/ui/ui/button';
	import type { FieldDefinition } from './api.ts';
	import { type FormValue, MAX_SLUG_CHARS, slugify } from './content.ts';

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

	function hint(field: FieldDefinition): string {
		const parts: string[] = [];
		if (!field.required) parts.push('Optional.');
		if (field.type === 'text') parts.push(`Up to ${field.max_chars.toLocaleString()} characters.`);
		if (field.type === 'number') {
			parts.push(field.integer ? 'A whole number.' : 'A number.');
			if (field.min !== undefined && field.max !== undefined) parts.push(`From ${field.min} to ${field.max}.`);
		}
		if (field.type === 'datetime') parts.push('Your local time.');
		if (field.type === 'slug') parts.push('Lowercase letters, digits and hyphens; unique in this collection.');
		return parts.join(' ');
	}

	/** The slug `field` would get from its source field's current text, or '' if there is none. */
	function suggestion(field: FieldDefinition): string {
		if (field.type !== 'slug' || field.source === undefined) return '';
		const source = form[field.source];
		return typeof source === 'string' ? slugify(source) : '';
	}

	function suggest(field: FieldDefinition) {
		form[field.id] = suggestion(field);
		onchange();
	}
	const select =
		'h-9 w-full rounded-md border border-input bg-card px-3 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40';
</script>

<div class={stacked ? 'grid gap-4' : ''}>
	{#each fields as field (field.id)}
		{@const input = `${id}-${field.id}`}
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
				{:else if field.type === 'slug'}
					<div class="flex max-w-xl items-center gap-2">
						<Input
							id={input}
							class="font-mono"
							maxlength={MAX_SLUG_CHARS}
							autocomplete="off"
							spellcheck="false"
							bind:value={form[field.id] as string}
							oninput={onchange}
						/>
						{#if field.source !== undefined}
							<Button
								type="button"
								variant="outline"
								size="sm"
								disabled={suggestion(field) === '' || suggestion(field) === form[field.id]}
								onclick={() => suggest(field)}
							>
								From {fields.find((other) => other.id === field.source)?.label ?? field.source}
							</Button>
						{/if}
					</div>
				{/if}
			{/if}
		</div>
	{/each}
</div>
