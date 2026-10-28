<!--
  One field's settings in the content-type builder, in a dialog with two tabs. Basics: its label,
  whether it's required, help for editors, and what defines its kind (options of a choice, what a
  link points to, what a list holds). Advanced: length limits and number ranges, the allowed form
  (D-069), unique (D-070), a default (D-071), "Show only when" (D-072), and the code name, which can
  be set only before the field is first saved. It edits a copy: Add field or Done puts the copy
  in the content type's unsaved changes, Cancel (or Escape) leaves them as they were. A refusal
  from the last save that concerns one of these settings is shown at it, on its tab.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Input } from '@colophon/ui/ui/input';
	import * as Tabs from '@colophon/ui/ui/tabs';
	import { Textarea } from '@colophon/ui/ui/textarea';
	import { type Component, type ContentType, type FieldDefinition, type FieldType, type ItemKind, MAX_DEPTH } from './api.ts';
	import { toLocal, toUtc } from './content.ts';
	import { blankKind, canBeUnique, canHaveDefault, canNest, CODE_NAME, componentFits, defaultInput, describeField, idFrom, ITEM_TYPES, NAMES, refusalAt } from './fieldDefs.ts';
	import { iconOf } from './fieldIcons.ts';
	import { ALLOWED_FORM, capital, DEFAULT_VALUE, INVALID_UNIQUE, SHOW_WHEN, UNIQUE } from './words.ts';

	let {
		open = $bindable(false),
		source,
		isNew,
		isTitle = false,
		depth,
		siblings,
		collections,
		self,
		components = [],
		project,
		editable,
		at,
		refusal = null,
		onapply
	}: {
		open?: boolean;
		/** The field as it stands; the dialog edits a copy of it. */
		source: FieldDefinition;
		/** Not saved yet: its code name can still be set, and a link's collection chosen. */
		isNew: boolean;
		isTitle?: boolean;
		/** How many structures it sits inside (0: the content type's own fields). */
		depth: number;
		/** The other fields beside it, for code names, "Show only when" and a URL name's source. */
		siblings: FieldDefinition[];
		collections: ContentType[];
		/** This content type's slug ("" in a component). */
		self: string;
		components?: Component[];
		project: string;
		editable: boolean;
		/** The field's place in the definition, as a JSON Pointer ("/fields/2"). */
		at: string;
		refusal?: { pointer: string; message: string } | null;
		onapply: (field: FieldDefinition) => void;
	} = $props();

	// svelte-ignore state_referenced_locally
	let field = $state($state.snapshot(source) as FieldDefinition);
	// A new field's code name follows its label until it is typed in (or was, before).
	// svelte-ignore state_referenced_locally
	let codeTouched = $state(source.id !== '' && source.id !== idFrom(source.label));
	$effect(() => {
		if (isNew && !codeTouched) field.id = idFrom(field.label);
	});

	const componentOf = (slug: string | undefined) => components.find((item) => item.slug === slug);
	const componentName = (slug: string) => componentOf(slug)?.definition.name ?? slug;
	const nameOf = (slug: string) => collections.find((item) => item.slug === slug)?.definition.name ?? slug;
	const target = $derived(collections.find((item) => item.slug !== self)?.slug ?? self);
	const Icon = $derived(iconOf(field));
	const select =
		'h-9 rounded-xl border border-input bg-card px-3 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40 disabled:opacity-60';

	// Where the last save's refusal lands on this field, if it does: its tab opens first.
	// svelte-ignore state_referenced_locally
	const refused = refusal === null ? null : refusalAt(refusal.pointer, at);
	const here = (option: string) => refusal !== null && refused === option;
	const ADVANCED = /^\/(pattern|unique|default|when|min_chars|max_chars|min|max|of\/(pattern|min_chars|max_chars|min|max))$/;
	// svelte-ignore state_referenced_locally
	let tab = $state(refused !== null && ADVANCED.test(refused) && !(refused === '/max' && field.type !== 'number') ? 'advanced' : 'basics');

	/** What keeps the copy from going into the content type, said plainly; nothing when it can. */
	const problem = $derived.by(() => {
		if (field.label.trim() === '') return 'Give it a label.';
		if (isNew && field.id === '') return 'The label needs a letter to make a code name from.';
		if (isNew && !CODE_NAME.test(field.id)) return 'A code name starts with a lowercase letter and has only lowercase letters, digits and underscores.';
		if (siblings.some((other) => other.id === field.id)) return `Another field here already has the code name ${field.id}.`;
		if (field.type === 'enum' && field.values.length === 0) return 'Give it at least one option.';
		if (field.type === 'list' && field.of.type === 'enum' && field.of.values.length === 0) return 'Give each item at least one option.';
		return '';
	});

	function apply(event: SubmitEvent) {
		event.preventDefault();
		if (problem !== '' || !editable) return;
		const done = $state.snapshot(field) as FieldDefinition;
		done.label = done.label.trim();
		onapply(done);
		open = false;
	}

	const bound = (text: string): number | undefined => (text.trim() === '' || !Number.isFinite(Number(text)) ? undefined : Number(text));
	function setMinimum(def: { min_chars?: number }, text: string) {
		const value = bound(text);
		if (value === undefined || value < 1) delete def.min_chars;
		else def.min_chars = Math.trunc(value);
	}
	/** An allowed form (D-069): written only when there is one. */
	function setPattern(def: { pattern?: string }, text: string) {
		if (text === '') delete def.pattern;
		else def.pattern = text;
	}
	function setDescription(text: string) {
		if (text.trim() === '') delete field.description;
		else field.description = text;
	}
	function setUnique(on: boolean) {
		if (on) field.unique = true;
		else delete field.unique;
	}
	function setDefault(value: unknown) {
		if (value === undefined || value === '') delete field.default;
		else field.default = value;
	}
	const numberDefault = (text: string) => (text.trim() === '' ? undefined : Number.isFinite(Number(text)) ? Number(text) : text);
	const swatch = (value: unknown) => (typeof value === 'string' && /^#[0-9a-f]{6}$/i.test(value) ? value.toLowerCase() : '#000000');
	function setMany(def: { many: boolean; max?: number }, many: boolean) {
		def.many = many;
		if (many) def.max = def.max ?? 10;
		else delete def.max;
	}
	/** What each item of a list is: a new kind replaces the old one. */
	function setItem(type: string) {
		if (field.type !== 'list') return;
		const shared = type.startsWith('component:') ? componentOf(type.slice('component:'.length)) : undefined;
		field.of = shared
			? ({ type: 'object', component: shared.slug, fields: structuredClone($state.snapshot(shared.definition.fields)) } as ItemKind)
			: (blankKind(type as FieldType, target) as ItemKind);
	}
	/** A new component field's component: its fields come with it (the server copies them in). */
	function setComponent(slug: string) {
		const shared = componentOf(slug);
		if (field.type !== 'object' || shared === undefined) return;
		field.component = shared.slug;
		field.fields = structuredClone($state.snapshot(shared.definition.fields));
	}

	// "Show only when" (D-072): a top-level yes-or-no or choice field beside it that always shows.
	const valuesOf = (other: FieldDefinition | undefined): unknown[] =>
		other?.type === 'boolean' ? [true, false] : other?.type === 'enum' ? other.values : [];
	const said = (value: unknown) => (value === true ? 'Yes' : value === false ? 'No' : String(value));
	const choices = $derived(siblings.filter((other) => (other.type === 'boolean' || other.type === 'enum') && other.when === undefined));
	const dependedOn = $derived(siblings.filter((other) => other.when?.field === field.id).map((other) => other.label));
	const controller = $derived(siblings.find((other) => other.id === field.when?.field));
	function setWhen(depends: string) {
		if (depends === '') {
			delete field.when;
			return;
		}
		const first = valuesOf(siblings.find((other) => other.id === depends))[0];
		field.when = { field: depends, is: first === undefined ? [] : [first] };
	}
	function tickWhen(value: unknown, on: boolean) {
		if (field.when === undefined) return;
		const when = field.when;
		const order = valuesOf(siblings.find((other) => other.id === when.field));
		field.when = { field: when.field, is: order.filter((one) => (one === value ? on : when.is.includes(one))) };
	}
	const singleLineText = $derived(siblings.filter((other) => other.type === 'text' && !other.multiline));
	const EXAMPLES = [
		{ form: '[A-Z]{3}-[0-9]{4}', means: 'three capitals, a dash, four digits: ABC-1234' },
		{ form: '[0-9]{5}', means: 'exactly five digits: 90210' },
		{ form: '[a-z0-9-]+', means: 'lowercase letters, digits and dashes: summer-sale-2' }
	];
	const showsAdvanced = $derived(depth === 0 || field.type === 'text' || field.type === 'markdown' || field.type === 'number' || field.type === 'list' || isNew);
</script>

{#snippet basics(def: FieldDefinition | ItemKind, place: string, isItem: boolean)}
	{#if def.type === 'text'}
		<label class="flex items-center gap-2 text-sm">
			<input type="checkbox" class="size-4 accent-primary" bind:checked={def.multiline} disabled={!editable || isTitle} />
			Several lines (a paragraph, not one line)
		</label>
	{:else if def.type === 'number'}
		<label class="flex items-center gap-2 text-sm">
			<input type="checkbox" class="size-4 accent-primary" bind:checked={def.integer} disabled={!editable} />
			Whole numbers only
		</label>
	{:else if def.type === 'enum'}
		<label class="grid gap-1.5 text-sm">
			<span class="font-medium">Options, one per line</span>
			<Textarea
				rows={Math.min(Math.max(def.values.length, 3), 8)}
				value={def.values.join('\n')}
				oninput={(event) => (def.values = event.currentTarget.value.split('\n').map((line) => line.trim()).filter((line) => line !== ''))}
				disabled={!editable}
			/>
		</label>
	{:else if def.type === 'slug'}
		<label class="grid gap-1.5 text-sm">
			<span class="font-medium">Suggested from</span>
			<select class={select} value={def.source ?? ''} onchange={(event) => ((def as { source?: string }).source = event.currentTarget.value === '' ? undefined : event.currentTarget.value)} disabled={!editable}>
				<option value="">Nothing</option>
				{#each singleLineText as other (other.id)}<option value={other.id}>{other.label}</option>{/each}
			</select>
			<span class="text-xs text-muted-foreground">Editors get a button that makes the URL name from it, like lemon-tart from Lemon tart.</span>
		</label>
	{:else if def.type === 'reference'}
		<div class="grid gap-3 text-sm" data-testid="reference-options">
			<label class="grid gap-1.5">
				<span class="font-medium">Links to</span>
				<select class={select} value={def.to} onchange={(event) => ((def as { to: string }).to = event.currentTarget.value)} disabled={!editable || !isNew}>
					{#each collections as item (item.slug)}
						<option value={item.slug}>{item.definition.name}{item.slug === self ? ' (this one)' : ''}</option>
					{:else}
						<option value={def.to}>{nameOf(def.to)}</option>
					{/each}
				</select>
				{#if !isNew}<span class="text-xs text-muted-foreground">It keeps pointing at {nameOf(def.to)}: what it holds are {nameOf(def.to)} entries.</span>{/if}
			</label>
			{#if !isItem}
				<fieldset class="flex flex-wrap items-center gap-x-4 gap-y-2">
					<legend class="sr-only">How many</legend>
					<label class="flex items-center gap-2">
						<input type="radio" class="size-4 accent-primary" name="many-{place}" checked={!def.many} onchange={() => setMany(def as { many: boolean }, false)} disabled={!editable} />
						One entry
					</label>
					<label class="flex items-center gap-2">
						<input type="radio" class="size-4 accent-primary" name="many-{place}" checked={def.many} onchange={() => setMany(def as { many: boolean }, true)} disabled={!editable} />
						A list of up to
					</label>
					{#if def.many}
						<Input class="h-8 w-20 tabular" type="number" min={1} max={100} aria-label="Most entries in {field.label || 'this field'}" bind:value={def.max} disabled={!editable} />
					{/if}
				</fieldset>
			{/if}
		</div>
	{:else if def.type === 'object' && def.component !== undefined}
		<div class="grid gap-1.5 text-sm" data-testid="component-choice">
			{#if isNew && !isItem}
				<label class="grid gap-1.5">
					<span class="font-medium">Component</span>
					<select class={select} value={def.component} onchange={(event) => setComponent(event.currentTarget.value)} disabled={!editable}>
						{#each components.filter((shared) => componentFits(shared, depth)) as shared (shared.slug)}<option value={shared.slug}>{shared.definition.name}</option>{/each}
					</select>
				</label>
			{/if}
			<p class="text-xs text-muted-foreground">
				Its fields ({def.fields.map((child) => child.label).join(', ')}) are shared with every content type that uses it, and are changed on its own page:
				<a class="font-medium text-foreground underline underline-offset-2" href={resolve('/projects/[project]/settings/components/[component]', { project, component: def.component })}
					>open {componentName(def.component)}</a
				>.
			</p>
		</div>
	{:else if def.type === 'object'}
		<p class="text-sm text-muted-foreground">Its fields are listed under it on the content type's page, with a button to add more.</p>
	{:else if def.type === 'list'}
		{@const itemTypes = ITEM_TYPES.filter((type) => type !== 'object' || canNest(depth + 1))}
		<div class="grid gap-3 text-sm">
			<div class="flex flex-wrap items-end gap-4">
				<label class="grid gap-1.5">
					<span class="font-medium">Each item is</span>
					<select
						class={select}
						value={def.of.type === 'object' && def.of.component !== undefined ? `component:${def.of.component}` : def.of.type}
						onchange={(event) => setItem(event.currentTarget.value)}
						disabled={!editable}
						aria-label="What each item of {field.label || 'the list'} is"
					>
						{#each itemTypes as type (type)}<option value={type}>{NAMES[type]}</option>{/each}
						{#each components.filter((shared) => componentFits(shared, depth + 1)) as shared (shared.slug)}
							<option value="component:{shared.slug}">Component: {shared.definition.name}</option>
						{/each}
					</select>
				</label>
				<label class="flex items-center gap-2 pb-1.5">
					At most
					<Input class="h-8 w-20 tabular" type="number" min={1} max={100} aria-label="Most items in {field.label || 'the list'}" bind:value={def.max} disabled={!editable} />
					items
				</label>
			</div>
			{#if !canNest(depth + 1)}<p class="text-xs text-muted-foreground">Items can't be groups of fields this deep: groups nest at most {MAX_DEPTH} deep.</p>{/if}
			{#if def.of.type === 'object' && def.of.component === undefined}
				<p class="text-xs text-muted-foreground">Each item's fields are listed under it on the content type's page.</p>
			{:else}
				<div class="grid gap-3 rounded-xl border border-dashed border-border p-3">
					{@render basics(def.of, `${place}/item`, true)}
				</div>
			{/if}
		</div>
	{:else if def.type === 'blocks'}
		<div class="grid gap-2 text-sm">
			<label class="flex items-center gap-2">
				At most
				<Input class="h-8 w-20 tabular" type="number" min={1} max={100} aria-label="Most sections in {field.label || 'this field'}" bind:value={def.max} disabled={!editable} />
				sections
			</label>
			<p class="text-xs text-muted-foreground">Its kinds of section, and their fields, are listed under it on the content type's page.</p>
		</div>
	{/if}
{/snippet}

{#snippet limits(def: FieldDefinition | ItemKind, rel: string)}
	{#if def.type === 'text'}
		<div class="flex flex-wrap items-center gap-4 text-sm">
			<label class="flex items-center gap-2">
				Up to
				<Input class="h-8 w-28 tabular" type="number" min={1} max={100000} bind:value={def.max_chars} disabled={!editable} />
				characters
			</label>
			<label class="flex items-center gap-2">
				At least
				<Input class="h-8 w-24 tabular" type="number" min={1} placeholder="—" value={def.min_chars ?? ''} oninput={(event) => setMinimum(def as { min_chars?: number }, event.currentTarget.value)} disabled={!editable} />
				characters
			</label>
		</div>
		<div class="grid gap-1.5 text-sm">
			<label class="grid gap-1.5">
				<span class="font-medium">{capital(ALLOWED_FORM)} (optional)</span>
				<Input
					class="h-9 font-mono"
					placeholder={'[A-Z]{3}-[0-9]{4}'}
					maxlength={500}
					autocomplete="off"
					spellcheck="false"
					value={def.pattern ?? ''}
					oninput={(event) => setPattern(def as { pattern?: string }, event.currentTarget.value)}
					aria-invalid={here(`${rel}/pattern`) || undefined}
					disabled={!editable}
					data-testid="field-pattern"
				/>
			</label>
			<p class="text-xs text-muted-foreground">
				Only values in this form are accepted. It's written as a regular expression that the whole value must match; leave it empty to allow any text, and
				say the form in the help for editors.
			</p>
			{#if editable}
				<ul class="flex flex-wrap gap-1.5" aria-label="Examples">
					{#each EXAMPLES as example (example.form)}
						<li>
							<button
								type="button"
								class="rounded-full border border-border bg-card px-2.5 py-1 text-left text-xs outline-none hover:bg-accent focus-visible:ring-4 focus-visible:ring-ring/25"
								onclick={() => setPattern(def as { pattern?: string }, example.form)}
								title="Use this form"><code class="font-mono">{example.form}</code> <span class="text-muted-foreground">{example.means}</span></button
							>
						</li>
					{/each}
				</ul>
			{/if}
			{#if here(`${rel}/pattern`)}<span class="text-xs text-destructive" data-testid="pattern-problem">{refusal?.message}</span>{/if}
		</div>
	{:else if def.type === 'markdown'}
		<label class="flex items-center gap-2 text-sm">
			Up to
			<Input class="h-8 w-28 tabular" type="number" min={1} max={100000} bind:value={def.max_chars} disabled={!editable} />
			characters
		</label>
	{:else if def.type === 'number'}
		<div class="flex flex-wrap items-center gap-4 text-sm">
			<label class="flex items-center gap-2">
				From
				<Input class="h-8 w-28 tabular" type="number" value={def.min ?? ''} oninput={(event) => ((def as { min?: number }).min = bound(event.currentTarget.value))} disabled={!editable} />
			</label>
			<label class="flex items-center gap-2">
				to
				<Input class="h-8 w-28 tabular" type="number" value={def.max ?? ''} oninput={(event) => ((def as { max?: number }).max = bound(event.currentTarget.value))} disabled={!editable} />
			</label>
		</div>
	{/if}
{/snippet}

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-xl" data-testid="field-dialog">
		<form class="grid gap-5" onsubmit={apply} aria-label="Settings of {field.label || 'the new field'}">
			<Dialog.Header>
				<div class="flex items-center gap-3">
					<span class="flex size-9 shrink-0 items-center justify-center rounded-lg border border-border bg-background" aria-hidden="true"><Icon class="size-4" /></span>
					<div class="grid min-w-0 gap-0.5">
						<Dialog.Title>{isNew ? `New ${describeField(field, componentName).toLowerCase()} field` : field.label || 'Field'}</Dialog.Title>
						<Dialog.Description>{isNew ? 'Name it, then add it. You can change these settings later.' : describeField(field, componentName)}</Dialog.Description>
					</div>
				</div>
			</Dialog.Header>

			<Tabs.Root bind:value={tab}>
				<Tabs.List aria-label="Settings">
					<Tabs.Trigger value="basics">Basics</Tabs.Trigger>
					<Tabs.Trigger value="advanced">Advanced</Tabs.Trigger>
				</Tabs.List>

				<Tabs.Content value="basics" class="mt-4 grid gap-4">
					<label class="grid gap-1.5 text-sm">
						<span class="font-medium">Label</span>
						<!-- svelte-ignore a11y_autofocus -->
						<Input bind:value={field.label} maxlength={100} disabled={!editable} autofocus={isNew} data-testid="field-label" />
						<span class="text-xs text-muted-foreground">What editors see above the field.</span>
					</label>
					{@render basics(field, at, false)}
					<label class="flex items-start gap-2 text-sm">
						<input type="checkbox" class="mt-0.5 size-4 accent-primary" bind:checked={field.required} disabled={!editable || isTitle} />
						<span>
							<span class="font-medium">Required</span>
							<span class="block text-xs text-muted-foreground">{isTitle ? 'The field that names each entry always is.' : 'Editors must fill it in.'}</span>
						</span>
					</label>
					<label class="grid gap-1.5 text-sm">
						<span class="font-medium">Help for editors (optional)</span>
						<Input
							maxlength={500}
							placeholder="Shown under the field in the editor"
							value={field.description ?? ''}
							oninput={(event) => setDescription(event.currentTarget.value)}
							disabled={!editable}
							data-testid="field-description"
						/>
					</label>
					{#if refused !== null && !ADVANCED.test(refused) && refusal}
						<p class="text-xs text-destructive" data-testid="field-refusal">{refusal.message}</p>
					{/if}
				</Tabs.Content>

				<Tabs.Content value="advanced" class="mt-4 grid gap-5">
					{@render limits(field, '')}
					{#if field.type === 'list' && (field.of.type === 'text' || field.of.type === 'markdown' || field.of.type === 'number')}
						<div class="grid gap-3">
							<p class="text-sm font-medium">Each item</p>
							{@render limits(field.of, '/of')}
						</div>
					{/if}

					{#if canBeUnique(field, depth) || field.unique === true}
						<div class="grid gap-1 text-sm">
							<label class="flex items-center gap-2">
								<input type="checkbox" class="size-4 accent-primary" checked={field.unique === true} onchange={(event) => setUnique(event.currentTarget.checked)} disabled={!editable} data-testid="field-unique" />
								<span class="font-medium">{UNIQUE}</span>
							</label>
							{#if field.unique === true && !canBeUnique(field, depth)}
								<span class="text-xs text-destructive">{INVALID_UNIQUE} Turn this off, or make it one line.</span>
							{:else}
								<span class="text-xs text-muted-foreground">Each value belongs to one entry, among drafts and among what readers get.</span>
							{/if}
							{#if here('/unique')}<span class="text-xs text-destructive" data-testid="unique-problem">{refusal?.message}</span>{/if}
						</div>
					{/if}

					{#if canHaveDefault(field, depth) || field.default !== undefined}
						{@const kind = defaultInput(field)}
						{@const wrong = here('/default')}
						<div class="grid gap-1.5 text-sm" data-testid="field-default">
							<label class="grid gap-1.5">
								<span class="font-medium">{DEFAULT_VALUE}</span>
								{#if kind === 'yes-no'}
									<select class={select} value={field.default === true ? 'yes' : field.default === false ? 'no' : ''} onchange={(event) => setDefault({ yes: true, no: false }[event.currentTarget.value as 'yes' | 'no'])} disabled={!editable} aria-invalid={wrong || undefined}>
										<option value="">No default</option>
										<option value="yes">Yes</option>
										<option value="no">No</option>
									</select>
								{:else if kind === 'choice' && field.type === 'enum'}
									<select class={select} value={typeof field.default === 'string' ? field.default : ''} onchange={(event) => setDefault(event.currentTarget.value)} disabled={!editable} aria-invalid={wrong || undefined}>
										<option value="">No default</option>
										{#each field.values as option (option)}<option value={option}>{option}</option>{/each}
									</select>
								{:else if kind === 'textarea'}
									<Textarea rows={2} value={typeof field.default === 'string' ? field.default : ''} oninput={(event) => setDefault(event.currentTarget.value)} disabled={!editable} aria-invalid={wrong || undefined} />
								{:else if kind === 'number'}
									<Input class="h-9 w-40 tabular" type="number" step="any" value={field.default === undefined ? '' : String(field.default)} oninput={(event) => setDefault(numberDefault(event.currentTarget.value))} disabled={!editable} aria-invalid={wrong || undefined} />
								{:else if kind === 'datetime-local'}
									<Input class="h-9 max-w-60" type="datetime-local" step="1" value={typeof field.default === 'string' ? toLocal(field.default) : ''} oninput={(event) => setDefault(event.currentTarget.value === '' ? undefined : toUtc(event.currentTarget.value))} disabled={!editable} aria-invalid={wrong || undefined} />
								{:else if kind === 'color'}
									<Input class="h-9 max-w-40 font-mono" placeholder="#1a2b3c" maxlength={7} value={typeof field.default === 'string' ? field.default : ''} oninput={(event) => setDefault(event.currentTarget.value.trim().toLowerCase())} disabled={!editable} aria-invalid={wrong || undefined} />
								{:else}
									<Input class="h-9" type={kind ?? 'text'} value={typeof field.default === 'string' ? field.default : ''} oninput={(event) => setDefault(event.currentTarget.value)} disabled={!editable} aria-invalid={wrong || undefined} />
								{/if}
							</label>
							{#if kind === 'color'}
								<input type="color" class="h-8 w-12 rounded-md border border-input bg-card p-1" aria-label="Pick the default {field.label}" value={swatch(field.default)} oninput={(event) => setDefault(event.currentTarget.value.toLowerCase())} disabled={!editable} />
							{/if}
							{#if !canHaveDefault(field, depth)}
								<span class="text-xs text-destructive">A field that must be unique can't have a default: two new entries would share it. Clear it.</span>
							{:else}
								<span class="text-xs text-muted-foreground">New entries start with this; changing it never touches entries already written.</span>
							{/if}
							{#if wrong}<span class="text-xs text-destructive" data-testid="default-problem">{refusal?.message}</span>{/if}
						</div>
					{/if}

					{#if depth === 0 && !isTitle}
						<div class="grid gap-1.5 text-sm" data-testid="field-when">
							{#if dependedOn.length > 0 && field.when === undefined}
								<span class="text-xs text-muted-foreground">{dependedOn.join(', ')} {dependedOn.length === 1 ? 'shows' : 'show'} only for some of its values, so this one always shows.</span>
							{:else}
								<label class="grid gap-1.5">
									<span class="font-medium">{SHOW_WHEN}</span>
									<select class={select} value={field.when?.field ?? ''} onchange={(event) => setWhen(event.currentTarget.value)} disabled={!editable}>
										<option value="">Always shown</option>
										{#each choices as other (other.id)}<option value={other.id}>{other.label} is…</option>{/each}
										{#if field.when !== undefined && !choices.some((other) => other.id === field.when?.field)}
											<option value={field.when.field}>{controller?.label ?? field.when.field} is…</option>
										{/if}
									</select>
								</label>
								{#if field.when !== undefined}
									<div class="flex flex-wrap gap-x-4 gap-y-1" role="group" aria-label="Values of {controller?.label ?? field.when.field} that show {field.label}">
										{#each valuesOf(controller) as value (String(value))}
											<label class="flex items-center gap-1.5">
												<input type="checkbox" class="size-4 accent-primary" checked={field.when.is.includes(value)} onchange={(event) => tickWhen(value, event.currentTarget.checked)} disabled={!editable} />
												{said(value)}
											</label>
										{/each}
									</div>
									{#if field.when.is.length === 0}<span class="text-xs text-destructive">Tick at least one value, or choose Always shown.</span>{/if}
									<span class="text-xs text-muted-foreground">While hidden, it's left out of the entry and isn't required.</span>
								{:else if choices.length === 0}
									<span class="text-xs text-muted-foreground">Add a yes-or-no or choice field to show this one only for some of its values.</span>
								{/if}
							{/if}
							{#if here('/when')}<span class="text-xs text-destructive" data-testid="when-problem">{refusal?.message}</span>{/if}
						</div>
					{/if}

					<div class="grid gap-1.5 text-sm">
						{#if isNew && editable}
							<label class="grid gap-1.5">
								<span class="font-medium">Code name</span>
								<Input
									class="h-9 font-mono"
									maxlength={64}
									autocomplete="off"
									spellcheck="false"
									value={field.id}
									oninput={(event) => {
										codeTouched = true;
										field.id = event.currentTarget.value;
									}}
									data-testid="field-code-name"
								/>
							</label>
							<span class="text-xs text-muted-foreground">How the API and your site's code refer to it. Made from the label; it can't change once saved.</span>
						{:else}
							<span class="font-medium">Code name</span>
							<p class="flex flex-wrap items-center gap-2">
								<code class="rounded-md bg-muted px-2 py-0.5 font-mono text-xs" data-testid="field-code-name">{field.id}</code>
								<span class="text-xs text-muted-foreground">How the API and your site's code refer to it. It can't change once saved.</span>
							</p>
						{/if}
					</div>
					{#if !showsAdvanced}<p class="text-xs text-muted-foreground">Nothing else to set for this kind of field.</p>{/if}
				</Tabs.Content>
			</Tabs.Root>

			<div class="grid gap-3">
				{#if problem !== '' && editable && field.label.trim() !== ''}<p class="text-sm text-destructive" data-testid="field-dialog-problem">{problem}</p>{/if}
				<Dialog.Footer>
					{#if editable}
						<Button type="button" variant="outline" onclick={() => (open = false)}>Cancel</Button>
						<Button type="submit" disabled={problem !== ''} data-testid="field-apply">{isNew ? 'Add field' : 'Done'}</Button>
					{:else}
						<Button type="button" variant="outline" onclick={() => (open = false)}>Close</Button>
					{/if}
				</Dialog.Footer>
			</div>
		</form>
	</Dialog.Content>
</Dialog.Root>
