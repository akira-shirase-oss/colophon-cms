<!--
  What one entry's change does for readers, from what they had to what they get: a publish
  request's item before it goes live, or a receipt's row after. A newer version is compared field
  by field (rich text block by block, in its shape); a new entry is shown as readers will read it,
  with nothing marked, because everything in it is new (the audit's P2-13); a takedown shows what
  readers stop getting.
-->
<script lang="ts">
	import { createQuery } from '@tanstack/svelte-query';
	import { type ChangesetItem, getContentType, getRevision } from './api.ts';
	import { compare } from './diff.ts';
	import DiffView from './DiffView.svelte';
	import VersionPreview from './VersionPreview.svelte';

	let {
		project,
		item,
		done = false
	}: {
		project: string;
		/** `published_revision_id` is what readers had before the change (null: nothing). */
		item: Pick<ChangesetItem, 'kind' | 'entry_id' | 'collection' | 'revision_id' | 'published_revision_id'>;
		/** It already happened (a receipt): said in the past tense. */
		done?: boolean;
	} = $props();

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
	<p class="mb-3 text-sm text-attention-ink" data-testid="takedown-note">
		{done ? 'Readers stopped seeing this entry. What they had:' : 'Readers will no longer see this entry. What they have now:'}
	</p>
	<VersionPreview {project} definition={loaded.type.definition} content={loaded.pinned.content} />
{:else if item.published_revision_id === null}
	<p class="mb-3 text-sm text-muted-foreground" data-testid="new-note">{done ? 'New for readers, as they first read it:' : 'New for readers, as they will read it:'}</p>
	<VersionPreview {project} definition={loaded.type.definition} content={loaded.pinned.content} />
{:else}
	<DiffView changes={compare(loaded.type.definition, loaded.live?.content ?? null, loaded.pinned.content)} before={done ? 'the version before' : 'live'} after={done ? 'this release' : 'this request'} />
{/if}
