<!--
  A structure's value in the editor (D-066): a group of fields as a fieldset; a list as rows that
  can be added, moved and removed, up to its maximum; page sections as a stack, each with its
  kind's label, collapsible, movable and removable, added by choosing a kind. Every control inside
  is the editor's own (FieldInputs, which comes back here for structures inside structures), and
  each sits at its JSON Pointer (`/faq/0/q`, `/sections/1/by`), so a refusal lands under it.
  Moving or removing items re-creates the controls below, so none keeps a stale item.
-->
<script lang="ts">
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import XIcon from '@lucide/svelte/icons/x';
	import { Button } from '@colophon/ui/ui/button';
	import type { FieldDefinition } from './api.ts';
	import { blockOf, type FormRecord, type FormValue, formValue, isRecord, itemField, sectionForm } from './content.ts';
	import FieldInputs from './FieldInputs.svelte';

	type Structure = Extract<FieldDefinition, { type: 'object' | 'list' | 'blocks' }>;

	let {
		project,
		field,
		value = $bindable(),
		base,
		refused = null,
		disabled = false,
		labelledBy,
		onchange = () => {}
	}: {
		project: string;
		field: Structure;
		value: FormValue;
		/** This structure's JSON Pointer. */
		base: string;
		refused?: { pointer: string | null; message: string } | null;
		disabled?: boolean;
		labelledBy?: string;
		onchange?: () => void;
	} = $props();

	// Lists and sections are arrays; a group is a record. Start empty when there's nothing yet
	// (once: the parent makes these controls afresh when the whole entry is replaced).
	// svelte-ignore state_referenced_locally
	if (field.type === 'object' && !isRecord(value)) value = formValue(field, undefined);
	// svelte-ignore state_referenced_locally
	if (field.type !== 'object' && !Array.isArray(value)) value = [];
	const items = $derived(Array.isArray(value) ? value : []);
	/** Bumped when items move or go, so the controls below are made afresh for their new places. */
	let order = $state(0);
	const full = $derived(field.type !== 'object' && items.length >= field.max);

	function move(index: number, by: number) {
		if (!Array.isArray(value)) return;
		const [item] = value.splice(index, 1);
		if (item !== undefined) value.splice(index + by, 0, item);
		order += 1;
		onchange();
	}
	function remove(index: number) {
		if (!Array.isArray(value)) return;
		value.splice(index, 1);
		order += 1;
		onchange();
	}

	/** A list's items as fields of their own, named by their place: FieldInputs reads and writes
	 *  `items[place]`, at `${base}/${place}`, as the server points. */
	const rows = $derived(
		field.type === 'list' ? items.map((_, index) => ({ ...itemField(field), id: String(index), label: `${field.label} ${index + 1}` }) as FieldDefinition) : []
	);
	function addItem() {
		if (field.type !== 'list' || !Array.isArray(value) || full) return;
		// Not saved until something is typed into it: an empty item isn't anything yet.
		(value as FormValue[]).push(formValue(itemField(field), undefined));
	}

	// Page sections: each collapsible; a new one opens.
	let closed = $state<Record<number, boolean>>({});
	let adding = $state('');
	function addSection() {
		if (field.type !== 'blocks' || !Array.isArray(value) || full) return;
		const kind = field.blocks.find((block) => block.name === (adding || field.blocks[0]?.name));
		if (kind === undefined) return;
		(value as FormValue[]).push(sectionForm(field, { _block: kind.name }));
		onchange();
	}
	const sectionLabel = (item: FormValue, index: number) =>
		`${(field.type === 'blocks' && isRecord(item) ? blockOf(field, item)?.label : undefined) ?? 'Section'} ${index + 1}`;
	const select =
		'h-9 rounded-md border border-input bg-card px-3 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40';
</script>

