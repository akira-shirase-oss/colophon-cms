<!--
  The content type builder (D-054, D-055): rename the type and its fields, add fields, change
  limits, make fields required or optional, and remove them. The server decides what is safe: a
  change that would make stored drafts or publications invalid comes back with the entries in the
  way, listed here with links, so an owner can fix them first or keep the old shape. A field's id
  and type, and the title field, are fixed once saved. Keyed on the version, so a save starts a
  fresh copy of the new one. The fields themselves, structures included (D-066), are edited by
  FieldsEditor.svelte, recursively.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { ApiError, type ContentType, type Definition, type Details, type FieldDefinition, newKey, updateContentType } from './api.ts';
	import { plainError, plainOr } from './errors.ts';
	import { INVALID_DEFAULT } from './words.ts';
	import { savedPaths, structureProblems } from './fieldDefs.ts';
	import FieldsEditor from './FieldsEditor.svelte';
	import { componentsQuery, typesQuery } from './queries.ts';

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

	const changed = $derived(JSON.stringify(draft) !== JSON.stringify(saved));
	// What the server would refuse for sure, said before trying: an empty group or section kind.
	const problems = $derived(structureProblems(draft.fields));

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
							? 'Some entries share a value in this field, so it can’t be kept unique yet. Change them (they are listed below), then save again.'
							: failure.reason === 'needs_migration' && (failure.details?.pointer?.endsWith('/when') || (failure.details?.entries ?? []).some((entry) => entry.problem?.reason === 'not_applicable'))
								? 'Some entries don’t fit this condition yet. Change them (they are listed below), then save again.'
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

<div class="max-w-4xl space-y-8" data-testid="type-builder">
	<label class="grid max-w-md gap-1.5">
		<span class="text-sm font-medium">Name</span>
		<Input bind:value={draft.name} maxlength={100} disabled={!editable} />
	</label>

	<label class="flex max-w-xl items-start gap-3 rounded-xl border border-border bg-card px-4 py-3" data-testid="single-switch">
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

	<div>
		<FieldsEditor
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
	</div>

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

	{#if editable}
		{#if changed && problems.length > 0}
			<ul class="space-y-1 text-sm text-destructive" data-testid="builder-problems" aria-label="Before saving">
				{#each problems as problem (problem)}<li>{problem}</li>{/each}
			</ul>
		{/if}
		<div class="flex justify-end gap-2">
			<Button variant="ghost" disabled={!changed || busy} onclick={() => ((draft = structuredClone(saved)), (refusal = null))}>Discard changes</Button>
			<Button disabled={!changed || busy || problems.length > 0} onclick={save}>Save content type</Button>
		</div>
	{/if}
</div>
