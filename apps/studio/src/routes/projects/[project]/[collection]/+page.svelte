<!--
  One collection as a table: search titles (on the server, so it finds beyond the first page),
  filter by where each entry stands for readers, and start a new entry.
-->
<script lang="ts">
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import SearchIcon from '@lucide/svelte/icons/search';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, keepPreviousData } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import * as InputGroup from '@colophon/ui/ui/input-group';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import * as Table from '@colophon/ui/ui/table';
	import * as Tabs from '@colophon/ui/ui/tabs';
	import { ApiError, createEntry, type EntryState, entryState, getContentType, newKey } from '#lib/api.ts';
	import { type FormValue, formValue, starter, storedValue } from '#lib/content.ts';
	import FieldInputs from '#lib/FieldInputs.svelte';
	import { ago, entriesQuery } from '#lib/queries.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import StatusBadge from '#lib/shell/StatusBadge.svelte';

	const project = $derived(page.params.project ?? '');
	const collection = $derived(page.params.collection ?? '');
	const type = createQuery(() => ({ queryKey: ['type', project, collection], queryFn: () => getContentType(project, collection) }));

	let typed = $state('');
	let q = $state('');
	$effect(() => {
		const next = typed;
		const timer = setTimeout(() => (q = next.trim()), 200);
		return () => clearTimeout(timer);
	});
	const entries = createQuery(() => ({ ...entriesQuery(project, collection, q), placeholderData: keepPreviousData }));
	let filter = $state<'all' | EntryState>('all');
	const rows = $derived((entries.data?.data ?? []).filter((entry) => filter === 'all' || entryState(entry) === filter));
	const tally = $derived(
		(entries.data?.data ?? []).reduce<Record<EntryState, number>>(
			(counts, entry) => {
				counts[entryState(entry)] += 1;
				return counts;
			},
			{ draft: 0, live: 0, changed: 0 }
		)
	);

	// A new entry asks only for the required fields that have no natural starting value.
	const asked = $derived(
		(type.data?.definition.fields ?? []).filter((field) => field.required && field.type !== 'rich_text' && field.type !== 'boolean')
	);
	let creating = $state(false);
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
			creating = false;
			form = {};
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
	<Skeleton class="mb-8 h-10 w-56" />
{:else if type.isError}
	<Problem error={type.error} />
{:else}
	<PageHeader title={type.data.definition.name}>
		{#snippet meta()}
			<span>{type.data.definition.fields.length} fields</span><span aria-hidden="true">·</span><span>version {type.data.version}</span>
		{/snippet}
		{#snippet actions()}
			<Button onclick={() => (creating = true)}><PlusIcon data-icon="inline-start" />New entry</Button>
		{/snippet}
	</PageHeader>

	<div class="mb-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
		<Tabs.Root bind:value={filter as string}>
			<Tabs.List aria-label="Show">
				<Tabs.Trigger value="all">All</Tabs.Trigger>
				<Tabs.Trigger value="draft">Drafts <span class="tabular text-muted-foreground">{tally.draft}</span></Tabs.Trigger>
				<Tabs.Trigger value="changed">Changed <span class="tabular text-muted-foreground">{tally.changed}</span></Tabs.Trigger>
				<Tabs.Trigger value="live">Live <span class="tabular text-muted-foreground">{tally.live}</span></Tabs.Trigger>
			</Tabs.List>
		</Tabs.Root>
		<InputGroup.Root class="sm:w-72">
			<InputGroup.Addon><SearchIcon /></InputGroup.Addon>
			<InputGroup.Input type="search" placeholder="Search titles" aria-label="Search titles" maxlength={100} bind:value={typed} data-testid="entry-search" />
		</InputGroup.Root>
	</div>

	{#if entries.isPending}
		<Skeleton class="h-48 w-full rounded-xl" />
	{:else if entries.isError}
		<Problem error={entries.error} />
	{:else if entries.data.data.length === 0 && q === ''}
		<EmptyState icon={FileTextIcon} title="Nothing here yet">Create the first {type.data.definition.name.toLowerCase()} entry.</EmptyState>
	{:else}
		<div class="overflow-hidden rounded-xl border border-border bg-card" class:opacity-60={entries.isPlaceholderData}>
			<Table.Root>
				<Table.Header>
					<Table.Row class="hover:bg-transparent">
						<Table.Head class="pl-4">Title</Table.Head>
						<Table.Head>Status</Table.Head>
						<Table.Head class="hidden md:table-cell">Live in</Table.Head>
						<Table.Head class="pr-4 text-right">Updated</Table.Head>
					</Table.Row>
				</Table.Header>
				<Table.Body data-testid="entries" class="stagger">
					{#each rows as entry, index (entry.id)}
						<Table.Row class="relative" style="--i: {index}">
							<Table.Cell class="max-w-0 pl-4 sm:max-w-md">
								<a
									class="block truncate font-medium after:absolute after:inset-0 focus-visible:underline focus-visible:outline-none"
									href={resolve('/projects/[project]/[collection]/[entry]', { project, collection, entry: entry.id })}>{entry.revision.title}</a
								>
							</Table.Cell>
							<Table.Cell><StatusBadge state={entryState(entry)} generation={entry.published?.generation ?? null} /></Table.Cell>
							<Table.Cell class="hidden text-sm text-muted-foreground tabular md:table-cell">
								{entry.published ? `No. ${entry.published.generation}` : '—'}
							</Table.Cell>
							<Table.Cell class="pr-4 text-right text-sm text-muted-foreground" title={new Date(entry.updated_at).toLocaleString()}>{ago(entry.updated_at)}</Table.Cell>
						</Table.Row>
					{:else}
						<Table.Row class="hover:bg-transparent">
							<Table.Cell colspan={4} class="py-10 text-center text-muted-foreground">
								{q === '' ? 'No entries in this view.' : `No titles match “${q}”.`}
							</Table.Cell>
						</Table.Row>
					{/each}
				</Table.Body>
			</Table.Root>
		</div>
		{#if entries.data.next_cursor !== null}
			<p class="mt-3 text-sm text-muted-foreground">Showing the newest 100. Search to find older entries.</p>
		{/if}
	{/if}

	<Dialog.Root bind:open={creating}>
		<Dialog.Content class="sm:max-w-lg">
			<Dialog.Header>
				<Dialog.Title>New {type.data.definition.name.toLowerCase()} entry</Dialog.Title>
				<Dialog.Description>It starts as a draft. Nothing is published until a changeset with it is approved and published.</Dialog.Description>
			</Dialog.Header>
			<form class="grid gap-4" onsubmit={create} aria-label="New entry">
				<FieldInputs fields={asked} bind:form titleField={type.data.definition.title_field} stacked />
				{#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
				<Dialog.Footer>
					<Button variant="outline" type="button" onclick={() => (creating = false)}>Cancel</Button>
					<Button type="submit" disabled={busy}>Create</Button>
				</Dialog.Footer>
			</form>
		</Dialog.Content>
	</Dialog.Root>
{/if}
