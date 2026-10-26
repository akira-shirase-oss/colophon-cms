<!--
  A project's changesets (D-044): what each is waiting for, and a form to pin entries into a new
  one. A changeset holds the exact revisions it was made from; reviews approve exactly those. An
  entry can be picked to publish its current version or to take its published one down (D-049).
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Button, TextField } from '@colophon/ui';
	import { ApiError, createChangeset, listChangesets, listContentTypes, listEntries, newKey } from '#lib/api.ts';
	import { STATE_LABEL } from '#lib/changesets.ts';

	const project = $derived(page.params.project ?? '');
	const sets = createQuery(() => ({ queryKey: ['changesets', project], queryFn: () => listChangesets(project) }));
	// Every entry, by content type, to pick from.
	const choices = createQuery(() => ({
		queryKey: ['changeset-choices', project],
		queryFn: async () => {
			const types = await listContentTypes(project);
			return Promise.all(types.map(async (type) => ({ type, entries: await listEntries(project, type.slug) })));
		}
	}));

	let title = $state('');
	let picked = $state<string[]>([]);
	let takenDown = $state<string[]>([]);
	let error = $state('');
	let busy = $state(false);
	// One key per changeset being made: pressing Create again after a lost answer is safe.
	let key = newKey();

	async function create(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		try {
			const made = await createChangeset(project, title, picked, takenDown, key);
			key = newKey();
			await goto(resolve('/projects/[project]/changesets/[changeset]', { project, changeset: made.id }));
		} catch (failure) {
			error = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>Changesets · Colophon Studio</title></svelte:head>

<p><a href={resolve('/projects/[project]', { project })}>← Content</a></p>
<h1>Changesets</h1>

{#if sets.isPending}
	<p class="muted">Loading…</p>
{:else if sets.isError}
	<p role="alert">{sets.error.message}</p>
{:else}
	<ul class="list" data-testid="changesets">
		{#each sets.data as set (set.id)}
			<li>
				<a href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: set.id })}>{set.title}</a>
				<span class="state" data-state={set.status.state}>{STATE_LABEL[set.status.state]}</span>
				<small class="muted"
					>{set.items} {set.items === 1 ? 'entry' : 'entries'}{#if set.unpublish > 0}, {set.unpublish} to unpublish{/if}</small
				>
			</li>
		{:else}
			<li class="muted">No changesets yet.</li>
		{/each}
	</ul>
{/if}

<form class="new" onsubmit={create} aria-label="New changeset">
	<h2>New changeset</h2>
	<p class="muted">
		It pins the current version of each entry you pick to publish, and the published version of each entry you pick to
		unpublish. Later changes don't change it until you refresh it.
	</p>
	<TextField label="Title" bind:value={title} required maxlength={200} />
	{#if choices.isPending}
		<p class="muted">Loading entries…</p>
	{:else if choices.isError}
		<p role="alert">{choices.error.message}</p>
	{:else}
		{#each choices.data as group (group.type.slug)}
			{#if group.entries.length > 0}
				<fieldset>
					<legend>{group.type.definition.name}</legend>
					{#each group.entries as entry (entry.id)}
						<!-- Picking an entry publishes its current version; Unpublish takes its published one down. -->
						<div class="pick">
							<label
								><input type="checkbox" value={entry.id} bind:group={picked} disabled={takenDown.includes(entry.id)} />
								{entry.revision.title}</label
							>
							<label class="down"
								><input
									type="checkbox"
									value={entry.id}
									bind:group={takenDown}
									disabled={picked.includes(entry.id)}
									aria-label={`Unpublish ${entry.revision.title}`}
								/> Unpublish</label
							>
						</div>
					{/each}
				</fieldset>
			{/if}
		{:else}
			<p class="muted">No entries to pick yet.</p>
		{/each}
	{/if}
	{#if error}<p role="alert" class="error">{error}</p>{/if}
	<div><Button type="submit" disabled={busy || picked.length + takenDown.length === 0}>Create changeset</Button></div>
</form>

<style>
	.list {
		display: grid;
		gap: 0.5rem;
		padding-left: 1.25rem;
	}
	.state {
		font: var(--md-sys-typescale-label-medium);
		padding: 0.125rem 0.5rem;
		border-radius: var(--md-sys-shape-corner-small, 8px);
		background: var(--md-sys-color-secondary-container);
		color: var(--md-sys-color-on-secondary-container);
	}
	.new {
		display: grid;
		gap: 0.75rem;
		max-width: 40rem;
		margin-top: 2rem;
	}
	.new h2 {
		margin: 0;
	}
	fieldset {
		display: grid;
		gap: 0.375rem;
		border: 1px solid var(--md-sys-color-outline-variant);
		border-radius: var(--md-sys-shape-corner-small, 8px);
		padding: 0.75rem 1rem;
	}
	legend {
		font: var(--md-sys-typescale-label-large);
		padding: 0 0.25rem;
	}
	.pick {
		display: flex;
		gap: 1rem;
		align-items: center;
	}
	.down {
		color: var(--md-sys-color-on-surface-variant);
	}
	.error {
		color: var(--md-sys-color-error);
	}
	.muted {
		color: var(--md-sys-color-on-surface-variant);
	}
</style>
