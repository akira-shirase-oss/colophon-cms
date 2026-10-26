<script lang="ts">
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import EntryEditor from '#lib/EntryEditor.svelte';
	import { getContentType, getEntry } from '#lib/api.ts';
	import { ago, entriesQuery } from '#lib/queries.ts';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';

	const project = $derived(page.params.project ?? '');
	const collection = $derived(page.params.collection ?? '');
	const entry = $derived(page.params.entry ?? '');
	const type = createQuery(() => ({ queryKey: ['type', project, collection], queryFn: () => getContentType(project, collection) }));
	// Loaded once per visit: after that the editor owns the content, and saving keeps it current.
	const draft = createQuery(() => ({
		queryKey: ['draft', project, collection, entry],
		queryFn: () => getEntry(project, collection, entry),
		staleTime: Infinity,
		refetchOnWindowFocus: false
	}));
	// What readers get now comes with the collection's list (the newest 100).
	const list = createQuery(() => entriesQuery(project, collection));
	const published = $derived(list.data?.data.find((summary) => summary.id === entry)?.published ?? null);
</script>

<svelte:head><title>{draft.data?.draft.revision.title ?? 'Editing'} · Colophon Studio</title></svelte:head>

{#if draft.isPending || type.isPending}
	<Skeleton class="mb-3 h-10 w-2/3" />
	<Skeleton class="mb-8 h-4 w-48" />
	<Skeleton class="h-64 w-full rounded-xl" />
{:else if draft.isError || type.isError}
	<Problem error={(draft.error ?? type.error) as Error} />
{:else}
	<PageHeader title={draft.data.draft.revision.title || 'Untitled'}>
		{#snippet eyebrow()}{type.data.definition.name}{/snippet}
		{#snippet meta()}
			<span>Saved {ago(draft.data.draft.updated_at)}</span>
			<span aria-hidden="true">·</span>
			<span class="font-mono text-xs">{entry.slice(-12)}</span>
		{/snippet}
	</PageHeader>
	{#key entry}
		<EntryEditor {project} {collection} {entry} type={type.data} initial={draft.data} {published} />
	{/key}
{/if}
