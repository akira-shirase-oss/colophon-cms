<!--
  A list of field definitions in the content-type builder, at one depth: each field's label, type,
  options and help text, a way to remove it, and a form to add one. Structures (D-066) hold their
  own: a group of fields and each kind of page section get a FieldsEditor one level deeper, and a
  list chooses what each item is. Only what the server accepts is offered: structures while they
  may still nest (three deep at most), a URL name at the top only, and the builder says why when it
  stops offering something.
-->
<script lang="ts">
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { Textarea } from '@colophon/ui/ui/textarea';
	import { type ContentType, FIELD_TYPES, type FieldDefinition, type FieldType, type ItemKind, MAX_DEPTH } from './api.ts';
	import { blank, blankBlock, blankKind, canNest, idFrom, ITEM_TYPES, NAMES, STRUCTURES } from './fieldDefs.ts';
	import FieldsEditor from './FieldsEditor.svelte';

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
		refusal = null
	}: {
		/** The fields to edit, in place. */
		fields: FieldDefinition[];
		/** How many structures these fields sit inside (0: the content type's own). */
		depth: number;
		editable: boolean;
		/** Paths of fields already saved: a saved link keeps its collection. */
		saved: Set<string>;
		/** These fields' place, for ids and test ids ("hero", "sections/quote"). */
		path?: string;
		titleField?: string;
		/** Every content type, for what a link can point to. */
		collections: ContentType[];
		/** This content type's slug. */
		self: string;
		/** What these fields belong to, for labels ("Hero", "Quote section"). */
		within?: string;
		/** These fields' place in the definition, as a JSON Pointer, so a refusal of one of their
		 *  options (an allowed form the server can't use) is shown at that option. */
		pointer?: string;
		refusal?: { pointer: string; message: string } | null;
	} = $props();

	const select =
		'h-9 rounded-md border border-input bg-card px-3 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40';
	const target = $derived(collections.find((item) => item.slug !== self)?.slug ?? self);
	const nameOf = (slug: string) => collections.find((item) => item.slug === slug)?.definition.name ?? slug;
	const placeOf = (id: string) => (path === '' ? id : `${path}/${id}`);
	const singleLineText = $derived(fields.filter((field) => field.type === 'text' && !field.multiline));
	const hasSlug = $derived(fields.some((field) => field.type === 'slug'));
	/** The types a new field here can have. */
	const offered = $derived(FIELD_TYPES.filter((type) => (type !== 'slug' || depth === 0) && (!STRUCTURES.includes(type) || canNest(depth))));
	const describe = (field: FieldDefinition | ItemKind) =>
		field.type === 'list' ? `List of ${NAMES[field.of.type].toLowerCase()}` : NAMES[field.type];

	let newLabel = $state('');
	let newType = $state<FieldType>('text');
	const newId = $derived(idFrom(newLabel));
	const newProblem = $derived(
		newLabel.trim() === ''
			? ''
			: newId === ''
				? 'The label needs a letter to make an id from.'
				: fields.some((field) => field.id === newId)
					? `A field with the id ${newId} already exists here.`
					: newType === 'slug' && hasSlug
						? 'A content type has at most one URL name field.'
						: ''
	);

	function add(event: SubmitEvent) {
		event.preventDefault();
		if (newProblem !== '' || newId === '') return;
		fields.push(blank(newType, newId, newLabel.trim(), target, titleField));
		newLabel = '';
		newType = 'text';
	}

	const bound = (text: string): number | undefined => (text.trim() === '' || !Number.isFinite(Number(text)) ? undefined : Number(text));

	/** Help text: written only when there is some (D-064). */
	function setDescription(field: FieldDefinition, text: string) {
		if (text.trim() === '') delete field.description;
		else field.description = text;
	}

	/** A text minimum: written only when set. */
	function setMinimum(field: { min_chars?: number }, text: string) {
		const value = bound(text);
		if (value === undefined || value < 1) delete field.min_chars;
		else field.min_chars = Math.trunc(value);
	}

	/** An allowed form (D-069): written only when there is one. */
	function setPattern(field: { pattern?: string }, text: string) {
		if (text === '') delete field.pattern;
		else field.pattern = text;
	}

	/** One linked entry, or a list of up to `max`. */
	function setMany(field: { many: boolean; max?: number }, many: boolean) {
		field.many = many;
		if (many) field.max = field.max ?? 10;
		else delete field.max;
	}

	/** What each item of a list is: a new kind replaces the old one. */
	function setItem(field: FieldDefinition, type: FieldType) {
		if (field.type === 'list') field.of = blankKind(type, target) as ItemKind;
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

{#snippet options(def: FieldDefinition | ItemKind, place: string, isNew: boolean, isTitle: boolean, level: number, owner: string, at: string)}
	{#if def.type === 'text'}
		<div class="flex flex-wrap items-center gap-4 text-sm">
			<label class="flex items-center gap-2">
				Up to
				<Input class="h-8 w-28 tabular" type="number" min={1} max={100000} bind:value={def.max_chars} disabled={!editable} />
				characters
			</label>
			<label class="flex items-center gap-2">
				At least
				<Input class="h-8 w-24 tabular" type="number" min={1} placeholder="—" value={def.min_chars ?? ''} oninput={(event) => setMinimum(def, event.currentTarget.value)} disabled={!editable} />
				characters
			</label>
			<label class="flex items-center gap-2">
				<input type="checkbox" class="size-4 accent-primary" bind:checked={def.multiline} disabled={!editable || isTitle} />
				Several lines
			</label>
			<label class="grid w-full max-w-md basis-full gap-1">
				<span>Allowed form (optional)</span>
				<Input
					class="h-8 font-mono"
					placeholder={'[A-Z]{3}-[0-9]{4}'}
					maxlength={500}
					autocomplete="off"
					spellcheck="false"
					value={def.pattern ?? ''}
					oninput={(event) => setPattern(def, event.currentTarget.value)}
					aria-invalid={refusal?.pointer === `${at}/pattern` ? true : undefined}
					disabled={!editable}
					data-testid="field-pattern"
				/>
				<span class="text-xs text-muted-foreground">A regular expression the whole value must match. Empty allows any text; say the form in the help text.</span>
				{#if refusal?.pointer === `${at}/pattern`}<span class="text-xs text-destructive" data-testid="pattern-problem">{refusal.message}</span>{/if}
			</label>
		</div>
	{:else if def.type === 'number'}
		<div class="flex flex-wrap items-center gap-4 text-sm">
			<label class="flex items-center gap-2">
				<input type="checkbox" class="size-4 accent-primary" bind:checked={def.integer} disabled={!editable} />
				Whole numbers only
			</label>
			<label class="flex items-center gap-2">
				From
				<Input class="h-8 w-28 tabular" type="number" value={def.min ?? ''} oninput={(event) => (def.min = bound(event.currentTarget.value))} disabled={!editable} />
			</label>
			<label class="flex items-center gap-2">
				to
				<Input class="h-8 w-28 tabular" type="number" value={def.max ?? ''} oninput={(event) => (def.max = bound(event.currentTarget.value))} disabled={!editable} />
			</label>
		</div>
	{:else if def.type === 'enum'}
		<label class="grid max-w-md gap-1.5 text-sm">
			Options, one per line
			<Textarea
				rows={Math.min(Math.max(def.values.length, 2), 8)}
				value={def.values.join('\n')}
				oninput={(event) => (def.values = event.currentTarget.value.split('\n').map((line) => line.trim()).filter((line) => line !== ''))}
				disabled={!editable}
			/>
		</label>
	{:else if def.type === 'slug'}
		<label class="flex flex-wrap items-center gap-2 text-sm">
			Suggested from
			<select
				class={select}
				value={def.source ?? ''}
				onchange={(event) => (def.source = event.currentTarget.value === '' ? undefined : event.currentTarget.value)}
				disabled={!editable}
			>
				<option value="">Nothing</option>
				{#each singleLineText as source (source.id)}<option value={source.id}>{source.label}</option>{/each}
			</select>
		</label>
	{:else if def.type === 'markdown'}
		<div class="flex flex-wrap items-center gap-4 text-sm">
			<label class="flex items-center gap-2">
				Up to
				<Input class="h-8 w-28 tabular" type="number" min={1} max={100000} bind:value={def.max_chars} disabled={!editable} />
				characters
			</label>
			<span class="text-xs text-muted-foreground">Written in Markdown; your site turns it into a page.</span>
		</div>
	{:else if def.type === 'reference'}
		<div class="flex flex-wrap items-center gap-4 text-sm" data-testid="reference-options">
			<label class="flex items-center gap-2">
				Links to
				<select
					class={select}
					value={def.to}
					onchange={(event) => (def.to = event.currentTarget.value)}
					disabled={!editable || !isNew}
					title={isNew ? undefined : 'Saved links point to their collection for good'}
				>
					{#each collections as item (item.slug)}
						<option value={item.slug}>{item.definition.name}{item.slug === self ? ' (this one)' : ''}</option>
					{:else}
						<option value={def.to}>{nameOf(def.to)}</option>
					{/each}
				</select>
			</label>
			{#if owner !== 'item'}
				<label class="flex items-center gap-2">
					<input type="radio" class="size-4 accent-primary" name="many-{place}" checked={!def.many} onchange={() => setMany(def, false)} disabled={!editable} />
					One entry
				</label>
				<label class="flex items-center gap-2">
					<input type="radio" class="size-4 accent-primary" name="many-{place}" checked={def.many} onchange={() => setMany(def, true)} disabled={!editable} />
					A list of up to
				</label>
				{#if def.many}
					<Input class="h-8 w-20 tabular" type="number" min={1} max={100} aria-label="Most entries in {owner}" bind:value={def.max} disabled={!editable} />
				{/if}
			{/if}
			{#if !isNew}<span class="text-xs text-muted-foreground">It keeps pointing at {nameOf(def.to)}: what it holds are {nameOf(def.to)} entries.</span>{/if}
		</div>
	{:else if def.type === 'object'}
		<div class="rounded-lg border border-dashed border-border p-3" data-testid="group-{place}">
			<p class="mb-2 text-xs text-muted-foreground">The fields in {owner}:</p>
			<FieldsEditor fields={def.fields} depth={level + 1} {editable} {saved} path={place} {collections} {self} within={owner} pointer="{at}/fields" {refusal} />
		</div>
	{:else if def.type === 'list'}
		{@const itemTypes = ITEM_TYPES.filter((type) => type !== 'object' || canNest(level + 1))}
		<div class="grid gap-3 text-sm">
			<div class="flex flex-wrap items-center gap-4">
				<label class="flex items-center gap-2">
					Each item is
					<select class={select} value={def.of.type} onchange={(event) => setItem(def as FieldDefinition, event.currentTarget.value as FieldType)} disabled={!editable} aria-label="What each item of {owner} is">
						{#each itemTypes as type (type)}<option value={type}>{NAMES[type]}</option>{/each}
					</select>
				</label>
				<label class="flex items-center gap-2">
					At most
					<Input class="h-8 w-20 tabular" type="number" min={1} max={100} aria-label="Most items in {owner}" bind:value={def.max} disabled={!editable} />
					items
				</label>
			</div>
			{#if !canNest(level + 1)}<p class="text-xs text-muted-foreground">Items can't be groups of fields this deep: structures nest at most {MAX_DEPTH} deep.</p>{/if}
			{@render options(def.of, `${place}/item`, isNew, false, level + 1, `${owner} item`, `${at}/of`)}
		</div>
	{:else if def.type === 'blocks'}
		<div class="grid gap-3 text-sm" data-testid="blocks-{place}">
			<label class="flex items-center gap-2">
				At most
				<Input class="h-8 w-20 tabular" type="number" min={1} max={100} aria-label="Most sections in {owner}" bind:value={def.max} disabled={!editable} />
				sections
			</label>
			{#each def.blocks as kind, index (kind.name)}
				<div class="rounded-lg border border-dashed border-border p-3" data-testid="kind-{place}/{kind.name}">
					<div class="mb-2 flex flex-wrap items-center gap-2">
						<Input class="h-8 max-w-56" aria-label="Name of the {kind.label} section" bind:value={kind.label} maxlength={100} disabled={!editable} />
						<code class="font-mono text-xs text-muted-foreground">{kind.name}</code>
						<span class="flex-1"></span>
						{#if editable}
							<Button variant="ghost" size="icon-xs" aria-label="Remove the {kind.label} section" onclick={() => def.type === 'blocks' && def.blocks.splice(index, 1)}><Trash2Icon /></Button>
						{/if}
					</div>
					<FieldsEditor
						fields={kind.fields}
						depth={level + 1}
						{editable}
						{saved}
						path="{place}/{kind.name}"
						{collections}
						{self}
						within="{kind.label} section"
						pointer="{at}/blocks/{index}/fields"
						{refusal}
					/>
				</div>
			{/each}
			{#if editable}
				<form class="flex flex-wrap items-end gap-2" onsubmit={(event) => (event.preventDefault(), addKind(def as FieldDefinition))} aria-label="Add a kind of section to {owner}">
					<label class="grid gap-1">
						<span class="text-xs text-muted-foreground">New kind of section</span>
						<Input class="h-8 w-56" placeholder="Quote, Gallery, Call to action" bind:value={newKind[(def as FieldDefinition).id]} maxlength={100} />
					</label>
					<Button type="submit" variant="outline" size="sm" disabled={(newKind[(def as FieldDefinition).id] ?? '').trim() === ''}>Add kind of section</Button>
				</form>
			{/if}
		</div>
	{/if}
{/snippet}

<ol class="overflow-hidden rounded-xl border border-border bg-card" aria-label={within === '' ? 'Fields' : `Fields in ${within}`}>
	{#each fields as field, index (field.id)}
		{@const place = placeOf(field.id)}
		{@const isTitle = depth === 0 && field.id === titleField}
		{@const isNew = !saved.has(place)}
		<li class="rule-dotted grid gap-3 px-4 py-4 last:border-b-0" data-testid="field-{place}">
			<div class="flex flex-wrap items-center gap-2">
				<Input class="h-8 max-w-64" aria-label="Label of {field.id}" bind:value={field.label} maxlength={100} disabled={!editable} />
				<code class="font-mono text-xs text-muted-foreground">{field.id}</code>
				<Badge variant="secondary">{describe(field)}</Badge>
				{#if isTitle}<Badge variant="outline">Title</Badge>{/if}
				{#if isNew}<Badge variant="outline">New</Badge>{/if}
				<span class="flex-1"></span>
				<label class="flex items-center gap-2 text-sm">
					<input type="checkbox" class="size-4 accent-primary" bind:checked={field.required} disabled={!editable || isTitle} />
					Required
				</label>
				{#if editable && !isTitle}
					<Button variant="ghost" size="icon-xs" aria-label="Remove {field.label}" onclick={() => fields.splice(index, 1)}><Trash2Icon /></Button>
				{/if}
			</div>
			{@render options(field, place, isNew, isTitle, depth, field.label, `${pointer}/${index}`)}
			<label class="grid max-w-xl gap-1 text-sm">
				<span class="text-muted-foreground">Help for editors (optional)</span>
				<Input
					class="h-8"
					maxlength={500}
					placeholder="Shown under the field in the editor"
					value={field.description ?? ''}
					oninput={(event) => setDescription(field, event.currentTarget.value)}
					disabled={!editable}
					data-testid="field-description"
				/>
			</label>
		</li>
	{:else}
		<li class="px-4 py-3 text-sm text-muted-foreground">No fields yet. Add one below.</li>
	{/each}
</ol>

{#if editable}
	<form class="mt-3 flex flex-wrap items-end gap-3" onsubmit={add} aria-label={within === '' ? 'Add a field' : `Add a field to ${within}`}>
		<label class="grid gap-1.5 text-sm">
			<span class="font-medium">{within === '' ? 'New field' : `New field in ${within}`}</span>
			<Input class="w-64" placeholder="Label" bind:value={newLabel} maxlength={100} />
		</label>
		<label class="grid gap-1.5 text-sm">
			<span class="sr-only">{within === '' ? 'Type' : `Type of the new field in ${within}`}</span>
			<select class={select} bind:value={newType}>
				{#each offered as kind (kind)}
					<option value={kind} disabled={kind === 'slug' && hasSlug}>{NAMES[kind]}</option>
				{/each}
			</select>
		</label>
		<Button type="submit" variant="outline" disabled={newLabel.trim() === '' || newProblem !== ''}>Add field</Button>
		{#if newId !== '' && newProblem === ''}<p class="w-full text-xs text-muted-foreground">Its id will be <code class="font-mono">{newId}</code>.</p>{/if}
		{#if newProblem}<p class="w-full text-xs text-destructive">{newProblem}</p>{/if}
		{#if !canNest(depth)}
			<p class="w-full text-xs text-muted-foreground" data-testid="nesting-limit">
				Groups, lists and page sections aren't offered here: structures nest at most {MAX_DEPTH} deep.
			</p>
		{/if}
	</form>
{/if}
