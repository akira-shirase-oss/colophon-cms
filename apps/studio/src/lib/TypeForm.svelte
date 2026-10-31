<!--
  The collection builder's form (D-040), shared by New collection and a collection's Change
  fields: its name, the slug the API knows it by (fixed once created), and the fields entries
  carry, listed as rows like a CMS's content-type builder. Adding a field is two steps — pick
  its kind, then name and set it in a dialog with basic and advanced settings, the same dialog a
  field's row opens for editing. Saving a change to an existing collection keeps every stored
  entry valid; the server refuses what doesn't.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import AlignLeftIcon from '@lucide/svelte/icons/align-left';
	import BracesIcon from '@lucide/svelte/icons/braces';
	import CalendarIcon from '@lucide/svelte/icons/calendar';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import GlobeIcon from '@lucide/svelte/icons/globe';
	import HashIcon from '@lucide/svelte/icons/hash';
	import MailIcon from '@lucide/svelte/icons/mail';
	import ListIcon from '@lucide/svelte/icons/list';
	import PaletteIcon from '@lucide/svelte/icons/palette';
	import PenLineIcon from '@lucide/svelte/icons/pen-line';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import ToggleLeftIcon from '@lucide/svelte/icons/toggle-left';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import TypeIcon from '@lucide/svelte/icons/type';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Checkbox } from '@colophon/ui/ui/checkbox';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Input } from '@colophon/ui/ui/input';
	import { Label } from '@colophon/ui/ui/label';
	import * as NativeSelect from '@colophon/ui/ui/native-select';
	import * as Tabs from '@colophon/ui/ui/tabs';
	import { Textarea } from '@colophon/ui/ui/textarea';
	import {
		ApiError,
		createContentType,
		newKey,
		updateContentType,
		type Definition,
		type FieldDefinition
	} from '#lib/api.ts';
	import FormRow from '#lib/shell/FormRow.svelte';

	let {
		project,
		/** The collection being changed; unset when creating. */
		editing = null
	}: {
		project: string;
		/** The collection being changed: its slug, the definition as read, and its version (the
		 *  save's If-Match). Unset when creating. */
		editing?: { collection: string; definition: Definition; version: number } | null;
	} = $props();

	const KIND = {
		text: { name: 'Text', icon: TypeIcon, about: 'A title, a name, a short description.' },
		rich_text: { name: 'Rich text', icon: AlignLeftIcon, about: 'Long writing with headings, lists, links and images.' },
		number: { name: 'Number', icon: HashIcon, about: 'A price, a quantity, a rating.' },
		boolean: { name: 'Yes or no', icon: ToggleLeftIcon, about: 'A switch: yes or no, on or off.' },
		enum: { name: 'Choice', icon: ListIcon, about: 'One option from a list you set, like a category.' },
		datetime: { name: 'Date and time', icon: CalendarIcon, about: 'A day and a time, like when an event starts.' },
		date: { name: 'Date', icon: CalendarIcon, about: 'A day, like a birthday or a launch date.' },
		time: { name: 'Time of day', icon: ClockIcon, about: 'A time of day, like when a shop opens.' },
		email: { name: 'Email address', icon: MailIcon, about: 'An email address, checked as one.' },
		url: { name: 'Web address', icon: GlobeIcon, about: 'A link to a page on the web.' },
		markdown: { name: 'Markdown', icon: PenLineIcon, about: 'Writing in Markdown: headings, lists and links.' },
		color: { name: 'Colour', icon: PaletteIcon, about: 'A colour, like a brand accent, written #1a6fdd.' },
		json: { name: 'JSON', icon: BracesIcon, about: 'Structured data: objects, lists and values.' }
	} as const;
	type Kind = keyof typeof KIND;

	type FieldRow = {
		key: number;
		id: string;
		idTouched: boolean;
		label: string;
		required: boolean;
		kind: Kind;
		maxChars: number;
		maxBytes: number;
		multiline: boolean;
		integer: boolean;
		min: string;
		max: string;
		values: string;
	};

	let nextKey = 0;
	const row = (): FieldRow => ({
		key: nextKey++,
		id: '',
		idTouched: false,
		label: '',
		required: false,
		kind: 'text',
		maxChars: 200,
		maxBytes: 65536,
		multiline: false,
		integer: false,
		min: '',
		max: '',
		values: ''
	});
	/** A field of the collection being changed, as a row: its id stays put unless edited by hand. */
	const fromStored = (field: FieldDefinition): FieldRow => ({
		...row(),
		id: field.id,
		idTouched: true,
		label: field.label,
		required: field.required,
		kind: field.type,
		maxChars: field.type === 'text' || field.type === 'markdown' ? field.max_chars : 200,
		maxBytes: field.type === 'json' ? field.max_bytes : 65536,
		multiline: field.type === 'text' ? field.multiline : false,
		integer: field.type === 'number' ? field.integer : false,
		min: field.type === 'number' && field.min !== undefined ? String(field.min) : '',
		max: field.type === 'number' && field.max !== undefined ? String(field.max) : '',
		values: field.type === 'enum' ? field.values.join('\n') : ''
	});

	// The component mounts once the collection is read: the props it starts with are the ones
	// it keeps.
	// svelte-ignore state_referenced_locally
	let name = $state(editing?.definition.name ?? '');
	// svelte-ignore state_referenced_locally
	let slug = $state(editing?.collection ?? '');
	// svelte-ignore state_referenced_locally
	let slugTouched = $state(editing !== null);
	// svelte-ignore state_referenced_locally
	let fields = $state<FieldRow[]>(
		editing?.definition.fields.map(fromStored) ?? [{ ...row(), id: 'title', label: 'Title', required: true }]
	);
	// svelte-ignore state_referenced_locally
	let titleKey = $state(String(editing?.definition.fields.findIndex((field) => field.id === editing.definition.title_field) ?? 0));
	let error = $state('');
	let busy = $state(false);

	const RESERVED = ['members', 'settings', 'changesets', 'releases', 'types'];
	const SLUG = /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/;
	const FIELD_ID = /^[a-z][a-z0-9_]{0,63}$/;

	const slugOf = (text: string) =>
		text
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, '-')
			.replace(/^-+|-+$/g, '')
			.slice(0, 63);

	const idOf = (text: string) => {
		const id = text
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, '_')
			.replace(/^_+|_+$/g, '')
			.slice(0, 64);
		return /^[a-z]/.test(id) ? id : `f_${id}`;
	};

	// The kind picker, then the field's settings dialog — the same dialog a row opens, a copy of
	// the row when editing (Cancel keeps the field as it was), a fresh one when adding.
	let picking = $state(false);
	let draft = $state<FieldRow | null>(null);
	let draftNew = $state(false);
	let settingsTab = $state<'basic' | 'advanced'>('basic');

	async function pick(kind: Kind) {
		draft = { ...row(), kind };
		draftNew = true;
		picking = false;
		settingsTab = 'basic';
		// "You name it next": the label box.
		await tick();
		document.getElementById('field-label')?.focus();
	}
	function edit(field: FieldRow) {
		draft = { ...field };
		draftNew = false;
		settingsTab = 'basic';
	}
	function finish() {
		if (draft === null) return;
		if (draftNew) fields.push(draft);
		else fields = fields.map((field) => (field.key === draft?.key ? draft : field));
		draft = null;
	}
	function remove(field: FieldRow) {
		fields = fields.filter((each) => each.key !== field.key);
	}

	const eligible = (field: FieldRow) =>
		field.required && field.kind === 'text' && !field.multiline && FIELD_ID.test(field.id);
	const titleChoices = $derived(fields.filter(eligible));
	const titleRow = $derived(fields.find((field) => String(field.key) === titleKey));
	const usable = $derived(titleRow !== undefined && eligible(titleRow));

	const problem = $derived.by(() => {
		if (name.trim() === '') return 'The collection needs a name.';
		if (!SLUG.test(slug)) return 'The slug is lowercase letters, digits and hyphens, starting and ending with a letter or digit.';
		if (RESERVED.includes(slug)) return `"${slug}" is the name of a Studio page; choose another slug.`;
		if (fields.length === 0) return 'A collection needs at least one field.';
		if (fields.some((field) => field.label.trim() === '')) return 'Every field needs a label.';
		if (fields.some((field) => !FIELD_ID.test(field.id)))
			return 'A field id is lowercase letters, digits and underscores, starting with a letter.';
		if (new Set(fields.map((field) => field.id)).size !== fields.length) return 'Two fields share an id.';
		if (
			fields.some(
				(field) =>
					field.kind === 'enum' &&
					field.values
						.split('\n')
						.map((option) => option.trim())
						.filter(Boolean).length === 0
			)
		)
			return 'A choice field needs at least one option.';
		if (!usable) return 'The title field must be a required, single-line text field.';
		return '';
	});

	const queryClient = useQueryClient();
	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (problem !== '' || titleRow === undefined) return;
		busy = true;
		error = '';
		const definition: Definition = {
			name: name.trim(),
			title_field: titleRow.id,
			fields: fields.map((field): FieldDefinition => {
				const base = { id: field.id, label: field.label.trim(), required: field.required };
				switch (field.kind) {
					case 'text':
						return { ...base, type: 'text', max_chars: field.maxChars, multiline: field.multiline };
					case 'rich_text':
						return { ...base, type: 'rich_text' };
					case 'number':
						return {
							...base,
							type: 'number',
							integer: field.integer,
							...(field.min.trim() !== '' ? { min: Number(field.min) } : {}),
							...(field.max.trim() !== '' ? { max: Number(field.max) } : {})
						};
					case 'boolean':
						return { ...base, type: 'boolean' };
					case 'enum':
						return {
							...base,
							type: 'enum',
							values: field.values
								.split('\n')
								.map((option) => option.trim())
								.filter((option) => option !== '')
						};
					case 'datetime':
						return { ...base, type: 'datetime' };
					case 'markdown':
						return { ...base, type: 'markdown', max_chars: field.maxChars };
					case 'json':
						return { ...base, type: 'json', max_bytes: field.maxBytes };
					case 'date':
					case 'time':
					case 'email':
					case 'url':
					case 'color':
						return { ...base, type: field.kind };
				}
			})
		};
		try {
			const collection = editing?.collection ?? slug;
			const made =
				editing === null
					? await createContentType(project, slug, definition, newKey())
					: await updateContentType(project, collection, definition, editing.version, newKey());
			await queryClient.invalidateQueries({ queryKey: ['types', project] });
			await queryClient.invalidateQueries({ queryKey: ['type', project, collection] });
			toast.success(editing === null ? `Created ${made.definition.name}` : `Saved ${made.definition.name}`);
			await goto(resolve('/projects/[project]/[collection]', { project, collection }));
		} catch (failure) {
			error = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			busy = false;
		}
	}
