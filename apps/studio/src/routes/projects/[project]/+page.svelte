<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui';
	import { listContentTypes } from '#lib/api.ts';

	const project = $derived(page.params.project ?? '');
	const types = createQuery(() => ({ queryKey: ['types', project], queryFn: () => listContentTypes(project) }));

	let showCreateMessage = $state(false);
</script>

<svelte:head><title>Content · Colophon Studio</title></svelte:head>

<div class="page-header">
	<h1>Content</h1>
	<Button onclick={() => (showCreateMessage = true)}>+ Create content type</Button>
</div>

{#if showCreateMessage}
	<div class="info-message">
		<p>Content type creation will be added in a future update.</p>
		<Button variant="outlined" onclick={() => (showCreateMessage = false)}>Close</Button>
	</div>
{/if}

{#if types.isPending}
	<p class="muted">Loading…</p>
{:else if types.isError}
	<p role="alert">{types.error.message}</p>
{:else if types.data.length === 0}
	<div class="empty-state">
		<p class="muted">No content types yet.</p>
		<p class="muted">Create your first content type to start organizing your content.</p>
		<Button onclick={() => (showCreateMessage = true)}>Create content type</Button>
	</div>
{:else}
	<div class="content-types-grid" data-testid="content-types">
		{#each types.data as type (type.slug)}
			<a
				href={resolve('/projects/[project]/[collection]', { project, collection: type.slug })}
				class="content-type-card"
			>
				<div class="card-header">
					<h3>{type.definition.name}</h3>
					<span class="badge">{type.slug}</span>
				</div>
				<p class="card-meta">
					{type.definition.fields.length} field{type.definition.fields.length === 1 ? '' : 's'} ·
					version {type.version}
				</p>
			</a>
		{/each}
	</div>
{/if}

<style>
	.page-header {
		display: flex;
		justify-content: space-between;
		align-items: center;
		margin-bottom: 2rem;
	}

	.page-header h1 {
		margin: 0;
	}

	.muted {
		color: var(--md-sys-color-on-surface-variant);
	}

	.info-message {
		padding: 1rem;
		background: var(--md-sys-color-primary-container);
		color: var(--md-sys-color-on-primary-container);
		border-radius: var(--md-sys-shape-corner-medium);
		margin-bottom: 2rem;
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 1rem;
	}

	.info-message p {
		margin: 0;
		flex: 1;
	}

	.empty-state {
		text-align: center;
		padding: 4rem 2rem;
		border: 2px dashed var(--md-sys-color-outline-variant);
		border-radius: var(--md-sys-shape-corner-extra-large);
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 1rem;
	}

	.content-types-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
		gap: 1rem;
	}

	.content-type-card {
		display: block;
		padding: 1.5rem;
		background: var(--md-sys-color-surface-container-low);
		border-radius: var(--md-sys-shape-corner-medium);
		text-decoration: none;
		color: var(--md-sys-color-on-surface);
		border: 1px solid var(--md-sys-color-outline-variant);
		transition: background 0.2s, border-color 0.2s;
	}

	.content-type-card:hover {
		background: var(--md-sys-color-surface-container-high);
		border-color: var(--md-sys-color-primary);
	}

	.card-header {
		display: flex;
		justify-content: space-between;
		align-items: start;
		gap: 0.5rem;
		margin-bottom: 0.5rem;
	}

	.card-header h3 {
		margin: 0;
		font: var(--md-sys-typescale-headline-small);
	}

	.badge {
		font: var(--md-sys-typescale-label-small);
		color: var(--md-sys-color-on-surface-variant);
		background: var(--md-sys-color-surface-container-highest);
		padding: 0.25rem 0.5rem;
		border-radius: var(--md-sys-shape-corner-small);
	}

	.card-meta {
		margin: 0;
		font: var(--md-sys-typescale-body-medium);
		color: var(--md-sys-color-on-surface-variant);
	}
</style>
