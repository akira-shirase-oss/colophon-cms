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
	import { createInfiniteQuery, createQuery, keepPreviousData } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import * as InputGroup from '@colophon/ui/ui/input-group';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import * as Table from '@colophon/ui/ui/table';
	import * as Tabs from '@colophon/ui/ui/tabs';
	import { ApiError, createEntry, type EntryState, entryState, getContentType, may, newKey, searchEntries } from '#lib/api.ts';
	import { type FormValue, formValue, slugify, starter, storedValue } from '#lib/content.ts';
	import { plainError } from '#lib/errors.ts';
	import FieldInputs from '#lib/FieldInputs.svelte';
	import { oneOf } from '#lib/firstRun.ts';
	import { ago, projectQuery } from '#lib/queries.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import StatusBadge from '#lib/shell/StatusBadge.svelte';
	import { shownTitle } from '#lib/words.ts';

	const project = $derived(page.params.project ?? '');
	const collection = $derived(page.params.collection ?? '');
	const type = createQuery(() => ({ queryKey: ['type', project, collection], queryFn: () => getContentType(project, collection) }));
	const mine = createQuery(() => projectQuery(project));
	const writes = $derived(mine.data === undefined || may(mine.data, 'content.create', collection));

	let typed = $state('');
	let q = $state('');
	$effect(() => {
		const next = typed;
		const timer = setTimeout(() => (q = next.trim()), 200);
		return () => clearTimeout(timer);
	});
	// Pages of 100, newest first, followed with the server's cursor when someone asks for more. Keyed
	// under the collection's entries, so saving and publishing refresh it.
	const entries = createInfiniteQuery(() => ({
		queryKey: ['entries', project, collection, q, 'pages'],
		queryFn: ({ pageParam }: { pageParam: string | null }) => searchEntries(project, collection, q, pageParam),
		initialPageParam: null as string | null,
		getNextPageParam: (last: { next_cursor: string | null }) => last.next_cursor,
		placeholderData: keepPreviousData
	}));
	const loaded = $derived((entries.data?.pages ?? []).flatMap((one) => one.data));
	let filter = $state<'all' | EntryState>('all');
	const rows = $derived(loaded.filter((entry) => filter === 'all' || entryState(entry) === filter));
	const tally = $derived(
		loaded.reduce<Record<EntryState, number>>(
			(counts, entry) => {
				counts[entryState(entry)] += 1;
				return counts;
			},
			{ draft: 0, live: 0, changed: 0 }
		)
	);

	// A new entry asks only for the required fields that have no natural starting value. A URL name
	// with a source field is made from it (the title, usually), and can be changed in the editor.
	const asked = $derived(
		(type.data?.definition.fields ?? []).filter(
			(field) =>
				field.required && field.type !== 'rich_text' && field.type !== 'boolean' && !(field.type === 'slug' && field.source !== undefined)
		)
	);
	// `?new=1` (the first-run checklist's "Write") opens the new-entry dialog at once.
	let creating = $state(page.url.searchParams.get('new') === '1');
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
		// A URL name made here from the title; if another entry has it, the next free one ("-2", "-3"…),
		// as the server's uniqueness rule (D-054) asks. It can be changed in the editor.
		const made = definition.fields.find(
			(field) => field.type === 'slug' && field.required && field.source !== undefined && content[field.id] === undefined
		);
		const base = made?.type === 'slug' && made.source !== undefined && typeof content[made.source] === 'string' ? slugify(content[made.source] as string) : '';
		try {
			for (let attempt = 1; ; attempt += 1) {
				if (made !== undefined) content[made.id] = attempt === 1 ? base : `${base}-${attempt}`;
				try {
					const loaded = await createEntry(project, collection, content, key);
					key = newKey();
					creating = false;
					form = {};
					await goto(resolve('/projects/[project]/[collection]/[entry]', { project, collection, entry: loaded.draft.id }));
					return;
				} catch (failure) {
					if (!(made !== undefined && failure instanceof ApiError && failure.reason === 'slug_taken' && attempt < 9)) throw failure;
					// A refused create left nothing behind; the next try is a different request.
					key = newKey();
				}
			}
		} catch (failure) {
			error =
				failure instanceof ApiError && failure.reason === 'slug_taken'
					? 'Another entry already uses that URL name. Give this one a different title.'
					: plainError(failure, 'create entries here');
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
		{#snippet description()}Everything here saves as you type and stays a draft until it is published.{/snippet}
		{#snippet actions()}
			{#if writes}<Button onclick={() => (creating = true)}><PlusIcon data-icon="inline-start" />New entry</Button>{/if}
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
		<Problem error={entries.error} what={type.data.definition.name} retry={() => entries.refetch()} />
	{:else if loaded.length === 0 && q === ''}
		<EmptyState icon={FileTextIcon} title="No {type.data.definition.name.toLowerCase()} yet" testid="entries-empty">
			Write the first {oneOf(type.data.definition.name)}. Nobody outside the project sees it until it is published.
			{#snippet action()}
				{#if writes}<Button onclick={() => (creating = true)}><PlusIcon data-icon="inline-start" />Write the first {oneOf(type.data.definition.name)}</Button>{/if}
			{/snippet}
		</EmptyState>
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
									href={resolve('/projects/[project]/[collection]/[entry]', { project, collection, entry: entry.id })}
									class:text-muted-foreground={entry.revision.title.trim() === ''}>{shownTitle(entry.revision.title)}</a
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
		{#if entries.hasNextPage || entries.isFetchingNextPage || entries.isFetchNextPageError}
			<div class="mt-3 flex flex-wrap items-center gap-3">
				<p class="text-sm text-muted-foreground" data-testid="entries-shown">Showing the newest {loaded.length.toLocaleString()}.</p>
				<Button variant="outline" size="sm" onclick={() => entries.fetchNextPage()} disabled={entries.isFetchingNextPage} data-testid="load-more">
					{entries.isFetchingNextPage ? 'Loading more…' : 'Load more'}
				</Button>
				{#if entries.isFetchNextPageError}<p role="alert" class="text-sm text-destructive">Couldn't load more. Try again.</p>{/if}
			</div>
		{/if}
	{/if}

	<Dialog.Root bind:open={creating}>
		<Dialog.Content class="sm:max-w-lg">
			<Dialog.Header>
				<Dialog.Title>New {oneOf(type.data.definition.name)}</Dialog.Title>
				<Dialog.Description>It starts as a draft. Readers see it only once it is published.</Dialog.Description>
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
