<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui';
	import { ApiError, createEntry, getContentType, listEntries, newKey } from '#lib/api.ts';
	import { type FormValue, formValue, starter, storedValue } from '#lib/content.ts';
	import FieldInputs from '#lib/FieldInputs.svelte';

	const project = $derived(page.params.project ?? '');
	const collection = $derived(page.params.collection ?? '');
	const type = createQuery(() => ({ queryKey: ['type', project, collection], queryFn: () => getContentType(project, collection) }));
	const entries = createQuery(() => ({
		queryKey: ['entries', project, collection],
		queryFn: () => listEntries(project, collection)
	}));

	// A new entry asks only for the required fields that have no natural starting value.
	const asked = $derived(
		(type.data?.definition.fields ?? []).filter((field) => field.required && field.type !== 'rich_text' && field.type !== 'boolean')
	);
	let form = $state<Record<string, FormValue>>({});
	let error = $state('');
	let busy = $state(false);
	// One key per entry being created: pressing Create again after a lost answer is safe.
	let key = newKey();

	async function create(event: SubmitEvent) {
		event.preventDefault();
		const definition = type.data?.definition;
		if (definition === undefined) return;
		busy = true;
		error = '';
		const content = starter(definition);
		for (const field of asked) {
			const value = storedValue(field, form[field.id] ?? formValue(field, content[field.id]));
			if (value !== undefined) content[field.id] = value;
		}
		try {
			const loaded = await createEntry(project, collection, content, key);
			key = newKey();
			await goto(resolve('/projects/[project]/[collection]/[entry]', { project, collection, entry: loaded.draft.id }));
		} catch (failure) {
			error = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>{type.data?.definition.name ?? 'Entries'} · Colophon Studio</title></svelte:head>

<p><a href={resolve('/projects/[project]', { project })}>← Content</a></p>
{#if type.isPending}
	<p class="muted">Loading…</p>
{:else if type.isError}
	<p role="alert">{type.error.message}</p>
{:else}
	<h1>{type.data.definition.name}</h1>
	<form class="new" onsubmit={create} aria-label="New entry">
		<h2>New entry</h2>
		<FieldInputs fields={asked} bind:form titleField={type.data.definition.title_field} />
		{#if error}<p role="alert" class="error">{error}</p>{/if}
		<div><Button type="submit" disabled={busy}>Create</Button></div>
	</form>

	{#if entries.isPending}
		<p class="muted">Loading…</p>
	{:else if entries.isError}
		<p role="alert">{entries.error.message}</p>
	{:else}
		<ul class="list" data-testid="entries">
			{#each entries.data as entry (entry.id)}
				<li>
					<a href={resolve('/projects/[project]/[collection]/[entry]', { project, collection, entry: entry.id })}>{entry.revision.title}</a>
					<small class="muted">saved {new Date(entry.updated_at).toLocaleString()}</small>
				</li>
			{:else}
				<li class="muted">Nothing here yet.</li>
			{/each}
		</ul>
	{/if}
{/if}

<style>
	.new {
		display: grid;
		gap: 0.75rem;
		max-width: 40rem;
		margin-bottom: 2rem;
	}
	.new h2 {
		margin: 0;
	}
	.error {
		color: var(--md-sys-color-error);
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
