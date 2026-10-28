<!--
  A shared component in the builder (D-073): its name (in an "Edit component" dialog) and its
  fields as a compact list (FieldList.svelte), edited like a group's (one structure deep, so
  nothing unique, no defaults, no conditions, no URL name and no component inside), with one save
  bar while there are unsaved changes. A change reaches every content type that uses it: the server gives each a new version
  in the same transaction, checked against its entries, or refuses the whole change. A refusal
  names the content type in the way (`details.content_type`) and lists its entries with links.
  Its page keys it on the version, so a save starts a fresh copy of the new one.
-->
<script lang="ts">
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import { resolve } from '$app/paths';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Input } from '@colophon/ui/ui/input';
	import { ApiError, type Component, type ComponentDefinition, type Details, type FieldDefinition, newKey, updateComponent } from './api.ts';
	import { plainError } from './errors.ts';
	import { savedPaths, structureProblems, usedBy } from './fieldDefs.ts';
	import FieldList from './FieldList.svelte';
	import { typesQuery } from './queries.ts';
	import SaveBar from './SaveBar.svelte';

	let {
		project,
		component,
		etag,
		editable,
		onsaved
	}: { project: string; component: Component; etag: string; editable: boolean; onsaved: (saved: Component) => void } = $props();
	const queryClient = useQueryClient();
	const types = createQuery(() => typesQuery(project));
	const users = $derived(usedBy(types.data ?? [], component.slug));

	// svelte-ignore state_referenced_locally
	const saved = component.definition;
	const savedAt = savedPaths(saved.fields);
	let draft = $state<ComponentDefinition>(structuredClone(saved));
	// One key per edited definition: a retry after a lost answer replays the same save.
	let key = newKey();
	let lastSent = '';
	let busy = $state(false);
	/** A content type a refused change names. */
	type InTheWay = { slug: string; name: string; fields: FieldDefinition[] };
	/** A refusal: its words, after the content type's name when it names one (linked), and the
	 *  server's own words beneath when the Studio has none of its own. */
	let refusal = $state<{ message: string; server: string; details: Details | null; stale: boolean; type: InTheWay | null } | null>(null);

	const changed = $derived(JSON.stringify(draft) !== JSON.stringify(saved));
	const problems = $derived([...(draft.name.trim() === '' ? ['Give the component a name.'] : []), ...structureProblems(draft.fields)]);
	let editingName = $state(false);

	/** Help text is sent trimmed and left out when empty, at every depth. */
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

	/** The content type a refused change names (`details.content_type`), with its fields for the
	 *  entries' problems; its name from the content types already read. */
	function typeNamed(slug: string | undefined): InTheWay | null {
		if (slug === undefined) return null;
		const type = types.data?.find((item) => item.slug === slug);
		return { slug, name: type?.definition.name ?? slug, fields: type?.definition.fields ?? [] };
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
			const result = await updateComponent(project, component.slug, sending, etag, key);
			queryClient.setQueryData(['component', project, component.slug], { component: result, etag: `"v${result.version}"` });
			await Promise.all([
				queryClient.invalidateQueries({ queryKey: ['components', project] }),
				queryClient.invalidateQueries({ queryKey: ['types', project] }),
				...(result.content_types ?? []).map((type) => queryClient.invalidateQueries({ queryKey: ['type', project, type.slug] }))
			]);
			onsaved(result);
		} catch (failure) {
			if (failure instanceof ApiError && failure.failure !== 'retry') {
				const type = typeNamed(failure.details?.content_type);
				if (type !== null && failure.reason === 'needs_migration') {
					const message = 'has entries that don’t fit this change yet. Change them (they are listed below), or keep the component as it is.';
					refusal = { message, server: '', details: failure.details, stale: false, type };
				} else if (type !== null) {
					// Its own refusal (too deep, say): the server's words, in this developer area.
					refusal = { message: 'can’t take this change.', server: failure.message, details: failure.details, stale: false, type };
				} else {
					// Words the Studio has, or the server's own in this developer area.
					const words = failure.reason === 'invalid_component' || failure.reason === 'unknown_component' ? plainError(failure) : failure.message;
					refusal = { message: words, server: '', details: failure.details, stale: failure.failure === 'conflict', type: null };
				}
			} else {
				refusal = { message: plainError(failure), server: '', details: null, stale: false, type: null };
			}
		} finally {
			busy = false;
		}
	}

	/** The label of the field at `pointer` in an entry's content ("/seo/description" →
	 *  "Description"), reading this component's fields as changed; null where it can't tell. */
	function labelAt(fields: FieldDefinition[], pointer: string): string | null {
		let level = fields;
		let label: string | null = null;
		for (const part of pointer.split('/').slice(1)) {
			if (/^\d+$/.test(part)) continue;
			const field = level.find((item) => item.id === part);
			if (field === undefined) return null;
			label = field.label;
			const inner = field.type === 'object' ? field : field.type === 'list' && field.of.type === 'object' ? field.of : null;
			level = inner === null ? [] : inner.component === component.slug ? draft.fields : inner.fields;
		}
		return label;
	}

	/** A stored entry's problem with the change, in words, or the server's where the Studio has none. */
	function problemWords(problem: { reason: string; pointer: string; message: string }, type: InTheWay): string {
		const label = labelAt(type.fields, problem.pointer);
		if (label !== null && problem.reason === 'missing_field') return `It has no ${label}, which it would need.`;
		if (label !== null && problem.reason === 'not_applicable') return `It has a ${label}, which wouldn't apply to it.`;
		return problem.message;
	}

	async function reload() {
		await queryClient.invalidateQueries({ queryKey: ['component', project, component.slug] });
	}