{#if field.type === 'object'}
	<fieldset class="rounded-xl border border-border bg-card/60 p-4" aria-labelledby={labelledBy} data-testid="group-{base.slice(1)}">
		<FieldInputs fields={field.fields} form={value as FormRecord} stacked {project} {base} {refused} locked={disabled ? field.fields.map((child) => child.id) : []} {onchange} />
	</fieldset>
{:else if field.type === 'list'}
	<div class="grid gap-2" role="group" aria-labelledby={labelledBy} data-testid="list-{base.slice(1)}">
		{#key order}
			{#each rows as row, index (row.id)}
				<div class="flex items-start gap-2 rounded-xl border border-border bg-card/60 p-3" data-testid="list-item">
					<div class="min-w-0 flex-1">
						<FieldInputs
							fields={[row]}
							form={items as unknown as FormRecord}
							stacked
							{project}
							{base}
							{refused}
							locked={disabled ? [row.id] : []}
							{onchange}
						/>
					</div>
					{#if !disabled}
						<div class="flex shrink-0 gap-0.5 pt-6">
							<Button variant="ghost" size="icon-xs" aria-label="Move {row.label} up" disabled={index === 0} onclick={() => move(index, -1)}><ArrowUpIcon /></Button>
							<Button variant="ghost" size="icon-xs" aria-label="Move {row.label} down" disabled={index === items.length - 1} onclick={() => move(index, 1)}><ArrowDownIcon /></Button>
							<Button variant="ghost" size="icon-xs" aria-label="Remove {row.label}" onclick={() => remove(index)}><XIcon /></Button>
						</div>
					{/if}
				</div>
			{/each}
		{/key}
		{#if !disabled}
			{#if full}
				<p class="text-xs text-muted-foreground">At most {field.max} items: remove one to add another.</p>
			{:else}
				<div><Button variant="outline" size="sm" onclick={addItem} data-testid="list-add"><PlusIcon data-icon="inline-start" />Add to {field.label}</Button></div>
			{/if}
		{/if}
	</div>
{:else}
	<div class="grid gap-2" role="group" aria-labelledby={labelledBy} data-testid="sections-{base.slice(1)}">
		{#key order}
			{#each items as item, index (index)}
				{@const kind = field.type === 'blocks' && isRecord(item) ? blockOf(field, item) : undefined}
				{@const open = !closed[index]}
				<section class="overflow-hidden rounded-xl border border-border bg-card/60" aria-label={sectionLabel(item, index)} data-testid="section" data-kind={isRecord(item) ? String(item['_block']) : ''}>
					<div class="flex items-center gap-1 px-3 py-2">
						<button
							type="button"
							class="flex min-w-0 flex-1 items-center gap-2 rounded-md text-left text-sm font-medium outline-none focus-visible:ring-[3px] focus-visible:ring-ring/40"
							aria-expanded={open}
							aria-controls="{base}-{index}"
							onclick={() => (closed[index] = open)}
						>
							<ChevronDownIcon class="size-4 shrink-0 transition-transform {open ? '' : '-rotate-90'}" aria-hidden="true" />
							<span class="truncate">{sectionLabel(item, index)}</span>
						</button>
						{#if !disabled}
							<Button variant="ghost" size="icon-xs" aria-label="Move {sectionLabel(item, index)} up" disabled={index === 0} onclick={() => move(index, -1)}><ArrowUpIcon /></Button>
							<Button variant="ghost" size="icon-xs" aria-label="Move {sectionLabel(item, index)} down" disabled={index === items.length - 1} onclick={() => move(index, 1)}><ArrowDownIcon /></Button>
							<Button variant="ghost" size="icon-xs" aria-label="Remove {sectionLabel(item, index)}" onclick={() => remove(index)}><XIcon /></Button>
						{/if}
					</div>
					{#if refused?.pointer === `${base}/${index}/_block` || refused?.pointer === `${base}/${index}`}
						<p class="px-3 pb-2 text-xs text-destructive" data-testid="field-problem" data-field="{base.slice(1)}/{index}">{refused.message}</p>
					{/if}
					<div id="{base}-{index}" class="border-t border-dotted border-border px-4 py-3" hidden={!open}>
						{#if kind}
							<FieldInputs
								fields={kind.fields}
								form={item as FormRecord}
								stacked
								{project}
								base="{base}/{index}"
								{refused}
								locked={disabled ? kind.fields.map((child) => child.id) : []}
								{onchange}
							/>
						{:else}
							<p class="text-sm text-muted-foreground">This section's kind no longer exists in the content type.</p>
						{/if}
					</div>
				</section>
			{/each}
		{/key}
		{#if !disabled}
			{#if full}
				<p class="text-xs text-muted-foreground">At most {field.max} sections: remove one to add another.</p>
			{:else}
				<div class="flex flex-wrap items-center gap-2">
					<select class={select} bind:value={adding} aria-label="Kind of section to add to {field.label}" data-testid="section-kind">
						{#each field.blocks as block (block.name)}<option value={block.name}>{block.label}</option>{/each}
					</select>
					<Button variant="outline" size="sm" onclick={addSection} data-testid="section-add"><PlusIcon data-icon="inline-start" />Add section</Button>
				</div>
			{/if}
		{/if}
	</div>
{/if}
