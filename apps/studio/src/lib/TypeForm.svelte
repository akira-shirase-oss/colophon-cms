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
	import { beforeNavigate, goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import AlignLeftIcon from '@lucide/svelte/icons/align-left';
	import AtSignIcon from '@lucide/svelte/icons/at-sign';
	import BoxIcon from '@lucide/svelte/icons/box';
	import BlocksIcon from '@lucide/svelte/icons/blocks';
	import BracesIcon from '@lucide/svelte/icons/braces';
	import CalendarIcon from '@lucide/svelte/icons/calendar';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import GlobeIcon from '@lucide/svelte/icons/globe';
	import HashIcon from '@lucide/svelte/icons/hash';
	import MailIcon from '@lucide/svelte/icons/mail';
	import ListIcon from '@lucide/svelte/icons/list';
	import PaletteIcon from '@lucide/svelte/icons/palette';
	import ListPlusIcon from '@lucide/svelte/icons/list-plus';
	import LinkIcon from '@lucide/svelte/icons/link';
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
		listContentTypes,
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
		json: { name: 'JSON', icon: BracesIcon, about: 'Structured data: objects, lists and values.' },
		object: { name: 'Object', icon: BoxIcon, about: 'A named group of fields, like an address or a venue.' },
		list: { name: 'List', icon: ListPlusIcon, about: 'Several values of one kind, like tags or links.' },
		slug: { name: 'Slug', icon: AtSignIcon, about: 'A web-safe id for the entry, like my-first-post. Always unique.' },
		reference: { name: 'Reference', icon: LinkIcon, about: 'A link to an entry in another collection, like a post\'s author.' },
		blocks: { name: 'Blocks', icon: BlocksIcon, about: 'Repeatable sections, each one of a few kinds — like text, quote or figure.' }
	} as const;
	type Kind = keyof typeof KIND;
	/** The kinds a list's items can be: scalars with their standard shape. */
	type ItemKind = 'text' | 'number' | 'boolean' | 'datetime' | 'date' | 'time' | 'email' | 'url' | 'color';
	const ITEM_KINDS: ItemKind[] = ['text', 'number', 'boolean', 'datetime', 'date', 'time', 'email', 'url', 'color'];
	/** The kinds a field inside an object can be: no rich text (its editor lives at the top
	 *  level), no object in object, and no list (its options don't fit the compact row). */
	type SubKind = Exclude<Kind, 'object' | 'rich_text' | 'list' | 'slug' | 'reference' | 'blocks'>;
	type SubRow = {
		key: number;
		id: string;
		idTouched: boolean;
		label: string;
		kind: SubKind;
		required: boolean;
		values: string;
	};

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
		defaultStr: string;
		whenField: string;
		whenIn: string;
		unique: boolean;
		slugOf: string;
		refTo: string;
		refMany: boolean;
		refInverse: string;
		blockTypes: BlockRow[];
		subfields: SubRow[];
		listOf: ItemKind;
		minItems: number;
		maxItems: number;
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
		values: '',
		defaultStr: '',
		whenField: '',
		whenIn: '',
		unique: false,
		slugOf: '',
		refTo: '',
		refMany: false,
		refInverse: '',
		blockTypes: [],
		subfields: [],
		listOf: 'text',
		minItems: 0,
		maxItems: 64
	});
	const subRow = (): SubRow => ({ key: nextKey++, id: '', idTouched: false, label: '', kind: 'text', required: false, values: '' });
	/** A named section type of a `blocks` field: an id, a label and its own fields. */
	type BlockRow = { key: number; id: string; label: string; subfields: SubRow[] };
	const blockRow = (): BlockRow => ({ key: nextKey++, id: '', label: '', subfields: [subRow()] });
	const blockDef = (block: BlockRow) => ({ id: block.id, label: block.label.trim() || block.id, fields: block.subfields.map(subDef) });
	const blockStored = (block: { id: string; label: string; fields: FieldDefinition[] }): BlockRow => ({
		key: nextKey++,
		id: block.id,
		label: block.label,
		subfields: block.fields.map(subStored)
	});
	const SUB_KINDS = Object.keys(KIND).filter((kind) => kind !== 'object' && kind !== 'rich_text' && kind !== 'list') as SubKind[];
	/** A field inside an object: its kind's options keep the schema's spelling of defaults. */
	const subDef = (sub: SubRow): FieldDefinition => {
		const base = { id: sub.id, label: sub.label.trim(), required: sub.required };
		switch (sub.kind) {
			case 'text':
				return { ...base, type: 'text', max_chars: 200, multiline: false };
			case 'number':
				return { ...base, type: 'number', integer: false };
			case 'enum':
				return { ...base, type: 'enum', values: sub.values.split('\n').map((v) => v.trim()).filter((v) => v !== '') };
			case 'markdown':
				return { ...base, type: 'markdown', max_chars: 2000 };
			case 'json':
				return { ...base, type: 'json', max_bytes: 65536 };
			default:
				return { ...base, type: sub.kind } as FieldDefinition;
		}
	};
	const subStored = (field: FieldDefinition): SubRow => ({
		...subRow(),
		id: field.id,
		idTouched: true,
		label: field.label,
		kind: (['object', 'rich_text', 'list', 'slug', 'reference', 'blocks'].includes(field.type) ? 'text' : field.type) as SubKind,
		required: field.required,
		values: field.type === 'enum' ? field.values.join('\n') : ''
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
		values: field.type === 'enum' ? field.values.join('\n') : '',
		defaultStr: field.default === undefined ? '' : JSON.stringify(field.default),
		whenField: field.when?.field ?? '',
		whenIn: field.when?.in.join('\n') ?? '',
		unique: field.unique === true,
		slugOf: field.type === 'slug' ? (field.of ?? '') : '',
		refTo: field.type === 'reference' ? field.to : '',
		refMany: field.type === 'reference' ? (field.many ?? false) : false,
		refInverse: field.type === 'reference' ? (field.inverse ?? '') : '',
		blockTypes: field.type === 'blocks' ? field.blocks.map(blockStored) : [],
		subfields: field.type === 'object' ? field.fields.map(subStored) : [],
		listOf: field.type === 'list' ? field.of : 'text',
		minItems: field.type === 'list' ? field.min_items : field.type === 'blocks' ? field.min_blocks : 0,
		maxItems: field.type === 'list' ? field.max_items : field.type === 'blocks' ? field.max_blocks : 64
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
	// svelte-ignore state_referenced_locally
	let single = $state(editing?.definition.kind === 'single');
	let error = $state('');
	let busy = $state(false);
	// A new type starts with its name asked for first, like Strapi's create dialog.
	// svelte-ignore state_referenced_locally
	let intro = $state(editing === null);
	// svelte-ignore state_referenced_locally
	let titleLabel = $state(fields[0]?.label ?? 'Title');

	// Anything that differs from the last saved definition is lost if the page is left.
	const snapshot = () => JSON.stringify({ name, slug, single, titleKey, fields });
	// svelte-ignore state_referenced_locally
	let baseline = $state(snapshot());
	const dirty = $derived(snapshot() !== baseline);
	beforeNavigate((navigation) => {
		if (dirty && !window.confirm('Leave without saving? Your changes to the type will be lost.')) {
			navigation.cancel();
		}
	});

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
		if (
			fields.some((field) => {
				if (field.defaultStr.trim() === '') return false;
				try {
					JSON.parse(field.defaultStr);
					return false;
				} catch {
					return true;
				}
			})
		)
			return 'A default must be JSON: "a string in quotes", true, false, a number, or {…}.';
		if (fields.some((field) => field.whenField !== '' && field.whenIn.trim() === ''))
			return 'A "show only when" rule needs at least one value.';
		if (fields.some((field) => field.required && field.whenField !== ''))
			return 'A required field can\'t hide.';
		if (
			fields.some(
				(field) =>
					field.unique &&
					(field.kind === 'rich_text' ||
						field.kind === 'markdown' ||
						field.kind === 'json' ||
						field.kind === 'object' ||
						field.kind === 'list' ||
						field.kind === 'reference' ||
						field.kind === 'blocks' ||
						(field.kind === 'text' && field.multiline))
			)
		)
			return 'A unique value must be a single, short field.';
		if (fields.some((field) => (field.kind === 'list' || field.kind === 'blocks') && field.minItems > field.maxItems))
			return 'A list needs its "at least" count to be no more than its "at most" count.';
		if (
			fields.some(
				(field) =>
					field.kind === 'blocks' &&
					(field.blockTypes.length === 0 ||
						new Set(field.blockTypes.map((block) => block.id)).size !== field.blockTypes.length ||
						field.blockTypes.some(
							(block) =>
								block.id === '' ||
								block.subfields.length === 0 ||
								new Set(block.subfields.map((sub) => sub.id)).size !== block.subfields.length ||
								block.subfields.some((sub) => sub.id === '')
						))
			)
		)
			return 'Each section type needs a distinct id and at least one field, each with a distinct id.';
		if (
			fields.some(
				(field) =>
					field.kind === 'slug' && field.slugOf !== '' && !fields.some((other) => other.key !== field.key && other.id === field.slugOf)
			)
		)
			return 'A slug can only be derived from one of the other fields.';
		if (fields.some((field) => field.kind === 'reference' && field.refTo === ''))
			return 'A reference needs a collection to point into.';
		if (
			fields.some(
				(field) =>
					field.kind === 'object' &&
					(field.subfields.length === 0 ||
						new Set(field.subfields.map((sub) => sub.id)).size !== field.subfields.length ||
						field.subfields.some(
							(sub) =>
								!FIELD_ID.test(sub.id) ||
								sub.label.trim() === '' ||
								(sub.kind === 'enum' &&
									sub.values.split('\n').filter((value) => value.trim() !== '').length === 0)
						))
			)
		)
			return 'An object field needs fields inside it: each with a name and a distinct id (a choice needs options).';
		return '';
	});

	const queryClient = useQueryClient();
	const collectionsQuery = createQuery(() => ({
		queryKey: ['types', project],
		queryFn: () => listContentTypes(project)
	}));
	async function submit(event: SubmitEvent) {
		event.preventDefault();
		if (problem !== '' || titleRow === undefined) return;
		busy = true;
		error = '';
		const definition: Definition = {
			name: name.trim(),
			title_field: titleRow.id,
			...(single ? { kind: 'single' as const } : {}),
			fields: fields.map((field): FieldDefinition => {
				const base = {
					id: field.id,
					label: field.label.trim(),
					required: field.required,
					...(field.defaultStr.trim() !== '' ? { default: JSON.parse(field.defaultStr) as unknown } : {}),
					...(field.unique ? { unique: true } : {}),
					...(field.whenField !== ''
						? {
								when: {
									field: field.whenField,
									in: field.whenIn
										.split('\n')
										.map((value) => value.trim())
										.filter((value) => value !== '')
								}
							}
						: {})
				};
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
					case 'object':
						return { ...base, type: 'object', fields: field.subfields.map(subDef) };
					case 'list':
						return { ...base, type: 'list', of: field.listOf, min_items: field.minItems, max_items: field.maxItems };
					case 'slug':
						return field.slugOf === '' ? { ...base, type: 'slug' } : { ...base, type: 'slug', of: field.slugOf };
					case 'reference': {
						const inverse = field.refInverse.trim();
						return { ...base, type: 'reference', to: field.refTo, ...(field.refMany ? { many: true as const } : {}), ...(inverse !== '' ? { inverse } : {}) };
					}
					case 'blocks':
						return { ...base, type: 'blocks', blocks: field.blockTypes.map(blockDef), min_blocks: field.minItems, max_blocks: field.maxItems };
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
			baseline = snapshot();
			await goto(resolve('/projects/[project]/[collection]', { project, collection }));
		} catch (failure) {
			error = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			busy = false;
		}
	}
</script>

<svelte:window
	on:beforeunload={(event) => {
		if (dirty) event.preventDefault();
	}}
/>

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
	<FormRow label="Entries" hint="A single collection holds exactly one entry — a homepage, a settings page.">
		<label class="flex items-center gap-2 text-sm" for="type-single">
			<Checkbox id="type-single" bind:checked={single} /> This collection holds one entry
		</label>
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
						{#if field.id}<code class="hidden font-mono text-sm text-muted-foreground md:block">{field.id}</code>{/if}
						<span class="shrink-0 text-sm text-muted-foreground">{Meta.name}</span>
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

<Dialog.Root open={intro} onOpenChange={(open) => (intro = open)}>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>Name the new type</Dialog.Title>
			<Dialog.Description>A type is a kind of entry — a collection holds many of them, a single type holds exactly one.</Dialog.Description>
		</Dialog.Header>
		<div class="space-y-4 py-2">
			<FormRow label="Name" hint="What editors see: Articles, Authors, Products." for="intro-name">
				<Input
					id="intro-name"
					bind:value={name}
					oninput={() => {
						if (!slugTouched) slug = slugOf(name);
					}}
					maxlength={100}
					required
					placeholder="Articles"
				/>
			</FormRow>
			<FormRow label="Title field" hint="Every entry needs one field that names it in lists — 'Title' is a good default." for="intro-title">
				<Input id="intro-title" bind:value={titleLabel} maxlength={100} required />
			</FormRow>
			<label class="flex items-center gap-2 text-sm" for="intro-single">
				<Checkbox id="intro-single" bind:checked={single} /> This collection holds one entry — like a homepage or a settings page
			</label>
		</div>
		<Dialog.Footer>
			<Button variant="outline" onclick={() => goto(resolve('/projects/[project]/types', { project }))}>Cancel</Button>
			<Button
				onclick={() => {
					if (fields[0]) fields[0].label = titleLabel.trim() || 'Title';
					intro = false;
				}}
				disabled={name.trim() === ''}
			>Continue</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>

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
							<span class="text-sm text-muted-foreground">{option.about}</span>
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
						<p class="text-sm text-muted-foreground">The key in the API's JSON: lowercase letters, digits and underscores.</p>
					</div>
					<div class="space-y-1.5">
						<Label for="field-default">Default value</Label>
						<Input
							id="field-default"
							class="font-mono"
							bind:value={draft.defaultStr}
							placeholder='"Guides", true, 42 — JSON, optional'
						/>
						<p class="text-sm text-muted-foreground">What an entry gets when the field is left out.</p>
					</div>
					{#if draft.kind === 'slug'}
						<div class="space-y-1.5">
							<Label for="field-sluof">Derive it from</Label>
							<NativeSelect.Root id="field-sluof" bind:value={draft.slugOf} class="w-full">
								<NativeSelect.Option value="">Written by hand — no source</NativeSelect.Option>
								{#each fields.filter((each) => each.key !== draft?.key) as each (each.key)}
									<NativeSelect.Option value={each.id}>{each.label || each.id}</NativeSelect.Option>
								{/each}
							</NativeSelect.Root>
						</div>
						<p class="text-sm text-muted-foreground">A slug is always unique: no two entries share it.</p>
					{:else if !['rich_text', 'markdown', 'json', 'object', 'list', 'reference', 'blocks'].includes(draft.kind) && !(draft.kind === 'text' && draft.multiline)}
						<label class="flex items-center gap-2 text-sm" for="field-unique">
							<Checkbox id="field-unique" bind:checked={draft.unique} /> Unique: no two entries share this value
						</label>
					{/if}
					{#if fields.length > 1}
						<div class="space-y-1.5">
							<Label for="field-when">Show only when</Label>
							<NativeSelect.Root id="field-when" bind:value={draft.whenField} class="w-full">
								<NativeSelect.Option value="">Always — no rule</NativeSelect.Option>
								{#each fields.filter((each) => each.key !== draft?.key) as each (each.key)}
									<NativeSelect.Option value={each.id}>{each.label || each.id}</NativeSelect.Option>
								{/each}
							</NativeSelect.Root>
							{#if draft.whenField !== ''}
								<Textarea
									rows={2}
									class="font-mono"
									placeholder={'One value per line, e.g.\nNews'}
									bind:value={draft.whenIn}
									aria-label="Values that show the field"
								/>
								<p class="text-sm text-muted-foreground">A yes/no field's values are true and false.</p>
							{/if}
						</div>
					{/if}
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
					{:else if draft.kind === 'reference'}
						<div class="space-y-1.5">
							<Label for="field-refto">Points into</Label>
							<NativeSelect.Root id="field-refto" bind:value={draft.refTo} class="w-full">
								<NativeSelect.Option value="">Choose a collection</NativeSelect.Option>
								{#if editing}<NativeSelect.Option value={editing.collection}>{editing.collection} (this collection)</NativeSelect.Option>{/if}
								{#each collectionsQuery.data ?? [] as other (other.slug)}
									{#if other.slug !== editing?.collection}
										<NativeSelect.Option value={other.slug}>{other.slug}</NativeSelect.Option>
									{/if}
								{/each}
							</NativeSelect.Root>
						</div>
						<label class="flex items-center gap-2 text-sm" for="field-refmany">
							<Checkbox id="field-refmany" bind:checked={draft.refMany} /> Holds more than one — a list of entries
						</label>
						<div class="space-y-1.5">
							<Label for="field-refinverse">Mirrored by</Label>
							<Input
								id="field-refinverse"
								class="w-full font-mono"
								placeholder="a reference field on that collection, like articles"
								bind:value={draft.refInverse}
								maxlength={64}
							/>
							<p class="text-sm text-muted-foreground">
								Optional. Saving this entry updates that field on each linked entry, and the other way around.
							</p>
						</div>
					{:else if draft.kind === 'blocks'}
						<div class="space-y-3">
							<p class="text-sm font-medium">Section types</p>
							{#each draft.blockTypes as block (block.key)}
								<div class="space-y-2 rounded-lg border border-border p-3">
									<div class="grid grid-cols-2 gap-2">
										<Input placeholder="id, like quote" bind:value={block.id} class="font-mono" />
										<Input placeholder="Label" bind:value={block.label} />
									</div>
									{#each block.subfields as sub (sub.key)}
										<div class="flex items-center gap-2">
											<Input placeholder="id" bind:value={sub.id} class="w-24 font-mono" />
											<Input placeholder="Label" bind:value={sub.label} class="min-w-0 flex-1" />
											<NativeSelect.Root class="w-28 shrink-0" bind:value={sub.kind}>
												{#each SUB_KINDS as kind (kind)}<NativeSelect.Option value={kind}>{KIND[kind].name}</NativeSelect.Option>{/each}
											</NativeSelect.Root>
											<label class="flex shrink-0 items-center gap-1 text-sm"><Checkbox bind:checked={sub.required} />Required</label>
											<Button variant="ghost" size="icon-sm" type="button" aria-label="Remove field" onclick={() => (block.subfields = block.subfields.filter((each) => each.key !== sub.key))}><Trash2Icon /></Button>
										</div>
									{/each}
									<div class="flex gap-2">
										<Button variant="ghost" size="sm" type="button" onclick={() => (block.subfields = [...block.subfields, subRow()])}><PlusIcon data-icon="inline-start" />Add a field</Button>
										<Button variant="ghost" size="sm" type="button" onclick={() => { if (draft) draft.blockTypes = draft.blockTypes.filter((each) => each.key !== block.key); }}><Trash2Icon data-icon="inline-start" />Remove section</Button>
									</div>
								</div>
							{/each}
							<Button variant="outline" size="sm" type="button" onclick={() => { if (draft) draft.blockTypes = [...draft.blockTypes, blockRow()]; }}><PlusIcon data-icon="inline-start" />Add a section type</Button>
							<div class="flex items-center gap-4 text-sm">
								<label class="flex items-center gap-2">At least
									<Input type="number" class="w-20" bind:value={draft.minItems} min={0} max={64} />
									sections</label>
								<label class="flex items-center gap-2">At most
									<Input type="number" class="w-20" bind:value={draft.maxItems} min={0} max={64} />
									sections</label>
							</div>
						</div>
					{:else if draft.kind === 'list'}
						<div class="space-y-1.5">
							<Label for="field-listof">Each item is</Label>
							<NativeSelect.Root id="field-listof" class="w-full max-w-72" bind:value={draft.listOf}>
								{#each ITEM_KINDS as kind (kind)}
									<NativeSelect.Option value={kind}>{KIND[kind].name}</NativeSelect.Option>
								{/each}
							</NativeSelect.Root>
						</div>
						<div class="flex items-center gap-3">
							<Input type="number" class="w-28" placeholder="At least" aria-label="At least" min={0} max={64} bind:value={draft.minItems} />
							<Input type="number" class="w-28" placeholder="At most" aria-label="At most" min={0} max={64} bind:value={draft.maxItems} />
						</div>
						<p class="text-sm text-muted-foreground">How many items the list must and may hold (0–64).</p>
					{:else if draft.kind === 'object'}
						<div class="space-y-2">
							<Label>Fields inside {draft.label.trim() || 'the object'}</Label>
							<div class="space-y-2 rounded-lg border border-border p-3">
								{#each draft.subfields as sub, index (sub.key)}
									<div class="flex items-center gap-2">
										<Input
											class="w-32"
											placeholder="Name"
											aria-label="Field name"
											bind:value={sub.label}
											oninput={() => {
												if (!sub.idTouched) sub.id = idOf(sub.label);
											}}
										/>
										<Input
											class="w-24 font-mono"
											placeholder="id"
											aria-label="Field id"
											bind:value={sub.id}
											oninput={() => (sub.idTouched = true)}
										/>
										<NativeSelect.Root class="w-32" bind:value={sub.kind} aria-label="Field kind">
											{#each SUB_KINDS as kind (kind)}
												<NativeSelect.Option value={kind}>{KIND[kind].name}</NativeSelect.Option>
											{/each}
										</NativeSelect.Root>
										<label class="flex items-center gap-1.5 text-sm">
											<Checkbox bind:checked={sub.required} /> Required
										</label>
										<Button
											variant="ghost"
											size="icon"
											type="button"
											aria-label="Remove field"
											onclick={() => draft?.subfields.splice(index, 1)}
										>
											<Trash2Icon class="size-4" />
										</Button>
									</div>
									{#if sub.kind === 'enum'}
										<Textarea
											rows={2}
											class="font-mono"
											placeholder="Options, one per line"
											bind:value={sub.values}
											aria-label="Choice options"
										/>
									{/if}
								{:else}
									<p class="text-sm text-muted-foreground">Nothing inside yet.</p>
								{/each}
								<Button variant="outline" size="sm" type="button" onclick={() => draft?.subfields.push(subRow())}>
									<PlusIcon class="size-4" /> Add a field inside
								</Button>
							</div>
							<p class="text-sm text-muted-foreground">
								A field inside an object can be any kind except rich text or another object.
							</p>
						</div>
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
