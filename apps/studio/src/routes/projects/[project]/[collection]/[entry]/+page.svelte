<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import EntryEditor from '#lib/EntryEditor.svelte';
	import { getContentType, getEntry, signInUrl } from '#lib/api.ts';

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
</script>

<svelte:head><title>Editing · Colophon Studio</title></svelte:head>

<p><a href={resolve('/projects/[project]/[collection]', { project, collection })}>← {type.data?.definition.name ?? 'Entries'}</a></p>
<h1>Edit entry</h1>
{#if draft.isPending || type.isPending}
	<p class="muted">Loading…</p>
{:else if draft.isError || type.isError}
	{@const message = (draft.error ?? type.error)?.message ?? ''}
	<p role="alert">
		{message}
		{#if message.includes('sign in')}<a href={signInUrl(page.url.pathname)} data-sveltekit-reload>Sign in</a>{/if}
	</p>
{:else}
	{#key entry}
		<EntryEditor {project} {collection} {entry} type={type.data} initial={draft.data} />
	{/key}
{/if}

<style>
	.muted {
		color: var(--md-sys-color-on-surface-variant);
	}
</style>
