<!--
  One content type in the builder (D-054, D-055): its name and kind in a header (changed in an
  "Edit type" dialog), its fields as a compact list (FieldList.svelte), and one save bar while
  there are unsaved changes. The server decides what is safe: a change that would make stored
  drafts or publications invalid comes back with the entries in the way, listed here with links,
  and said on the field it concerns, so an owner can fix them first or keep the old shape. A
  field's code name and kind, and the title field, are fixed once saved. Keyed on the version, so
  a save starts a fresh copy of the new one.
-->
<script lang="ts">
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import { resolve } from '$app/paths';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Input } from '@colophon/ui/ui/input';
	import { ApiError, type ContentType, type Definition, type Details, type FieldDefinition, newKey, updateContentType } from './api.ts';
	import { plainError, plainOr } from './errors.ts';
	import { savedPaths, structureProblems } from './fieldDefs.ts';
	import FieldList from './FieldList.svelte';
	import { componentsQuery, typesQuery } from './queries.ts';
	import SaveBar from './SaveBar.svelte';
	import { INVALID_DEFAULT } from './words.ts';

	let { project, type, editable }: { project: string; type: ContentType; editable: boolean } = $props();
	const queryClient = useQueryClient();
	// The collections a link field can point to: every content type, this one included.
	const collections = createQuery(() => typesQuery(project));
	const components = createQuery(() => componentsQuery(project));

	// svelte-ignore state_referenced_locally
	const saved = type.definition;
	// svelte-ignore state_referenced_locally
	const etag = `"v${type.version}"`;
	const savedAt = savedPaths(saved.fields);
	let draft = $state<Definition>(structuredClone(saved));
	// One key per edited definition: a retry after a lost answer replays the same save.
	let key = newKey();
	let lastSent = '';
	let busy = $state(false);
	let refusal = $state<{ message: string; details: Details | null; stale: boolean } | null>(null);
	let editingType = $state(false);

	const changed = $derived(JSON.stringify(draft) !== JSON.stringify(saved));
	// What the server would refuse for sure, said before trying: an empty group or section kind.
	const problems = $derived([...(draft.name.trim() === '' ? ['Give the content type a name.'] : []), ...structureProblems(draft.fields)]);

	/** Help text is sent trimmed (the server refuses spaces at either end), and left out when
	 *  empty, at every depth. */
	function trimmed(fields: FieldDefinition[]) {
		for (const field of fields) {
			const help = field.description?.trim() ?? '';
			if (help === '') delete field.description;
			else field.description = help;
			if (field.type === 'object') trimmed(field.fields);
			if (field.type === 'list' && field.of.type === 'object') trimmed(field.of.fields);
			if (field.type === 'blocks') for (const kind of field.blocks) trimmed(kind.fields);
		}
	}

	async function save() {
		busy = true;
		refusal = null;
		const body = JSON.stringify(draft);
		if (body !== lastSent) key = newKey();
		lastSent = body;
		try {
			const sending = $state.snapshot(draft);
			sending.name = sending.name.trim();
			trimmed(sending.fields);
			const result = await updateContentType(project, type.slug, sending, etag, key);
			toast.success(`Saved ${result.type.definition.name}, version ${result.type.version}`);
			queryClient.setQueryData(['type', project, type.slug], result.type);
			await queryClient.invalidateQueries({ queryKey: ['types', project] });
		} catch (failure) {
			if (failure instanceof ApiError && failure.failure !== 'retry') {
				// A refusal the Studio has words for gets them; the developer area keeps the server's otherwise.
				const words =
					failure.reason === 'too_many_entries' ||
					failure.reason === 'invalid_pattern' ||
					failure.reason === 'invalid_unique' ||
					failure.reason === 'invalid_condition'
						? plainError(failure)
						: failure.reason === 'invalid_default'
							? plainOr(failure.message, INVALID_DEFAULT)
							: failure.reason === 'needs_migration' && failure.details?.pointer?.endsWith('/unique')
								? 'Some entries share a value in this field, so it can’t be kept unique yet. Change them (they are listed above), then save again.'
								: failure.reason === 'needs_migration' && (failure.details?.pointer?.endsWith('/when') || (failure.details?.entries ?? []).some((entry) => entry.problem?.reason === 'not_applicable'))
									? 'Some entries don’t fit this condition yet. Change them (they are listed above), then save again.'
									: failure.message;
				refusal = { message: words, details: failure.details, stale: failure.failure === 'conflict' };
			} else {
				toast.error(failure instanceof Error ? failure.message : String(failure));
			}
		} finally {
			busy = false;
		}
	}

	/** A stored entry's problem with the change, in words: a shared unique value names its field. */
	function problemWords(problem: { reason: string; pointer: string; message: string }): string {
		const field = draft.fields.find((item) => `/${item.id}` === problem.pointer);
		const label = field?.label ?? 'value';
		if (problem.reason === 'value_taken') return `It shares its ${label} with another entry.`;
		if (problem.reason === 'not_applicable') return `It has a ${label}, which wouldn't apply to it.`;
		if (problem.reason === 'missing_field') return `It has no ${label}, which it would need.`;
		return problem.message;
	}

	async function reload() {
		await queryClient.invalidateQueries({ queryKey: ['type', project, type.slug] });
	}