</script>

<form class="max-w-3xl" onsubmit={submit} aria-label={editing === null ? 'New collection' : 'Change fields'}>
	<FormRow label="Name" hint="What editors see: Articles, Authors, Products." for="type-name">
		<Input
			id="type-name"
			bind:value={name}
			oninput={() => {
				if (!slugTouched) slug = slugOf(name);
			}}
			maxlength={100}
			required
			placeholder="Articles"
		/>
	</FormRow>
	<FormRow label="Slug" hint="The collection's name in the API and its address. Lowercase, hyphens allowed." for="type-slug">
		<Input
			id="type-slug"
			class="font-mono"
			bind:value={slug}
			oninput={() => (slugTouched = true)}
			maxlength={63}
			required
			readonly={editing !== null}
			placeholder="articles"
		/>
	</FormRow>
	<FormRow label="Fields" hint="At least one. Entries keep whatever they were validated against.">
		<div class="overflow-hidden rounded-md border border-border bg-card">
			{#each fields as field, index (field.key)}
				{@const Meta = KIND[field.kind]}
				<div class="flex items-center gap-1 border-b border-border py-1.5 pr-1.5 pl-3 last:border-b-0">
					<button
						type="button"
						class="flex min-w-0 flex-1 items-center gap-2.5 rounded-sm py-1 text-left outline-none focus-visible:ring-[3px] focus-visible:ring-ring/40"
						onclick={() => edit(field)}
					>
						<Meta.icon class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
						<span class="min-w-0 flex-1 truncate text-sm font-medium">{field.label || `Field ${index + 1}`}</span>
						{#if field.id}<code class="hidden font-mono text-xs text-muted-foreground md:block">{field.id}</code>{/if}
						<span class="shrink-0 text-xs text-muted-foreground">{Meta.name}</span>
						{#if field.required}<Badge variant="outline" class="shrink-0">required</Badge>{/if}
					</button>
					<Button variant="ghost" size="icon-sm" onclick={() => edit(field)} aria-label="Edit field {index + 1}"><PencilIcon /></Button>
					<Button
						variant="ghost"
						size="icon-sm"
						onclick={() => remove(field)}
						disabled={fields.length === 1}
						aria-label="Remove field {index + 1}"><Trash2Icon /></Button
					>
				</div>
			{/each}
		</div>
		<div class="mt-2">
			<Button variant="outline" size="sm" onclick={() => (picking = true)}><PlusIcon /> Add field</Button>
		</div>
	</FormRow>
	<FormRow label="Title field" hint="The field an entry is listed by: a required, single-line text field." for="title-field">
		<NativeSelect.Root id="title-field" bind:value={titleKey} disabled={titleChoices.length === 0} class="max-w-xs">
			{#each titleChoices as field (field.key)}
				<NativeSelect.Option value={String(field.key)}>{field.label || field.id}</NativeSelect.Option>
			{/each}
		</NativeSelect.Root>
		{#if titleChoices.length === 0}
			<p class="mt-2 text-sm text-muted-foreground">Mark a text field required and single-line to use it as the title.</p>
		{/if}
	</FormRow>

	{#if problem !== ''}<p class="text-sm text-muted-foreground" data-testid="form-hint">{problem}</p>{/if}
	{#if error}<p role="alert" class="mt-2 text-sm text-destructive">{error}</p>{/if}
	<div class="mt-6">
		<Button type="submit" disabled={busy || problem !== ''}>{editing === null ? 'Create collection' : 'Save fields'}</Button>
	</div>
</form>

<Dialog.Root bind:open={picking}>
	<Dialog.Content class="sm:max-w-2xl" data-testid="field-picker">
		<Dialog.Header>
			<Dialog.Title>Add a field</Dialog.Title>
			<Dialog.Description>Choose what kind of value it holds. You name it next.</Dialog.Description>
		</Dialog.Header>
		<ul class="grid gap-2 sm:grid-cols-2">
			{#each Object.entries(KIND) as [kind, option] (kind)}
				{@const Icon = option.icon}
				<li>
					<button
						type="button"
						class="flex h-full w-full items-start gap-3 rounded-md border border-border bg-card p-3 text-left transition-colors outline-none hover:border-ring/60 hover:bg-accent focus-visible:border-ring focus-visible:ring-4 focus-visible:ring-ring/25"
						onclick={() => pick(kind as Kind)}
						data-testid="pick-{kind}"
					>
						<span
							class="flex size-9 shrink-0 items-center justify-center rounded-md border border-border bg-background text-foreground"
							aria-hidden="true"><Icon class="size-4" /></span
						>
						<span class="grid min-w-0 gap-0.5">
							<span class="text-sm font-medium">{option.name}</span>
							<span class="text-xs text-muted-foreground">{option.about}</span>
						</span>
					</button>
				</li>
			{/each}
		</ul>
	</Dialog.Content>
</Dialog.Root>

<Dialog.Root open={draft !== null} onOpenChange={(open) => { if (!open) draft = null; }}>
	<Dialog.Content class="sm:max-w-xl" data-testid="field-settings">
		{#if draft !== null}
			{@const Meta = KIND[draft.kind]}
			<Dialog.Header>
				<Dialog.Title>{draftNew ? 'Add a field' : 'Edit field'} · {Meta.name}</Dialog.Title>
				<Dialog.Description>{Meta.about}</Dialog.Description>
			</Dialog.Header>
			<Tabs.Root bind:value={settingsTab}>
				<Tabs.List class="w-full">
					<Tabs.Trigger value="basic" class="flex-1">Basic settings</Tabs.Trigger>
					<Tabs.Trigger value="advanced" class="flex-1">Advanced settings</Tabs.Trigger>
				</Tabs.List>
				<Tabs.Content value="basic" class="grid gap-4 pt-4">
					<div class="space-y-1.5">
						<Label for="field-label">Name</Label>
						<Input
							id="field-label"
							bind:value={draft.label}
							oninput={() => {
								if (draft && !draft.idTouched) draft.id = idOf(draft.label);
							}}
							maxlength={100}
							placeholder="What editors see, like Price"
						/>
					</div>
					<label class="flex items-center gap-2 text-sm" for="field-required">
						<Checkbox id="field-required" bind:checked={draft.required} /> Required: an entry can't be saved without it
					</label>
				</Tabs.Content>
				<Tabs.Content value="advanced" class="grid gap-4 pt-4">
					<div class="space-y-1.5">
						<Label for="field-id">Id</Label>
						<Input
							id="field-id"
							class="font-mono"
							bind:value={draft.id}
							oninput={() => draft && (draft.idTouched = true)}
							maxlength={64}
						/>
						<p class="text-xs text-muted-foreground">The key in the API's JSON: lowercase letters, digits and underscores.</p>
					</div>
					{#if draft.kind === 'text'}
						<label class="flex items-center gap-2 text-sm" for="field-multiline">
							<Checkbox id="field-multiline" bind:checked={draft.multiline} /> Multiple lines
						</label>
						<label class="flex items-center gap-2 text-sm" for="field-maxchars">
							Up to
							<Input id="field-maxchars" type="number" class="w-24" bind:value={draft.maxChars} min={1} max={100000} />
							characters
						</label>
					{:else if draft.kind === 'number'}
						<label class="flex items-center gap-2 text-sm" for="field-integer">
							<Checkbox id="field-integer" bind:checked={draft.integer} /> Whole numbers only
						</label>
						<div class="flex items-center gap-3">
							<Input type="number" class="w-28" placeholder="Smallest" bind:value={draft.min} aria-label="Smallest" />
							<Input type="number" class="w-28" placeholder="Largest" bind:value={draft.max} aria-label="Largest" />
						</div>
					{:else if draft.kind === 'enum'}
						<div class="space-y-1.5">
							<Label for="field-values">Options</Label>
							<Textarea id="field-values" rows={4} placeholder="One option per line" bind:value={draft.values} />
						</div>
					{:else if draft.kind === 'markdown'}
						<label class="flex items-center gap-2 text-sm" for="field-mdchars">
							Up to
							<Input id="field-mdchars" type="number" class="w-24" bind:value={draft.maxChars} min={1} max={100000} />
							characters
						</label>
					{:else if draft.kind === 'json'}
						<label class="flex items-center gap-2 text-sm" for="field-jsonbytes">
							Up to
							<Input id="field-jsonbytes" type="number" class="w-28" bind:value={draft.maxBytes} min={1} max={65536} />
							bytes
						</label>
					{/if}
				</Tabs.Content>
			</Tabs.Root>
			<Dialog.Footer>
				<Button variant="outline" type="button" onclick={() => (draft = null)}>Cancel</Button>
				<Button type="button" onclick={finish} data-testid="field-finish">Finish</Button>
			</Dialog.Footer>
		{/if}
	</Dialog.Content>
</Dialog.Root>
