<!--
  The collection builder's form (D-040), shared by New collection and a collection's Change
  fields: its name, the slug the API knows it by (fixed once created), and the fields entries
  carry, built field by field, kinds picked from a dialog in plain words. Saving a change to an
  existing collection keeps every stored entry valid; the server refuses what doesn't.
-->
<script lang="ts">
	import { tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import AlignLeftIcon from '@lucide/svelte/icons/align-left';
	import CalendarIcon from '@lucide/svelte/icons/calendar';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import HashIcon from '@lucide/svelte/icons/hash';
	import ListIcon from '@lucide/svelte/icons/list';
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
		datetime: { name: 'Date and time', icon: CalendarIcon, about: 'A day and a time, like when an event starts.' }
	} as const;
	type Kind = keyof typeof KIND;

	type FieldRow = {
		key: number;
		id: string;
		idTouched: boolean;
		label: string;
		required: boolean;
		kind: Kind;
		open: boolean;
		maxChars: number;
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
		open: true,
		maxChars: 200,
		multiline: false,
		integer: false,
		min: '',
		max: '',
		values: ''
	});
	/** A field of the collection being changed, as a row: its id stays put unless edited by hand,
	 *  and it opens folded — it's complete already. */
	const fromStored = (field: FieldDefinition): FieldRow => ({
		...row(),
		open: false,
		id: field.id,
		idTouched: true,
		label: field.label,
		required: field.required,
		kind: field.type,
		maxChars: field.type === 'text' ? field.max_chars : 200,
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

	let picking = $state(false);
	async function pick(kind: Kind) {
		const field = row();
		field.kind = kind;
		fields.push(field);
		picking = false;
		// "You name it next": the label box of the field just added.
		await tick();
		document.getElementById(`f-label-${field.key}`)?.focus();
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
		<div class="space-y-2">
			{#each fields as field, index (field.key)}
				{@const Meta = KIND[field.kind]}
				<div class="overflow-hidden rounded-md border border-border bg-card">
					<div class="flex items-center gap-1.5 py-1.5 pr-1.5 pl-3">
						<button
							type="button"
							class="flex min-w-0 flex-1 items-center gap-2.5 text-left"
							onclick={() => (field.open = !field.open)}
							aria-expanded={field.open}
						>
							<Meta.icon class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
							<span class="min-w-0 flex-1 truncate text-sm font-medium">{field.label || `Field ${index + 1}`}</span>
							{#if field.id}<code class="hidden font-mono text-xs text-muted-foreground md:block">{field.id}</code>{/if}
							<Badge variant="secondary" class="shrink-0">{Meta.name}</Badge>
							{#if field.required}<Badge variant="outline" class="shrink-0">required</Badge>{/if}
							<ChevronDownIcon class="size-4 shrink-0 text-muted-foreground transition-transform {field.open ? 'rotate-180' : ''}" aria-hidden="true" />
						</button>
						<Button
							variant="ghost"
							size="icon-sm"
							onclick={() => remove(field)}
							disabled={fields.length === 1}
							aria-label="Remove field {index + 1}"><Trash2Icon /></Button
						>
					</div>
					{#if field.open}
						<div class="space-y-3 border-t border-border px-3 py-3">
							<div class="grid gap-3 sm:grid-cols-[minmax(0,1fr)_9rem_11rem]">
								<div class="space-y-1">
									<Label for="f-label-{field.key}">Label</Label>
									<Input
										id="f-label-{field.key}"
										bind:value={field.label}
										oninput={() => {
											if (!field.idTouched) field.id = idOf(field.label);
										}}
										maxlength={100}
										placeholder="Field {index + 1}"
									/>
								</div>
								<div class="space-y-1">
									<Label for="f-id-{field.key}">Id</Label>
									<Input
										id="f-id-{field.key}"
										class="font-mono"
										bind:value={field.id}
										oninput={() => (field.idTouched = true)}
										maxlength={64}
									/>
								</div>
								<div class="space-y-1">
									<Label for="f-kind-{field.key}">Type</Label>
									<NativeSelect.Root id="f-kind-{field.key}" bind:value={field.kind} class="w-full">
										{#each Object.entries(KIND) as [kind, option] (kind)}
											<NativeSelect.Option value={kind}>{option.name}</NativeSelect.Option>
										{/each}
									</NativeSelect.Root>
								</div>
							</div>
							<div class="flex flex-wrap items-center gap-x-5 gap-y-2 text-sm">
								<label class="flex items-center gap-2" for="f-required-{field.key}">
									<Checkbox id="f-required-{field.key}" bind:checked={field.required} /> Required
								</label>
								{#if field.kind === 'text'}
									<label class="flex items-center gap-2" for="f-multiline-{field.key}">
										<Checkbox id="f-multiline-{field.key}" bind:checked={field.multiline} /> Multiple lines
									</label>
									<label class="flex items-center gap-2" for="f-maxchars-{field.key}">
										Up to
										<Input
											id="f-maxchars-{field.key}"
											type="number"
											class="w-24"
											bind:value={field.maxChars}
											min={1}
											max={100000}
										/>
										characters
									</label>
								{:else if field.kind === 'number'}
									<label class="flex items-center gap-2" for="f-integer-{field.key}">
										<Checkbox id="f-integer-{field.key}" bind:checked={field.integer} /> Whole numbers
									</label>
									<Input type="number" class="w-28" placeholder="min" bind:value={field.min} aria-label="Minimum" />
									<Input type="number" class="w-28" placeholder="max" bind:value={field.max} aria-label="Maximum" />
								{:else if field.kind === 'enum'}
									<Textarea
										class="w-full"
										rows={3}
										placeholder="One option per line"
										bind:value={field.values}
										aria-label="Options"
									/>
								{/if}
							</div>
						</div>
					{/if}
				</div>
			{/each}
			<div>
				<Button variant="outline" size="sm" onclick={() => (picking = true)}><PlusIcon /> Add field</Button>
			</div>
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
