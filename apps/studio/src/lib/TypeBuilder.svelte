<!--
  The content type builder (D-054, D-055): rename the type and its fields, add fields, change
  limits, make fields required or optional, and remove them. The server decides what is safe: a
  change that would make stored drafts or publications invalid comes back with the entries in the
  way, listed here with links, so an owner can fix them first or keep the old shape. A field's id
  and type, and the title field, are fixed once saved. Keyed on the version, so a save starts a
  fresh copy of the new one.
-->
<script lang="ts">
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { resolve } from '$app/paths';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { Textarea } from '@colophon/ui/ui/textarea';
	import {
		ApiError,
		type ContentType,
		type Definition,
		type Details,
		FIELD_TYPES,
		type FieldDefinition,
		type FieldType,
		newKey,
		updateContentType
	} from './api.ts';
	import { typesQuery } from './queries.ts';

	let { project, type, editable }: { project: string; type: ContentType; editable: boolean } = $props();
	const queryClient = useQueryClient();
	// The collections a link field can point to: every content type, this one included.
	const collections = createQuery(() => typesQuery(project));
	const nameOf = (slug: string) => collections.data?.find((item) => item.slug === slug)?.definition.name ?? slug;

	// svelte-ignore state_referenced_locally
	const saved = type.definition;
	// svelte-ignore state_referenced_locally
	const etag = `"v${type.version}"`;
	const savedIds = new Set(saved.fields.map((field) => field.id));
	let draft = $state<Definition>(structuredClone(saved));
	// One key per edited definition: a retry after a lost answer replays the same save.
	let key = newKey();
	let lastSent = '';
	let busy = $state(false);
	let refusal = $state<{ message: string; details: Details | null; stale: boolean } | null>(null);

	const changed = $derived(JSON.stringify(draft) !== JSON.stringify(saved));
	const singleLineText = $derived(draft.fields.filter((field) => field.type === 'text' && !field.multiline));
	const hasSlug = $derived(draft.fields.some((field) => field.type === 'slug'));

	const NAMES: Record<FieldType, string> = {
		text: 'Text',
		rich_text: 'Rich text',
		markdown: 'Markdown text',
		number: 'Number',
		boolean: 'Yes or no',
		enum: 'Choice',
		datetime: 'Date and time',
		date: 'Date',
		time: 'Time of day',
		email: 'Email address',
		url: 'Web address',
		color: 'Colour',
		json: 'Structured data (JSON)',
		reference: 'Link to entries',
		slug: 'URL name'
	};

	/** A field id from a label: lowercase letters, digits and underscores, starting with a letter. */
	function idFrom(label: string): string {
		const id = label
			.normalize('NFKD')
			.replace(/\p{M}+/gu, '')
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, '_')
			.replace(/^[^a-z]+|_+$/g, '');
		return id.slice(0, 64);
	}

	function blank(kind: FieldType, id: string, label: string): FieldDefinition {
		const common = { id, label, required: false };
		switch (kind) {
			case 'text':
				return { ...common, type: 'text', max_chars: 200, multiline: false };
			case 'number':
				return { ...common, type: 'number', integer: false };
			case 'enum':
				return { ...common, type: 'enum', values: ['First option'] };
			case 'slug':
				return { ...common, type: 'slug', source: draft.title_field };
			case 'markdown':
				return { ...common, type: 'markdown', max_chars: 100_000 };
			case 'reference':
				// Another collection if there is one (a story names its author), else this one.
				return { ...common, type: 'reference', to: collections.data?.find((item) => item.slug !== type.slug)?.slug ?? type.slug, many: false };
			case 'rich_text':
			case 'boolean':
			case 'datetime':
			case 'date':
			case 'time':
			case 'email':
			case 'url':
			case 'color':
			case 'json':
				return { ...common, type: kind };
		}
	}

	let newLabel = $state('');
	let newType = $state<FieldType>('text');
	const newId = $derived(idFrom(newLabel));
	const newProblem = $derived(
		newLabel.trim() === ''
			? ''
			: newId === ''
				? 'The label needs a letter to make an id from.'
				: draft.fields.some((field) => field.id === newId)
					? `A field with the id ${newId} already exists.`
					: newType === 'slug' && hasSlug
						? 'A content type has at most one URL name field.'
						: ''
	);

	function add(event: SubmitEvent) {
		event.preventDefault();
		if (newProblem !== '' || newId === '') return;
		draft.fields.push(blank(newType, newId, newLabel.trim()));
		newLabel = '';
		newType = 'text';
	}

	function remove(index: number) {
		draft.fields.splice(index, 1);
	}

	function enumValues(field: FieldDefinition): string {
		return field.type === 'enum' ? field.values.join('\n') : '';
	}

	function setEnumValues(field: FieldDefinition, text: string) {
		if (field.type === 'enum') field.values = text.split('\n').map((line) => line.trim()).filter((line) => line !== '');
	}

	/** One linked entry, or a list of up to `max` (the server's default and limit is 100). */
	function setMany(field: FieldDefinition, many: boolean) {
		if (field.type !== 'reference') return;
		const index = draft.fields.indexOf(field);
		draft.fields[index] = many
			? { id: field.id, label: field.label, required: field.required, ...(field.description ? { description: field.description } : {}), type: 'reference', to: field.to, many: true, max: 10 }
			: { id: field.id, label: field.label, required: field.required, ...(field.description ? { description: field.description } : {}), type: 'reference', to: field.to, many: false };
	}

	/** Help text: written only when there is some (D-064). */
	function setDescription(field: FieldDefinition, text: string) {
		if (text.trim() === '') delete field.description;
		else field.description = text;
	}

	/** A text field's minimum: written only when set. */
	function setMinimum(field: FieldDefinition, text: string) {
		if (field.type !== 'text') return;
		const value = bound(text);
		if (value === undefined || value < 1) delete field.min_chars;
		else field.min_chars = Math.trunc(value);
	}

	function bound(text: string): number | undefined {
		return text.trim() === '' || !Number.isFinite(Number(text)) ? undefined : Number(text);
	}

	async function save() {
		busy = true;
		refusal = null;
		const body = JSON.stringify(draft);
		if (body !== lastSent) key = newKey();
		lastSent = body;
		try {
			// Help text is sent trimmed (the server refuses spaces at either end), and left out when empty.
			const sending = $state.snapshot(draft);
			for (const field of sending.fields) {
				const help = field.description?.trim() ?? '';
				if (help === '') delete field.description;
				else field.description = help;
			}
			const result = await updateContentType(project, type.slug, sending, etag, key);
			toast.success(`Saved ${result.type.definition.name}, version ${result.type.version}`);
			queryClient.setQueryData(['type', project, type.slug], result.type);
			await queryClient.invalidateQueries({ queryKey: ['types', project] });
		} catch (failure) {
			if (failure instanceof ApiError && failure.failure !== 'retry') {
				refusal = { message: failure.message, details: failure.details, stale: failure.failure === 'conflict' };
			} else {
				toast.error(failure instanceof Error ? failure.message : String(failure));
			}
		} finally {
			busy = false;
		}
	}

	async function reload() {
		await queryClient.invalidateQueries({ queryKey: ['type', project, type.slug] });
	}

	const select =
		'h-9 rounded-md border border-input bg-card px-3 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40';
