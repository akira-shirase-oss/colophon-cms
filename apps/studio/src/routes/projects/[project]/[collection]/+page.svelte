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

{#if type.isPending}
	<p class="muted">Loading…</p>
{:else if type.isError}
	<p role="alert">{type.error.message}</p>
{:else}
	<div class="page-header">
		<h1>{type.data.definition.name}</h1>
		<Button variant="outlined" onclick={() => document.getElementById('new-entry-form')?.scrollIntoView({ behavior: 'smooth' })}>
			+ New entry
		</Button>
	</div>

	<form id="new-entry-form" class="new" onsubmit={create} aria-label="New entry">
		<h2>New entry</h2>
		<FieldInputs fields={asked} bind:form titleField={type.data.definition.title_field} />
		{#if error}<p role="alert" class="error">{error}</p>{/if}
		<div><Button type="submit" disabled={busy}>Create entry</Button></div>
	</form>

	{#if entries.isPending}
		<p class="muted">Loading entries…</p>
	{:else if entries.isError}
		<p role="alert">{entries.error.message}</p>
	{:else if entries.data.length === 0}
		<div class="empty-state">
			<p class="muted">No entries yet.</p>
			<p class="muted">Create your first entry to get started.</p>
		</div>
	{:else}
		<div class="entries-grid" data-testid="entries">
			{#each entries.data as entry (entry.id)}
				<a
					href={resolve('/projects/[project]/[collection]/[entry]', { project, collection, entry: entry.id })}
					class="entry-card"
				>
					<h3>{entry.revision.title}</h3>
					<p class="entry-meta">
						<span class="status-badge draft">Draft</span>
						<span class="date">saved {new Date(entry.updated_at).toLocaleDateString()}</span>
					</p>
				</a>
			{/each}
		</div>
	{/if}
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

	.new {
		display: grid;
		gap: 0.75rem;
		max-width: 40rem;
		margin-bottom: 3rem;
		padding: 1.5rem;
		background: var(--md-sys-color-surface-container-low);
		border-radius: var(--md-sys-shape-corner-medium);
		border: 1px solid var(--md-sys-color-outline-variant);
	}

	.new h2 {
		margin: 0;
	}

	.error {
		color: var(--md-sys-color-error);
	}

	.muted {
		color: var(--md-sys-color-on-surface-variant);
	}

	.empty-state {
		text-align: center;
		padding: 4rem 2rem;
		border: 2px dashed var(--md-sys-color-outline-variant);
		border-radius: var(--md-sys-shape-corner-extra-large);
	}

	.entries-grid {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
		gap: 1rem;
	}

	.entry-card {
		display: block;
		padding: 1.5rem;
		background: var(--md-sys-color-surface-container-low);
		border-radius: var(--md-sys-shape-corner-medium);
		text-decoration: none;
		color: var(--md-sys-color-on-surface);
		border: 1px solid var(--md-sys-color-outline-variant);
		transition: background 0.2s, border-color 0.2s;
	}

	.entry-card:hover {
		background: var(--md-sys-color-surface-container-high);
		border-color: var(--md-sys-color-primary);
	}

	.entry-card h3 {
		margin: 0 0 0.5rem 0;
		font: var(--md-sys-typescale-headline-small);
	}

	.entry-meta {
		margin: 0;
		display: flex;
		align-items: center;
		gap: 0.75rem;
		font: var(--md-sys-typescale-body-small);
		color: var(--md-sys-color-on-surface-variant);
	}

	.status-badge {
		padding: 0.25rem 0.5rem;
		border-radius: var(--md-sys-shape-corner-small);
		font: var(--md-sys-typescale-label-small);
		font-weight: 500;
	}

	.status-badge.draft {
		background: var(--md-sys-color-surface-container-highest);
		color: var(--md-sys-color-on-surface-variant);
	}

	.date {
		flex: 1;
	}
</style>
