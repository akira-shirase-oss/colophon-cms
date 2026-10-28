<!--
  Inputs for a content type's plain fields (everything but rich text, which has its own
  editor), each with a visible label, its help text, and what is wrong with it, if anything. In
  the editor each field is a row: label left, field right, a dotted rule under; `stacked` puts the
  label above, for dialogs. Since D-064: native inputs for email, web address, date, time and
  colour, a counted textarea for markdown, and a monospaced one for JSON. The checks here mirror
  the server's (lib/content.ts); the server still decides, and a refusal it names a field for is
  shown under that field.
-->
<script lang="ts">
	import { Input } from '@colophon/ui/ui/input';
	import { Textarea } from '@colophon/ui/ui/textarea';
	import { Button } from '@colophon/ui/ui/button';
	import ReferencePicker from './ReferencePicker.svelte';
	import RichTextInput from './RichTextInput.svelte';
	import StructureInput from './StructureInput.svelte';
	import type { FieldDefinition } from './api.ts';
	import { patternMismatch } from './words.ts';
	import { characters, fieldProblem, type FormValue, isWebAddress, MAX_SLUG_CHARS, slugify, webAddress } from './content.ts';

	let {
		fields,
		form = $bindable(),
		titleField = '',
		onchange = () => {},
		stacked = false,
		locked = [],
		refused = null,
		project = '',
		base = ''
	}: {
		fields: FieldDefinition[];
		form: Record<string, FormValue>;
		titleField?: string;
		onchange?: () => void;
		stacked?: boolean;
		/** Fields the person's role can read but not change (D-058, step R.3): shown, disabled. */
		locked?: string[];
		/** The server's refusal of the last save, where (its JSON Pointer) and why. */
		refused?: { pointer: string | null; message: string; reason?: string | null } | null;
		/** For link fields, which search and read the entries they point to. */
		project?: string;
		/** These fields' place in the content, as a JSON Pointer ('' for the entry's own fields;
		 *  `/sections/1` inside a page section), so refusals land under the right control. */
		base?: string;
	} = $props();

	const at = (field: FieldDefinition) => `${base}/${field.id}`;
	/** What is wrong with a field now: the client's check, else the server's refusal of it. */
	function problemOf(field: FieldDefinition): string {
		// A list of links is refused at one of its places (`/related/1`): that's this field's too.
		const here = refused?.pointer === at(field) || (field.type === 'reference' && refusedPlace(field) !== null);
		// Not in its allowed form: the server's answer counts (its regex isn't imitated here), said
		// with the field's help text, where a type says what the form is.
		if (here && refused?.reason === 'pattern_mismatch') return patternMismatch(field.label, field.description);
		return fieldProblem(field, form[field.id]) || (here ? (refused?.message ?? '') : '');
	}
	/** For a list of links: the place of the id the server refused. */
	function refusedPlace(field: FieldDefinition): number | null {
		const rest = refused?.pointer?.startsWith(`${at(field)}/`) ? refused.pointer.slice(at(field).length + 1) : '';
		return /^\d+$/.test(rest) ? Number(rest) : null;
	}
	const structure = (field: FieldDefinition) => field.type === 'object' || field.type === 'list' || field.type === 'blocks';
	/** The ids describing a field: its help and its problem. */
	function describedBy(input: string, field: FieldDefinition): string | undefined {
		const ids = [field.description ? `${input}-help` : '', problemOf(field) ? `${input}-problem` : ''].filter((id) => id !== '');
		return ids.length > 0 ? ids.join(' ') : undefined;
	}
	/** A colour field's swatch needs a valid colour; anything else shows black until fixed. */
	const swatch = (value: FormValue | undefined) => (typeof value === 'string' && /^#[0-9a-f]{6}$/i.test(value.trim()) ? value.trim().toLowerCase() : '#000000');

	const id = $props.id();

	function hint(field: FieldDefinition): string {
		const parts: string[] = [];
		if (!field.required) parts.push('Optional.');
		if (field.type === 'text') {
			parts.push(
				field.min_chars === undefined
					? `Up to ${field.max_chars.toLocaleString()} characters.`
					: `From ${field.min_chars.toLocaleString()} to ${field.max_chars.toLocaleString()} characters.`
			);
		}
		if (field.type === 'email') parts.push('An email address.');
		if (field.type === 'url') parts.push('A web address starting with https://.');
		if (field.type === 'time') parts.push('24-hour, like 14:30.');
		if (field.type === 'color') parts.push('Pick it, or type it like #1a2b3c.');
		if (field.type === 'json') parts.push('Structured data, written as JSON.');
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
			{#if structure(field) || field.type === 'rich_text'}
				<!-- A structure has no one input to label: its name heads it, and it groups its own. -->
				<div class="space-y-0.5">
					<p class="text-sm font-medium" id="{input}-label">{field.label}{field.required ? '' : ' (optional)'}{locked.includes(field.id) ? ' (your role can’t change it)' : ''}</p>
					{#if field.description}<p class="text-xs text-pretty text-muted-foreground" id="{input}-help" data-user-content>{field.description}</p>{/if}
				</div>
				<div class="grid min-w-0 gap-1.5">
					{#if field.type === 'rich_text'}
						<RichTextInput bind:value={form[field.id] as FormValue} label={field.label} labelledBy="{input}-label" disabled={locked.includes(field.id)} {onchange} />
					{:else}
						<StructureInput
							{project}
							{field}
							bind:value={form[field.id] as FormValue}
							base={at(field)}
							{refused}
							disabled={locked.includes(field.id)}
							labelledBy="{input}-label"
							{onchange}
						/>
					{/if}
					{#if problemOf(field)}
						<p class="text-xs text-destructive" id="{input}-problem" data-testid="field-problem" data-field={at(field).slice(1)}>{problemOf(field)}</p>
					{/if}
				</div>
			{:else if field.type === 'boolean'}
				<span class="text-sm font-medium" class:sr-only={stacked} aria-hidden="true">{field.label}</span>
				<label for={input} class="flex items-center gap-2.5 text-sm">
					<input id={input} disabled={locked.includes(field.id)} type="checkbox" class="size-4 accent-primary" bind:checked={form[field.id] as boolean} {onchange} />
					{field.label}
				</label>
			{:else}
				<div class="space-y-0.5">
					<label for={input} class="text-sm font-medium">{field.label}{field.required ? '' : ' (optional)'}{locked.includes(field.id) ? ' (your role can’t change it)' : ''}</label>
					{#if field.description}<p class="text-xs text-pretty text-muted-foreground" id="{input}-help" data-user-content>{field.description}</p>{/if}
					{#if !stacked && hint(field)}<p class="text-xs text-muted-foreground">{hint(field)}</p>{/if}
				</div>
				<div class="grid min-w-0 gap-1.5">
				{#if field.type === 'text' && field.multiline}
					<Textarea id={input} disabled={locked.includes(field.id)} rows={4} maxlength={field.max_chars} bind:value={form[field.id] as string} oninput={onchange} />
				{:else if field.type === 'text'}
					<Input
						id={input}
						disabled={locked.includes(field.id)}
						maxlength={field.max_chars}
						class={field.id === titleField ? 'h-10 text-base! font-medium' : ''}
						bind:value={form[field.id] as string}
						oninput={onchange}
					/>
				{:else if field.type === 'number'}
					<Input
						id={input}
						disabled={locked.includes(field.id)}
						type="number"
						class="max-w-48 tabular"
						step={field.integer ? 1 : 'any'}
						min={field.min}
						max={field.max}
						bind:value={form[field.id] as string}
						oninput={onchange}
					/>
				{:else if field.type === 'enum'}
					<select id={input} disabled={locked.includes(field.id)} class="{select} max-w-72" bind:value={form[field.id] as string} {onchange}>
						{#if !field.required}<option value="">None</option>{/if}
						{#each field.values as option (option)}
							<option value={option}>{option}</option>
						{/each}
					</select>
				{:else if field.type === 'datetime'}
					<Input id={input} disabled={locked.includes(field.id)} type="datetime-local" step="1" class="max-w-72" bind:value={form[field.id] as string} oninput={onchange} />
				{:else if field.type === 'slug'}
					<div class="flex max-w-xl items-center gap-2">
						<Input
							id={input}
							disabled={locked.includes(field.id)}
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
								disabled={locked.includes(field.id) || suggestion(field) === '' || suggestion(field) === form[field.id]}
								onclick={() => suggest(field)}
							>
								From {fields.find((other) => other.id === field.source)?.label ?? field.source}
							</Button>
						{/if}
					</div>
				{:else if field.type === 'email'}
					<Input
						id={input}
						disabled={locked.includes(field.id)}
						type="email"
						autocomplete="off"
						spellcheck="false"
						class="max-w-md"
						aria-invalid={problemOf(field) ? true : undefined}
						aria-describedby={describedBy(input, field)}
						bind:value={form[field.id] as string}
						oninput={onchange}
					/>
				{:else if field.type === 'url'}
					<Input
						id={input}
						disabled={locked.includes(field.id)}
						type="url"
						autocomplete="off"
						spellcheck="false"
						placeholder="https://"
						class="max-w-xl"
						aria-invalid={problemOf(field) ? true : undefined}
						aria-describedby={describedBy(input, field)}
						bind:value={form[field.id] as string}
						oninput={onchange}
						onblur={() => {
							// Show it as it will be stored, once it is a web address.
							const text = String(form[field.id] ?? '').trim();
							if (isWebAddress(text) && webAddress(text) !== form[field.id]) {
								form[field.id] = webAddress(text);
								onchange();
							}
						}}
					/>
				{:else if field.type === 'date'}
					<Input
						id={input}
						disabled={locked.includes(field.id)}
						type="date"
						class="max-w-48"
						aria-invalid={problemOf(field) ? true : undefined}
						aria-describedby={describedBy(input, field)}
						bind:value={form[field.id] as string}
						oninput={onchange}
					/>
				{:else if field.type === 'time'}
					<Input
						id={input}
						disabled={locked.includes(field.id)}
						type="time"
						class="max-w-40"
						aria-invalid={problemOf(field) ? true : undefined}
						aria-describedby={describedBy(input, field)}
						bind:value={form[field.id] as string}
						oninput={onchange}
					/>
				{:else if field.type === 'color'}
					<div class="flex max-w-xs items-center gap-2">
						<input
							type="color"
							class="h-9 w-12 shrink-0 cursor-pointer rounded-md border border-input bg-card p-1 disabled:cursor-not-allowed disabled:opacity-60"
							aria-label="Pick {field.label}"
							disabled={locked.includes(field.id)}
							value={swatch(form[field.id])}
							oninput={(event) => {
								form[field.id] = event.currentTarget.value.toLowerCase();
								onchange();
							}}
						/>
						<Input
							id={input}
							disabled={locked.includes(field.id)}
							class="font-mono"
							maxlength={7}
							autocomplete="off"
							spellcheck="false"
							placeholder="#1a2b3c"
							aria-invalid={problemOf(field) ? true : undefined}
							aria-describedby={describedBy(input, field)}
							bind:value={form[field.id] as string}
							oninput={onchange}
						/>
					</div>
				{:else if field.type === 'markdown'}
					<Textarea
						id={input}
						disabled={locked.includes(field.id)}
						rows={8}
						class="font-mono text-sm"
						spellcheck="true"
						aria-invalid={problemOf(field) ? true : undefined}
						aria-describedby={[describedBy(input, field), `${input}-count`].filter(Boolean).join(' ')}
						bind:value={form[field.id] as string}
						oninput={onchange}
					/>
					<p class="text-xs text-muted-foreground tabular" id="{input}-count" data-testid="markdown-count">
						{characters(String(form[field.id] ?? '')).toLocaleString()} of {field.max_chars.toLocaleString()} characters
					</p>
				{:else if field.type === 'reference'}
					<ReferencePicker
						{project}
						{field}
						input={input}
						bind:value={form[field.id] as string | string[]}
						disabled={locked.includes(field.id)}
						describedBy={describedBy(input, field)}
						refusedAt={refusedPlace(field)}
						refusal={refusedPlace(field) === null ? '' : (refused?.message ?? '')}
						{onchange}
					/>
				{:else if field.type === 'json'}
					<Textarea
						id={input}
						disabled={locked.includes(field.id)}
						rows={8}
						class="font-mono text-xs"
						spellcheck="false"
						autocomplete="off"
						aria-invalid={problemOf(field) ? true : undefined}
						aria-describedby={describedBy(input, field)}
						bind:value={form[field.id] as string}
						oninput={onchange}
					/>
				{/if}
				{#if problemOf(field)}
					<p class="text-xs text-destructive" id="{input}-problem" data-testid="field-problem" data-field={at(field).slice(1)}>{problemOf(field)}</p>
				{/if}
				</div>
			{/if}
		</div>
	{/each}
</div>