</script>

<div class="max-w-4xl space-y-8" data-testid="type-builder">
	<label class="grid max-w-md gap-1.5">
		<span class="text-sm font-medium">Name</span>
		<Input bind:value={draft.name} maxlength={100} disabled={!editable} />
	</label>

	<ol class="overflow-hidden rounded-xl border border-border bg-card" aria-label="Fields">
		{#each draft.fields as field, index (field.id)}
			{@const isTitle = field.id === draft.title_field}
			{@const isNew = !savedIds.has(field.id)}
			<li class="rule-dotted grid gap-3 px-4 py-4 last:border-b-0" data-testid="field-{field.id}">
				<div class="flex flex-wrap items-center gap-2">
					<Input class="h-8 max-w-64" aria-label="Label of {field.id}" bind:value={field.label} maxlength={100} disabled={!editable} />
					<code class="font-mono text-xs text-muted-foreground">{field.id}</code>
					<Badge variant="secondary">{NAMES[field.type]}</Badge>
					{#if isTitle}<Badge variant="outline">Title</Badge>{/if}
					{#if isNew}<Badge variant="outline">New</Badge>{/if}
					<span class="flex-1"></span>
					<label class="flex items-center gap-2 text-sm">
						<input type="checkbox" class="size-4 accent-primary" bind:checked={field.required} disabled={!editable || isTitle} />
						Required
					</label>
					{#if editable && !isTitle}
						<Button variant="ghost" size="icon-xs" aria-label="Remove {field.label}" onclick={() => remove(index)}><Trash2Icon /></Button>
					{/if}
				</div>
				{#if field.type === 'text'}
					<div class="flex flex-wrap items-center gap-4 text-sm">
						<label class="flex items-center gap-2">
							Up to
							<Input class="h-8 w-28 tabular" type="number" min={1} max={100000} bind:value={field.max_chars} disabled={!editable} />
							characters
						</label>
						<label class="flex items-center gap-2">
							At least
							<Input
								class="h-8 w-24 tabular"
								type="number"
								min={1}
								placeholder="—"
								value={field.min_chars ?? ''}
								oninput={(event) => setMinimum(field, event.currentTarget.value)}
								disabled={!editable}
							/>
							characters
						</label>
						<label class="flex items-center gap-2">
							<input type="checkbox" class="size-4 accent-primary" bind:checked={field.multiline} disabled={!editable || isTitle} />
							Several lines
						</label>
					</div>
				{:else if field.type === 'reference'}
					<div class="flex flex-wrap items-center gap-4 text-sm" data-testid="reference-options">
						<label class="flex items-center gap-2">
							Links to
							<select
								class={select}
								value={field.to}
								onchange={(event) => {
									if (field.type === 'reference') field.to = event.currentTarget.value;
								}}
								disabled={!editable || !isNew}
								title={isNew ? undefined : 'Saved links point to their collection for good'}
							>
								{#each collections.data ?? [] as item (item.slug)}
									<option value={item.slug}>{item.definition.name}{item.slug === type.slug ? ' (this one)' : ''}</option>
								{:else}
									<option value={field.to}>{nameOf(field.to)}</option>
								{/each}
							</select>
						</label>
						<label class="flex items-center gap-2">
							<input type="radio" class="size-4 accent-primary" name="many-{field.id}" checked={!field.many} onchange={() => setMany(field, false)} disabled={!editable} />
							One entry
						</label>
						<label class="flex items-center gap-2">
							<input type="radio" class="size-4 accent-primary" name="many-{field.id}" checked={field.many} onchange={() => setMany(field, true)} disabled={!editable} />
							A list of up to
						</label>
						{#if field.many}
							<Input class="h-8 w-20 tabular" type="number" min={1} max={100} aria-label="Most entries in {field.label}" bind:value={field.max} disabled={!editable} />
						{/if}
						{#if !isNew}<span class="text-xs text-muted-foreground">It keeps pointing at {nameOf(field.to)}: what it holds are {nameOf(field.to)} entries.</span>{/if}
					</div>
				{:else if field.type === 'markdown'}
					<div class="flex flex-wrap items-center gap-4 text-sm">
						<label class="flex items-center gap-2">
							Up to
							<Input class="h-8 w-28 tabular" type="number" min={1} max={100000} bind:value={field.max_chars} disabled={!editable} />
							characters
						</label>
						<span class="text-xs text-muted-foreground">Written in Markdown; your site turns it into a page.</span>
					</div>
				{:else if field.type === 'number'}
					<div class="flex flex-wrap items-center gap-4 text-sm">
						<label class="flex items-center gap-2">
							<input type="checkbox" class="size-4 accent-primary" bind:checked={field.integer} disabled={!editable} />
							Whole numbers only
						</label>
						<label class="flex items-center gap-2">
							From
							<Input
								class="h-8 w-28 tabular"
								type="number"
								value={field.min ?? ''}
								oninput={(event) => (field.min = bound(event.currentTarget.value))}
								disabled={!editable}
							/>
						</label>
						<label class="flex items-center gap-2">
							to
							<Input
								class="h-8 w-28 tabular"
								type="number"
								value={field.max ?? ''}
								oninput={(event) => (field.max = bound(event.currentTarget.value))}
								disabled={!editable}
							/>
						</label>
					</div>
				{:else if field.type === 'enum'}
					<label class="grid max-w-md gap-1.5 text-sm">
						Options, one per line
						<Textarea
							rows={Math.min(Math.max(field.values.length, 2), 8)}
							value={enumValues(field)}
							oninput={(event) => setEnumValues(field, event.currentTarget.value)}
							disabled={!editable}
						/>
					</label>
				{:else if field.type === 'slug'}
					<label class="flex flex-wrap items-center gap-2 text-sm">
						Suggested from
						<select
							class={select}
							value={field.source ?? ''}
							onchange={(event) => (field.source = event.currentTarget.value === '' ? undefined : event.currentTarget.value)}
							disabled={!editable}
						>
							<option value="">Nothing</option>
							{#each singleLineText as source (source.id)}
								<option value={source.id}>{source.label}</option>
							{/each}
						</select>
					</label>
				{/if}
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
		{/each}
	</ol>

	{#if editable}
		<form class="flex flex-wrap items-end gap-3" onsubmit={add} aria-label="Add a field">
			<label class="grid gap-1.5 text-sm">
				<span class="font-medium">New field</span>
				<Input class="w-64" placeholder="Label" bind:value={newLabel} maxlength={100} />
			</label>
			<label class="grid gap-1.5 text-sm">
				<span class="sr-only">Type</span>
				<select class={select} bind:value={newType}>
					{#each FIELD_TYPES as kind (kind)}
						<option value={kind} disabled={kind === 'slug' && hasSlug}>{NAMES[kind]}</option>
					{/each}
				</select>
			</label>
			<Button type="submit" variant="outline" disabled={newLabel.trim() === '' || newProblem !== ''}>Add field</Button>
			{#if newId !== '' && newProblem === ''}<p class="w-full text-xs text-muted-foreground">Its id will be <code class="font-mono">{newId}</code>.</p>{/if}
			{#if newProblem}<p class="w-full text-xs text-destructive">{newProblem}</p>{/if}
		</form>
	{/if}

	{#if refusal}
		<div role="alert" class="rounded-xl border border-destructive/40 bg-destructive/5 px-4 py-3 text-sm" data-testid="type-refusal">
			<p class="font-medium">{refusal.message}</p>
			{#if refusal.stale}
				<Button class="mt-2" variant="outline" size="sm" onclick={reload}>Load the current version</Button>
			{/if}
			{#if refusal.details?.entries?.length}
				<ul class="mt-3 space-y-2" aria-label="Entries in the way">
					{#each refusal.details.entries as blocker (`${blocker.entry}-${blocker.state}`)}
						<li>
							<a
								class="font-medium underline"
								href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: type.slug, entry: blocker.entry })}
								>{blocker.title}</a
							>
							<Badge variant="secondary">{blocker.state}</Badge>
							{#if blocker.problem}<span class="text-muted-foreground">{blocker.problem.message}</span>{/if}
						</li>
					{/each}
				</ul>
				{#if (refusal.details.total ?? 0) > refusal.details.entries.length}
					<p class="mt-2 text-muted-foreground">And {(refusal.details.total ?? 0) - refusal.details.entries.length} more.</p>
				{/if}
			{/if}
		</div>
	{/if}

	{#if editable}
		<div class="flex justify-end gap-2">
			<Button variant="ghost" disabled={!changed || busy} onclick={() => ((draft = structuredClone(saved)), (refusal = null))}>Discard changes</Button>
			<Button disabled={!changed || busy} onclick={save}>Save content type</Button>
		</div>
	{/if}
</div>