</script>

<div class="grid gap-6" data-testid="type-builder">
	<header class="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
		<div class="min-w-0 space-y-1.5">
			<h1 class="text-2xl leading-tight font-semibold tracking-tight text-balance sm:text-[1.75rem]">{draft.name.trim() || saved.name}</h1>
			<p class="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-muted-foreground">
				<Badge variant="secondary">{draft.single === true ? 'Single type: just one entry' : 'Collection: many entries'}</Badge>
				<span>{draft.fields.length} field{draft.fields.length === 1 ? '' : 's'}</span>
				<span>In addresses as <code class="font-mono text-xs">{type.slug}</code></span>
				{#if !editable}<span>Your role can't change it.</span>{/if}
			</p>
		</div>
		<div class="flex shrink-0 items-center gap-2">
			<Button variant="ghost" size="sm" href={resolve('/projects/[project]/[collection]', { project, collection: type.slug })} data-testid="open-entries"
				>{type.definition.single === true ? 'Open it' : 'See its entries'}</Button
			>
			{#if editable}
				<Button variant="outline" size="sm" onclick={() => (editingType = true)} data-testid="edit-type"><PencilIcon data-icon="inline-start" />Edit type</Button>
			{/if}
		</div>
	</header>

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
							<a class="font-medium underline" href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: type.slug, entry: blocker.entry })}
								>{blocker.title}</a
							>
							<Badge variant="secondary">{blocker.state}</Badge>
							{#if blocker.problem}<span class="text-muted-foreground" data-testid="blocker-problem">{problemWords(blocker.problem)}</span>{/if}
						</li>
					{/each}
				</ul>
				{#if (refusal.details.total ?? 0) > refusal.details.entries.length}
					<p class="mt-2 text-muted-foreground">And {(refusal.details.total ?? 0) - refusal.details.entries.length} more.</p>
				{/if}
			{/if}
		</div>
	{/if}

	<FieldList
		fields={draft.fields}
		depth={0}
		{editable}
		saved={savedAt}
		titleField={draft.title_field}
		collections={collections.data ?? []}
		self={type.slug}
		components={components.data ?? []}
		{project}
		refusal={refusal?.details?.pointer ? { pointer: refusal.details.pointer, message: refusal.message } : null}
	/>

	{#if editable}
		<SaveBar {changed} {busy} {problems} what={saved.name} onsave={save} ondiscard={() => ((draft = structuredClone(saved)), (refusal = null))} />
	{/if}
</div>

<Dialog.Root bind:open={editingType}>
	<Dialog.Content class="sm:max-w-md">
		<div class="grid gap-5" data-testid="type-settings">
			<Dialog.Header>
				<Dialog.Title>Edit type</Dialog.Title>
				<Dialog.Description>Its name and whether it holds many entries or just one. Save to keep the changes.</Dialog.Description>
			</Dialog.Header>
			<label class="grid gap-1.5 text-sm">
				<span class="font-medium">Name</span>
				<Input bind:value={draft.name} maxlength={100} disabled={!editable} data-testid="type-name" />
			</label>
			<div class="grid gap-1.5 text-sm">
				<span class="font-medium">Address name</span>
				<p class="flex flex-wrap items-center gap-2">
					<code class="rounded-md bg-muted px-2 py-0.5 font-mono text-xs">{type.slug}</code>
					<span class="text-xs text-muted-foreground">Where its entries live in addresses. It can't change.</span>
				</p>
			</div>
			<label class="flex items-start gap-3 rounded-xl border border-border bg-card px-4 py-3" data-testid="single-switch">
				<input
					type="checkbox"
					class="mt-0.5 size-4 accent-primary"
					checked={draft.single === true}
					onchange={(event) => {
						// Written only when true, as the server writes definitions.
						if (event.currentTarget.checked) draft.single = true;
						else delete draft.single;
					}}
					disabled={!editable}
				/>
				<span>
					<span class="block text-sm font-medium">Just one entry (like site settings or a home page)</span>
					<span class="block text-xs text-muted-foreground">Editors open it straight from the sidebar instead of a list, and nobody can add a second.</span>
				</span>
			</label>
			<Dialog.Footer>
				<Button onclick={() => (editingType = false)}>Done</Button>
			</Dialog.Footer>
		</div>
	</Dialog.Content>
</Dialog.Root>
