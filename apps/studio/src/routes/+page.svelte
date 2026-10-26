<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Button, TextField } from '@colophon/ui';
	import { ApiError, createProject, me, signInUrl } from '#lib/api.ts';

	const queryClient = useQueryClient();
	const who = createQuery(() => ({ queryKey: ['me'], queryFn: me }));

	let name = $state('');
	let slug = $state('');
	// The slug follows the name until someone edits it.
	let slugEdited = $state(false);
	let error = $state('');
	let busy = $state(false);

	const slugOf = (text: string) =>
		text
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, '-')
			.replace(/^-+|-+$/g, '')
			.slice(0, 63)
			.replace(/-+$/, '');

	async function create(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		try {
			const project = await createProject(slug, name);
			await queryClient.invalidateQueries({ queryKey: ['me'] });
			await goto(resolve('/projects/[project]', { project: project.id }));
		} catch (failure) {
			error = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			busy = false;
		}
	}
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
	<p class="muted" data-testid="signed-in">
		Signed in as {who.data.display_name ?? who.data.id}{who.data.administrator ? ', administrator' : ''}
	</p>
	<ul class="list" data-testid="projects">
		{#each who.data.projects as project (project.id)}
			<li><a href={resolve('/projects/[project]', { project: project.id })}>{project.name}</a> <small>{project.role}</small></li>
		{:else}
			<li class="muted">No projects yet.</li>
		{/each}
	</ul>
	{#if who.data.administrator}
		<form class="new" onsubmit={create} aria-label="New project">
			<h2>New project</h2>
			<TextField
				label="Name"
				bind:value={name}
				required
				maxlength={200}
				oninput={() => {
					if (!slugEdited) slug = slugOf(name);
				}}
			/>
			<TextField
				label="Slug (in addresses and the CLI)"
				bind:value={slug}
				required
				maxlength={63}
				pattern="[a-z0-9]([a-z0-9\-]*[a-z0-9])?"
				oninput={() => (slugEdited = true)}
			/>
			{#if error}<p role="alert" class="error">{error}</p>{/if}
			<div><Button type="submit" disabled={busy}>Create project</Button></div>
		</form>
	{:else}
		<p class="muted" data-testid="not-administrator">
			Only an administrator can create projects. To work on one, ask its owner for an invitation link.
		</p>
	{/if}
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
	.new {
		display: grid;
		gap: 0.75rem;
		max-width: 32rem;
		margin-top: 2rem;
	}
	.new h2 {
		margin: 0;
	}
	.error {
		color: var(--md-sys-color-error);
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
