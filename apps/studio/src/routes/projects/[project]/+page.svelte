<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { listContentTypes } from '#lib/api.ts';

	const project = $derived(page.params.project ?? '');
	const types = createQuery(() => ({ queryKey: ['types', project], queryFn: () => listContentTypes(project) }));
</script>

<svelte:head><title>Content · Colophon Studio</title></svelte:head>

<p><a href={resolve('/')}>← Projects</a></p>
<h1>Content</h1>
<p>
	<a href={resolve('/projects/[project]/members', { project })} data-testid="members-link">Members and invitations</a> ·
	<a href={resolve('/projects/[project]/changesets', { project })} data-testid="changesets-link">Changesets</a>
</p>

{#if types.isPending}
	<p class="muted">Loading…</p>
{:else if types.isError}
	<p role="alert">{types.error.message}</p>
{:else}
	<ul class="list" data-testid="content-types">
		{#each types.data as type (type.slug)}
			<li>
				<a href={resolve('/projects/[project]/[collection]', { project, collection: type.slug })}>{type.definition.name}</a>
				<small class="muted">{type.definition.fields.length} fields · version {type.version}</small>
			</li>
		{/each}
	</ul>
{/if}

<style>
	.list {
		display: grid;
		gap: 0.5rem;
		padding-left: 1.25rem;
	}
	.muted {
		color: var(--md-sys-color-on-surface-variant);
	}
</style>
