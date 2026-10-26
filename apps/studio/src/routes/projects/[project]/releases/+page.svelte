<!--
  A project's releases (D-045): each one numbered, what it published and who published it, and
  where anyone can read the result. Release numbers only ever go up by one.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { deliveryUrl, listReleases } from '#lib/api.ts';

	const project = $derived(page.params.project ?? '');
	const releases = createQuery(() => ({ queryKey: ['releases', project], queryFn: () => listReleases(project) }));
</script>

<svelte:head><title>Releases · Colophon Studio</title></svelte:head>

<p><a href={resolve('/projects/[project]', { project })}>← Content</a></p>
<h1>Releases</h1>

{#if releases.isPending}
	<p class="muted">Loading…</p>
{:else if releases.isError}
	<p role="alert">{releases.error.message}</p>
{:else}
	<p data-testid="current-generation">
		{#if releases.data.generation === 0}
			Nothing is published yet.
		{:else}
			Live: release {releases.data.generation}. Anyone can read it at
			<a href={deliveryUrl(project, 'articles')} data-testid="delivery-link" data-sveltekit-reload>{deliveryUrl(project, 'articles')}</a>.
		{/if}
	</p>
	<ol class="list" reversed data-testid="releases">
		{#each releases.data.releases as release (release.generation)}
			<li>
				<strong>Release {release.generation}</strong>:
				<a href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: release.changeset_id })}>{release.title}</a>
				<small class="muted">
					{release.entries}
					{release.entries === 1 ? 'entry' : 'entries'} changed, by {release.published_by_name ?? release.published_by},
					{new Date(release.published_at).toLocaleString()}
				</small>
			</li>
		{/each}
	</ol>
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
