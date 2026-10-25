<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import type { RichTextValue } from '@colophon/editor';
	import { Button, TextField } from '@colophon/ui';
	import { ApiError, createArticle, listArticles, newKey } from '#lib/api.ts';

	const EMPTY: RichTextValue = { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } };

	const project = $derived(page.params.project ?? '');
	const articles = createQuery(() => ({ queryKey: ['articles', project], queryFn: () => listArticles(project) }));

	let title = $state('');
	let error = $state('');
	let busy = $state(false);
	// One key per article being created: pressing Create again after a lost answer is safe.
	let key = newKey();

	async function create(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		try {
			const loaded = await createArticle(project, { title, body: EMPTY }, key);
			key = newKey();
			await goto(resolve('/projects/[project]/articles/[entry]', { project, entry: loaded.draft.id }));
		} catch (failure) {
			error = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>Articles · Colophon Studio</title></svelte:head>

<p><a href={resolve('/')}>← Projects</a></p>
<h1>Articles</h1>

<form class="new" onsubmit={create}>
	<TextField label="New article title" bind:value={title} {error} maxlength={200} required />
	<Button type="submit" disabled={busy}>Create article</Button>
</form>

{#if articles.isPending}
	<p class="muted">Loading…</p>
{:else if articles.isError}
	<p role="alert">{articles.error.message}</p>
{:else}
	<ul class="list" data-testid="articles">
		{#each articles.data as article (article.id)}
			<li>
				<a href={resolve('/projects/[project]/articles/[entry]', { project, entry: article.id })}>{article.revision.title}</a>
				<small class="muted">saved {new Date(article.updated_at).toLocaleString()}</small>
			</li>
		{:else}
			<li class="muted">No articles yet.</li>
		{/each}
	</ul>
{/if}

<style>
	.new {
		display: flex;
		gap: 0.75rem;
		align-items: end;
		flex-wrap: wrap;
		margin-bottom: 1.5rem;
	}
	.list {
		display: grid;
		gap: 0.5rem;
		padding-left: 1.25rem;
	}
	.muted {
		color: var(--md-sys-color-on-surface-variant);
	}
</style>
