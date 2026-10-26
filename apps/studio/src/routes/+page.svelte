<script lang="ts">
	import { resolve } from '$app/paths';
	import { createQuery } from '@tanstack/svelte-query';
	import { me, signInUrl } from '#lib/api.ts';

	const who = createQuery(() => ({ queryKey: ['me'], queryFn: me }));
</script>

<svelte:head><title>Projects · Colophon Studio</title></svelte:head>

<h1>Projects</h1>
{#if who.isPending}
	<p class="muted">Loading…</p>
{:else if who.isError}
	<p role="alert">The server didn't answer: {who.error.message}</p>
{:else if who.data === null}
	<p data-testid="signed-out">Sign in to write and edit.</p>
	<a class="action" href={signInUrl(resolve('/'))} data-sveltekit-reload data-testid="sign-in">Sign in</a>
{:else}
	<p class="muted" data-testid="signed-in">Signed in as {who.data.display_name ?? who.data.id}</p>
	<ul class="list" data-testid="projects">
		{#each who.data.projects as project (project.id)}
			<li><a href={resolve('/projects/[project]', { project: project.id })}>{project.name}</a> <small>{project.role}</small></li>
		{:else}
			<li class="muted">No projects yet.</li>
		{/each}
	</ul>
{/if}
<p class="muted small"><a href={resolve('/playground')}>Editor playground</a>: every editing feature, on the shared fixtures, without saving.</p>

<style>
	.muted {
		color: var(--md-sys-color-on-surface-variant);
	}
	.small {
		margin-top: 2rem;
		font: var(--md-sys-typescale-body-small);
	}
	.list {
		display: grid;
		gap: 0.5rem;
		padding-left: 1.25rem;
	}
	.action {
		display: inline-block;
		padding: 0.625rem 1.5rem;
		border-radius: var(--md-sys-shape-corner-full, 999px);
		background: var(--md-sys-color-primary);
		color: var(--md-sys-color-on-primary);
		text-decoration: none;
		font: var(--md-sys-typescale-label-large);
	}
</style>
