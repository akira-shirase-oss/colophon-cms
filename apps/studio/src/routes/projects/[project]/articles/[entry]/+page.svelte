<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import ArticleEditor from '#lib/ArticleEditor.svelte';
	import { getArticle, signInUrl } from '#lib/api.ts';

	const project = $derived(page.params.project ?? '');
	const entry = $derived(page.params.entry ?? '');
	// Loaded once per visit: after that the editor owns the content, and saving keeps it current.
	const draft = createQuery(() => ({
		queryKey: ['draft', project, entry],
		queryFn: () => getArticle(project, entry),
		staleTime: Infinity,
		refetchOnWindowFocus: false
	}));
</script>

<svelte:head><title>Editing · Colophon Studio</title></svelte:head>

<p><a href={resolve('/projects/[project]', { project })}>← Articles</a></p>
<h1>Edit article</h1>
{#if draft.isPending}
	<p class="muted">Loading…</p>
{:else if draft.isError}
	<p role="alert">
		{draft.error.message}
		{#if draft.error.message.includes('sign in')}<a href={signInUrl(page.url.pathname)} data-sveltekit-reload>Sign in</a>{/if}
	</p>
{:else}
	{#key entry}
		<ArticleEditor {project} {entry} initial={draft.data} />
	{/key}
{/if}

<style>
	.muted {
		color: var(--md-sys-color-on-surface-variant);
	}
</style>
