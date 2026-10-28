<!--
  The fields of a content type (or a component, a group, a list's item or a kind of page section)
  in the builder: one compact row each, with its icon, label, kind in plain words and badges
  (Title, Required, Unique, Shown only when…, New), and Edit and Delete. Settings open in
  FieldDialog.svelte; "Add a field" opens the picker (FieldPicker.svelte) and then the dialog.
  Rows reorder by dragging their handle, or with the arrow keys on it. Structures (D-066) hold
  their own fields, listed under their row one level in: a group's, a list's item group's, and
  each kind of page section's. A shared component's fields (D-073) are listed but changed on its
  page. Every change lands in the unsaved changes; nothing is sent until Save.
-->
<script lang="ts">
	import GripVerticalIcon from '@lucide/svelte/icons/grip-vertical';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { resolve } from '$app/paths';
	import { toast } from 'svelte-sonner';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { type Component, type ContentType, type FieldDefinition, type FieldType } from './api.ts';
	import { blank, blankBlock, componentField, componentFits, describeField, refusalAt } from './fieldDefs.ts';
	import { iconOf } from './fieldIcons.ts';
	import FieldDialog from './FieldDialog.svelte';
	import FieldList from './FieldList.svelte';
	import FieldPicker from './FieldPicker.svelte';

	let {
		fields,
		depth,
		editable,
		saved,
		path = '',
		titleField = '',
		collections,
		self,
		within = '',
		pointer = '/fields',
		refusal = null,
		components = [],
		insideComponent = false,
		project
	}: {
		/** The fields, edited in place. */
		fields: FieldDefinition[];
		/** How many structures these fields sit inside (0: the content type's own). */
		depth: number;
		editable: boolean;
		/** Paths of the fields already saved ("hero/title"): the others are new. */
		saved: Set<string>;
		/** These fields' place, for test ids ("hero", "sections/quote", "faq/item"). */
		path?: string;
		titleField?: string;
		collections: ContentType[];
		self: string;
		/** What these fields belong to, for labels ("Hero", "Quote section"); "" at the top. */
		within?: string;
		/** These fields' place in the definition, as a JSON Pointer. */
		pointer?: string;
		refusal?: { pointer: string; message: string } | null;
		components?: Component[];
		/** These are a shared component's own fields. */
		insideComponent?: boolean;
		project: string;
	} = $props();

	const placeOf = (id: string) => (path === '' ? id : `${path}/${id}`);
	const componentName = (slug: string) => components.find((item) => item.slug === slug)?.definition.name ?? slug;
	const target = $derived(collections.find((item) => item.slug !== self)?.slug ?? self);
	const hasSlug = $derived(fields.some((field) => field.type === 'slug'));
	const valueWords = (value: unknown) => (value === true ? 'Yes' : value === false ? 'No' : String(value));

	// Adding: the picker, then the dialog with a blank field of the kind picked.
	let picking = $state(false);
	let adding = $state<FieldDefinition | null>(null);
	function pick(kind: FieldType | 'component') {
		picking = false;
		const shared = kind === 'component' ? components.find((item) => componentFits(item, depth)) : undefined;
		adding = shared !== undefined ? componentField('', '', shared) : blank(kind as FieldType, '', '', target, titleField);
	}

	// Editing: the dialog on a copy of the field at `index`.
	let editing = $state<number | null>(null);

	function remove(index: number) {
		const [gone] = fields.splice(index, 1);
		if (gone === undefined) return;
		if (editable) {
			toast(`Deleted ${gone.label}. Save to make it final.`, {
				action: { label: 'Undo', onClick: () => fields.splice(Math.min(index, fields.length), 0, gone) }
			});
		}
	}

	// Reordering: drag a row by its handle, or focus the handle and press the arrow keys.
	let said = $state('');
	let dragging = $state<number | null>(null);
	let over = $state<number | null>(null);
	function move(from: number, to: number) {
		if (to < 0 || to >= fields.length || from === to) return;
		const [field] = fields.splice(from, 1);
		if (field === undefined) return;
		fields.splice(to, 0, field);
		said = `${field.label} moved to place ${to + 1} of ${fields.length}.`;
	}
	function keyed(event: KeyboardEvent, index: number) {
		const to = event.key === 'ArrowUp' ? index - 1 : event.key === 'ArrowDown' ? index + 1 : event.key === 'Home' ? 0 : event.key === 'End' ? fields.length - 1 : null;
		if (to === null) return;
		event.preventDefault();
		move(index, to);
		// Focus follows the row to its new place.
		requestAnimationFrame(() => document.querySelector<HTMLElement>(`[data-handle="${CSS.escape(placeOf(fields[to]?.id ?? ''))}"]`)?.focus());
	}

	// Kinds of page section: a label, and the name items will carry, made from it.
	let newKind = $state<Record<string, string>>({});
	function addKind(field: FieldDefinition) {
		if (field.type !== 'blocks') return;
		const label = (newKind[field.id] ?? '').trim();
		const kind = blankBlock(label);
		if (label === '' || kind.name === '' || field.blocks.some((block) => block.name === kind.name)) return;
		field.blocks.push(kind);
		newKind[field.id] = '';
	}
</script>

<div class="grid gap-2">
	<ol class="overflow-hidden rounded-xl border border-border bg-card" aria-label={within === '' ? 'Fields' : `Fields in ${within}`} data-testid={path === '' ? 'field-list' : undefined}>
		{#each fields as field, index (field.id)}
			{@const place = placeOf(field.id)}
			{@const at = `${pointer}/${index}`}
			{@const isTitle = depth === 0 && field.id === titleField}
			{@const isNew = !saved.has(place)}
			{@const Icon = iconOf(field)}
			{@const problem = refusal === null ? null : refusalAt(refusal.pointer, at)}
			{@const controller = field.when === undefined ? undefined : fields.find((other) => other.id === field.when?.field)}
			<li
				class="rule-dotted last:border-b-0 {over === index && dragging !== null && dragging !== index ? 'bg-brand-soft/40 shadow-[inset_0_2px_0_var(--color-ring)]' : ''}"
				data-testid="field-{place}"
				ondragover={(event) => {
					if (dragging === null) return;
					event.preventDefault();
					over = index;
				}}
				ondrop={(event) => {
					event.preventDefault();
					if (dragging !== null) move(dragging, index);
					dragging = null;
					over = null;
				}}
			>
				<div class="flex items-center gap-3 px-3 py-2.5 {problem !== null ? 'bg-attention-soft/50' : ''}">
					{#if editable}
						<button
							type="button"
							class="flex size-7 shrink-0 cursor-grab items-center justify-center rounded-md text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-4 focus-visible:ring-ring/25 active:cursor-grabbing"
							draggable="true"
							aria-label="Move {field.label}"
							title="Drag to move, or use the arrow keys"
							aria-describedby="move-help"
							data-handle={place}
							ondragstart={(event) => {
								dragging = index;
								event.dataTransfer?.setData('text/plain', field.id);
								if (event.dataTransfer) event.dataTransfer.effectAllowed = 'move';
							}}
							ondragend={() => ((dragging = null), (over = null))}
							onkeydown={(event) => keyed(event, index)}><GripVerticalIcon class="size-4" /></button
						>
					{/if}
					<span class="flex size-8 shrink-0 items-center justify-center rounded-lg border border-border bg-background" aria-hidden="true"><Icon class="size-4" /></span>
					<div class="grid min-w-0 flex-1 gap-0.5">
						<div class="flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1">
							<span class="truncate text-sm font-medium" data-testid="field-label-text">{field.label}</span>
							{#if isTitle}<Badge variant="outline">Title</Badge>{/if}
							{#if field.required && !isTitle}<Badge variant="secondary">Required</Badge>{/if}
							{#if field.unique === true}<Badge variant="secondary">Unique</Badge>{/if}
							{#if field.when !== undefined}
								<Badge variant="outline" title="Shown only when {controller?.label ?? field.when.field} is {field.when.is.map(valueWords).join(' or ')}"
									>Shown only when {controller?.label ?? field.when.field} is {field.when.is.map(valueWords).join(' or ')}</Badge
								>
							{/if}
							{#if isNew}<Badge variant="brand">New</Badge>{/if}
						</div>
						<span class="truncate text-xs text-muted-foreground" data-testid="field-kind">{describeField(field, componentName)}</span>
					</div>
					<Button variant="ghost" size="icon-sm" aria-label="{editable ? 'Edit' : 'View'} {field.label}" title={editable ? 'Edit' : 'View'} onclick={() => (editing = index)} data-testid="edit-field"
						><PencilIcon /></Button
					>
					{#if editable}
						<Button
							variant="ghost"
							size="icon-sm"
							aria-label="Delete {field.label}"
							title={isTitle ? 'The title field stays' : 'Delete'}
							disabled={isTitle}
							onclick={() => remove(index)}
							data-testid="delete-field"><Trash2Icon /></Button
						>
					{/if}
				</div>
				{#if problem !== null && refusal}
					<p class="px-3 pb-2.5 pl-14 text-xs text-destructive" data-testid="row-problem">{refusal.message} <button type="button" class="font-medium underline underline-offset-2" onclick={() => (editing = index)}>Open its settings</button></p>
				{/if}

				{#if field.type === 'object' && field.component !== undefined}
					<div class="mx-3 mb-3 ml-14 rounded-lg border border-dashed border-border px-3 py-2 text-xs text-muted-foreground" data-testid="component-{place}">
						{field.fields.map((child) => child.label).join(', ')} ·
						<a class="font-medium text-foreground underline underline-offset-2" href={resolve('/projects/[project]/settings/components/[component]', { project, component: field.component })}
							>Change in {componentName(field.component)}</a
						>
					</div>
				{:else if field.type === 'object'}
					<div class="mb-3 mr-3 ml-14" data-testid="group-{place}">
						<FieldList fields={field.fields} depth={depth + 1} {editable} {saved} path={place} {collections} {self} within={field.label} pointer="{at}/fields" {refusal} {components} {insideComponent} {project} />
					</div>
				{:else if field.type === 'list' && field.of.type === 'object' && field.of.component !== undefined}
					<div class="mx-3 mb-3 ml-14 rounded-lg border border-dashed border-border px-3 py-2 text-xs text-muted-foreground" data-testid="component-{place}/item">
						Each item: {field.of.fields.map((child) => child.label).join(', ')} ·
						<a class="font-medium text-foreground underline underline-offset-2" href={resolve('/projects/[project]/settings/components/[component]', { project, component: field.of.component })}
							>Change in {componentName(field.of.component)}</a
						>
					</div>
				{:else if field.type === 'list' && field.of.type === 'object'}
					<div class="mb-3 mr-3 ml-14" data-testid="group-{place}/item">
						<FieldList fields={field.of.fields} depth={depth + 2} {editable} {saved} path="{place}/item" {collections} {self} within="{field.label} item" pointer="{at}/of/fields" {refusal} {components} {insideComponent} {project} />
					</div>
				{:else if field.type === 'blocks'}
					<div class="mb-3 mr-3 ml-14 grid gap-2" data-testid="blocks-{place}">
						{#each field.blocks as kind, kindIndex (kind.name)}
							<div class="rounded-xl border border-dashed border-border p-2.5" data-testid="kind-{place}/{kind.name}">
								<div class="mb-2 flex flex-wrap items-center gap-2">
									<Input class="h-8 max-w-56" aria-label="Name of the {kind.label} section" bind:value={kind.label} maxlength={100} disabled={!editable} />
									<span class="text-xs text-muted-foreground">section</span>
									<span class="flex-1"></span>
									{#if editable}
										<Button variant="ghost" size="icon-xs" aria-label="Delete the {kind.label} section" onclick={() => field.type === 'blocks' && field.blocks.splice(kindIndex, 1)}><Trash2Icon /></Button>
									{/if}
								</div>
								<FieldList
									fields={kind.fields}
									depth={depth + 1}
									{editable}
									{saved}
									path="{place}/{kind.name}"
									{collections}
									{self}
									within="{kind.label} section"
									pointer="{at}/blocks/{kindIndex}/fields"
									{refusal}
									{components}
									{insideComponent}
									{project}
								/>
							</div>
						{/each}
						{#if editable}
							<form class="flex flex-wrap items-end gap-2" onsubmit={(event) => (event.preventDefault(), addKind(field))} aria-label="Add a kind of section to {field.label}">
								<label class="grid gap-1">
									<span class="text-xs text-muted-foreground">New kind of section</span>
									<Input class="h-8 w-56" placeholder="Quote, Gallery, Call to action" bind:value={newKind[field.id]} maxlength={100} />
								</label>
								<Button type="submit" variant="outline" size="sm" disabled={(newKind[field.id] ?? '').trim() === ''}>Add kind of section</Button>
							</form>
						{/if}
					</div>
				{/if}
			</li>
		{:else}
			<li class="px-4 py-3 text-sm text-muted-foreground">No fields yet.</li>
		{/each}
	</ol>

	{#if editable}
		<button
			type="button"
			class="flex items-center justify-center gap-2 rounded-xl border border-dashed border-border px-4 py-2.5 text-sm font-medium text-muted-foreground outline-none hover:border-ring/60 hover:bg-card hover:text-foreground focus-visible:border-ring focus-visible:ring-4 focus-visible:ring-ring/25"
			onclick={() => (picking = true)}
			data-testid={path === '' ? 'add-field' : `add-field-${path}`}><PlusIcon class="size-4" />{within === '' ? 'Add a field' : `Add a field to ${within}`}</button
		>
	{/if}
	<p class="sr-only" aria-live="polite">{said}</p>
	{#if path === '' && editable}<p id="move-help" class="sr-only">Press the up or down arrow to move the field.</p>{/if}
</div>

{#if picking}
	<FieldPicker bind:open={picking} {within} {depth} {hasSlug} {components} {insideComponent} onpick={pick} />
{/if}
{#if adding !== null}
	<FieldDialog
		bind:open={() => adding !== null, (on) => !on && (adding = null)}
		source={adding}
		isNew={true}
		{depth}
		siblings={fields}
		{collections}
		{self}
		{components}
		{project}
		{editable}
		at="{pointer}/{fields.length}"
		onapply={(field) => {
			fields.push(field);
			said = `Added ${field.label}.`;
		}}
	/>
{/if}
{#if editing !== null && fields[editing] !== undefined}
	{@const index = editing}
	{@const field = fields[index] as FieldDefinition}
	<FieldDialog
		bind:open={() => editing !== null, (on) => !on && (editing = null)}
		source={field}
		isNew={!saved.has(placeOf(field.id))}
		isTitle={depth === 0 && field.id === titleField}
		{depth}
		siblings={fields.filter((_, other) => other !== index)}
		{collections}
		{self}
		{components}
		{project}
		{editable}
		at="{pointer}/{index}"
		{refusal}
		onapply={(changed) => {
			fields[index] = changed;
		}}
	/>
{/if}
