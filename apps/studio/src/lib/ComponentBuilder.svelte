<!--
  A shared component in the builder (D-073): its name and its fields, edited like a group's (one
  structure deep, so nothing unique, no defaults, no conditions, no URL name and no component
  inside). A change reaches every content type that uses it: the server gives each a new version
  in the same transaction, checked against its entries, or refuses the whole change. A refusal
  names the content type in the way and lists its entries with links. Its page keys it on the
  version, so a save starts a fresh copy of the new one.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { ApiError, type Component, type ComponentDefinition, type ContentType, type Details, type FieldDefinition, getEntry, newKey, updateComponent } from './api.ts';
	import { plainError } from './errors.ts';
	import { savedPaths, structureProblems, usedBy } from './fieldDefs.ts';
	import FieldsEditor from './FieldsEditor.svelte';
	import { typesQuery } from './queries.ts';

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
	let refusal = $state<{ message: string; details: Details | null; stale: boolean; type: ContentType | null } | null>(null);

	const changed = $derived(JSON.stringify(draft) !== JSON.stringify(saved));
	const problems = $derived(structureProblems(draft.fields));

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

	/** The content type whose entries are in the way. The refusal names it only in its message, so
	 *  it is found from the types that use the component: the one that holds the first entry. */
	async function typeInTheWay(details: Details | null): Promise<ContentType | null> {
		const candidates = (types.data ?? []).filter((type) => users.some((user) => user.slug === type.slug));
		if (candidates.length === 1) return candidates[0] ?? null;
		const first = details?.entries?.[0]?.entry;
		if (first === undefined) return null;
		for (const type of candidates) {
			try {
				await getEntry(project, type.slug, first);
				return type;
			} catch {
				// Not this one.
			}
		}
		return null;
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
				if (failure.reason === 'needs_migration') {
					const type = await typeInTheWay(failure.details);
					const message = `${type?.definition.name ?? 'A content type that uses it'} has entries that don’t fit this change yet. Change them (they are listed below), or keep the component as it is.`;
					refusal = { message, details: failure.details, stale: false, type };
				} else {
					// Words the Studio has, or the server's own in this developer area.
					const words = failure.reason === 'invalid_component' || failure.reason === 'unknown_component' ? plainError(failure) : failure.message;
					refusal = { message: words, details: failure.details, stale: failure.failure === 'conflict', type: null };
				}
			} else {
				refusal = { message: plainError(failure), details: null, stale: false, type: null };
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
	function problemWords(problem: { reason: string; pointer: string; message: string }, type: ContentType): string {
		const label = labelAt(type.definition.fields, problem.pointer);
		if (label !== null && problem.reason === 'missing_field') return `It has no ${label}, which it would need.`;
		if (label !== null && problem.reason === 'not_applicable') return `It has a ${label}, which wouldn't apply to it.`;
		return problem.message;
	}

	async function reload() {
		await queryClient.invalidateQueries({ queryKey: ['component', project, component.slug] });
	}
</script>

<div class="max-w-4xl space-y-8" data-testid="component-builder">
	<label class="grid max-w-md gap-1.5">
		<span class="text-sm font-medium">Name</span>
		<Input bind:value={draft.name} maxlength={100} disabled={!editable} />
	</label>

	<p class="text-sm text-muted-foreground" data-testid="component-users">
		{#if users.length === 0}
			No content type uses it yet. Add it to one from that content type's fields.
		{:else}
			Used by {users.map((user) => user.name).join(', ')}. Saving a change gives each of them a new version too, or changes nothing if one of
			them can't take it.
		{/if}
	</p>

	<div>
		<FieldsEditor fields={draft.fields} depth={1} {editable} saved={savedAt} collections={types.data ?? []} self="" within={draft.name} {project} refusal={refusal?.details?.pointer ? { pointer: refusal.details.pointer, message: refusal.message } : null} />
	</div>

	{#if refusal}
		<div role="alert" class="rounded-xl border border-destructive/40 bg-destructive/5 px-4 py-3 text-sm" data-testid="component-refusal">
			<p class="font-medium">{refusal.message}</p>
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

	{#if editable}
		{#if changed && problems.length > 0}
			<ul class="space-y-1 text-sm text-destructive" data-testid="builder-problems" aria-label="Before saving">
				{#each problems as problem (problem)}<li>{problem}</li>{/each}
			</ul>
		{/if}
		<div class="flex justify-end gap-2">
			<Button variant="ghost" disabled={!changed || busy} onclick={() => ((draft = structuredClone(saved)), (refusal = null))}>Discard changes</Button>
			<Button disabled={!changed || busy || problems.length > 0 || draft.name.trim() === ''} onclick={save}>Save component</Button>
		</div>
	{/if}
</div>
