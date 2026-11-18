<script lang="ts">
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import EntryEditor from '#lib/EntryEditor.svelte';
	import { getContentType, getEntry } from '#lib/api.ts';
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
	// What readers get now: the entry's own answer says (B1), however old it is. Keyed under the
	// collection's entries, so saving and publishing, which refresh those, refresh this too.
	// Its read also says which entries link to it (D-065).
	const state = createQuery(() => ({
		queryKey: ['entries', project, collection, 'state', entry],
		queryFn: async () => {
			const { draft } = await getEntry(project, collection, entry);
			return { published: draft.published ?? null, referencedBy: draft.referenced_by ?? [] };
		}
	}));
	const published = $derived(state.data !== undefined ? state.data.published : (draft.data?.draft.published ?? null));
	const referencedBy = $derived(state.data?.referencedBy ?? draft.data?.draft.referenced_by ?? []);
</script>

<svelte:head><title>{draft.data?.draft.revision.title ?? 'Editing'} · Colophon Studio</title></svelte:head>

<!-- A refresh that fails keeps what is on screen: only a first load that fails has nothing better
     to show than the problem. (Replacing an open editor because a background read was turned away
     — the rate limit, a blip — took the person's page away under them.) -->
{#if draft.data === undefined && draft.isError}
	<Problem error={draft.error as Error} />
{:else if type.data === undefined && type.isError}
	<Problem error={type.error as Error} />
{:else if draft.data === undefined || type.data === undefined}
	<Skeleton class="mb-8 h-10 w-2/3" />
	<Skeleton class="h-64 w-full rounded-xl" />
{:else}
	<!-- The editor draws the title itself, as the page's heading it edits in place (D-183); the
	     trail above already names the collection, so there is no eyebrow repeating it. -->
	{#key entry}
		<EntryEditor {project} {collection} {entry} type={type.data} initial={draft.data} {published} {referencedBy} />
	{/key}
{/if}
