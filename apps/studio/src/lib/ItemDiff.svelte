<!--
  What one changeset item changes for readers: from the live revision to the pinned one, field by
  field. A new entry is all additions; an unpublish takes everything down.
-->
<script lang="ts">
	import { createQuery } from '@tanstack/svelte-query';
	import { type ChangesetItem, getContentType, getRevision } from './api.ts';
	import { compare } from './diff.ts';
	import DiffView from './DiffView.svelte';

	let { project, item }: { project: string; item: ChangesetItem } = $props();

	const type = createQuery(() => ({
		queryKey: ['type', project, item.collection],
		queryFn: () => getContentType(project, item.collection)
	}));
	const revisionQuery = (revision: string | null) => ({
		queryKey: ['revision', project, item.collection, item.entry_id, revision],
		queryFn: () => getRevision(project, item.collection, item.entry_id, revision ?? ''),
		enabled: revision !== null,
		staleTime: Infinity
	});
	const pinned = createQuery(() => revisionQuery(item.revision_id));
	const live = createQuery(() => revisionQuery(item.kind === 'unpublish' ? null : item.published_revision_id));
	// Every query's data is read together, never behind a short-circuit: svelte-query only updates
	// what has been read, so a query first read after its answer arrived kept showing nothing, and
	// "Loading the comparison…" stayed on screen for good (seen on a published request, step 3.7).
	const loaded = $derived({ type: type.data, pinned: pinned.data, live: live.data });
	const needsLive = $derived(item.published_revision_id !== null && item.kind !== 'unpublish');
</script>

{#if type.isError || pinned.isError || live.isError}
	<p class="text-sm text-destructive">Couldn't load this comparison: {(type.error ?? pinned.error ?? live.error)?.message}</p>
{:else if loaded.type === undefined || loaded.pinned === undefined || (needsLive && loaded.live === undefined)}
	<p class="text-sm text-muted-foreground">Loading the comparison…</p>
{:else if item.kind === 'unpublish'}
	<p class="mb-3 text-sm text-attention-ink">Readers will get “gone” (HTTP 410) for this entry instead of:</p>
	<DiffView changes={compare(loaded.type.definition, loaded.pinned.content, null)} before="live" after="nothing" />
{:else}
	{#if item.published_revision_id === null}<p class="mb-3 text-sm text-muted-foreground">New for readers: everything below is added.</p>{/if}
	<DiffView changes={compare(loaded.type.definition, loaded.live?.content ?? null, loaded.pinned.content)} />
{/if}