</script>

<div class="grid gap-6" data-testid="component-builder">
	<header class="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
		<div class="min-w-0 space-y-1.5">
			<h1 class="text-2xl leading-tight font-semibold tracking-tight text-balance sm:text-[1.75rem]">{draft.name.trim() || saved.name}</h1>
			<p class="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-muted-foreground">
				<Badge variant="secondary">Shared component</Badge>
				<span>{draft.fields.length} field{draft.fields.length === 1 ? '' : 's'}</span>
				<span>In code as <code class="font-mono text-xs">{component.slug}</code></span>
				{#if !editable}<span>Changing it takes a role that may change every content type.</span>{/if}
			</p>
		</div>
		{#if editable}
			<Button variant="outline" size="sm" onclick={() => (editingName = true)} data-testid="edit-component"><PencilIcon data-icon="inline-start" />Edit component</Button>
		{/if}
	</header>

	<p class="text-sm text-muted-foreground" data-testid="component-users">
		{#if users.length === 0}
			No content type uses it yet. Add it to one with Add a field, then Shared component.
		{:else}
			Used by {users.map((user) => user.name).join(', ')}. Saving a change gives each of them a new version too, or changes nothing if one of
			them can't take it.
		{/if}
	</p>

	{#if refusal}
		<div role="alert" class="rounded-xl border border-destructive/40 bg-destructive/5 px-4 py-3 text-sm" data-testid="component-refusal">
			<p class="font-medium" data-testid="refusal-words">
				{#if refusal.type}<a class="underline" href={resolve('/projects/[project]/settings/types/[type]', { project, type: refusal.type.slug })}>{refusal.type.name}</a>{' '}{/if}{refusal.message}
			</p>
			{#if refusal.server}<p class="mt-1 text-muted-foreground">{refusal.server}</p>{/if}
			{#if refusal.stale}
				<Button class="mt-2" variant="outline" size="sm" onclick={reload}>Load the current version</Button>
			{/if}
			{#if refusal.details?.entries?.length}
				{@const type = refusal.type}
				<ul class="mt-3 space-y-2" aria-label="Entries in the way">
					{#each refusal.details.entries as blocker (`${blocker.entry}-${blocker.state}`)}
						<li>
							{#if type}
								<a class="font-medium underline" href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: type.slug, entry: blocker.entry })}
									>{blocker.title}</a
								>
							{:else}
								<span class="font-medium">{blocker.title}</span>
							{/if}
							<Badge variant="secondary">{blocker.state}</Badge>
							{#if blocker.problem && type}<span class="text-muted-foreground" data-testid="blocker-problem">{problemWords(blocker.problem, type)}</span>{/if}
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
		depth={1}
		{editable}
		saved={savedAt}
		collections={types.data ?? []}
		self=""
		insideComponent
		{project}
		refusal={refusal?.details?.pointer ? { pointer: refusal.details.pointer, message: refusal.message } : null}
	/>

	{#if editable}
		<SaveBar {changed} {busy} {problems} what={saved.name} onsave={save} ondiscard={() => ((draft = structuredClone(saved)), (refusal = null))} />
	{/if}
</div>

<Dialog.Root bind:open={editingName}>
	<Dialog.Content class="sm:max-w-md">
		<div class="grid gap-5">
			<Dialog.Header>
				<Dialog.Title>Edit component</Dialog.Title>
				<Dialog.Description>Its name, as content types show it. Save to keep the change.</Dialog.Description>
			</Dialog.Header>
			<label class="grid gap-1.5 text-sm">
				<span class="font-medium">Name</span>
				<Input bind:value={draft.name} maxlength={100} disabled={!editable} data-testid="component-name-input" />
			</label>
			<Dialog.Footer>
				<Button onclick={() => (editingName = false)}>Done</Button>
			</Dialog.Footer>
		</div>
	</Dialog.Content>
</Dialog.Root>
