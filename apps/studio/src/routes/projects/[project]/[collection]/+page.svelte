<!--
  One collection as a table: search titles (on the server, so it finds beyond the first page),
  filter by where each entry stands for readers, and start a new entry.
-->
<script lang="ts">
	import CopyIcon from '@lucide/svelte/icons/copy';
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import SearchIcon from '@lucide/svelte/icons/search';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createInfiniteQuery, createQuery, keepPreviousData, useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import * as InputGroup from '@colophon/ui/ui/input-group';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import * as Table from '@colophon/ui/ui/table';
	import * as Tabs from '@colophon/ui/ui/tabs';
	import { ApiError, type Content, createEntry, type EntryState, entryState, getContentType, getEntry, may, mayOn, newKey, searchEntries } from '#lib/api.ts';
	import { blockingProblem, type FormValue, formValue, slugify, starter, storedValue } from '#lib/content.ts';
	import { plainError } from '#lib/errors.ts';
	import FieldInputs from '#lib/FieldInputs.svelte';
	import { oneOf } from '#lib/firstRun.ts';
	import { newKeys, type Outcome, publishesInOneClick, publishInOneClick } from '#lib/publishing.ts';
	import { ago, meQuery, projectQuery, releasesQuery } from '#lib/queries.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import StatusBadge from '#lib/shell/StatusBadge.svelte';
	import { PUBLISH_REQUEST, releaseName, shownTitle } from '#lib/words.ts';

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

	// Bulk publishing (bounded: a publish request holds at most 100 entries). Picking rows publishes
	// their current versions together, in one click when the role may approve and publish its own
	// requests here, else as one request sent for review. Live entries with nothing new are left out.
	const queryClient = useQueryClient();
	const releases = createQuery(() => releasesQuery(project));
	const oneClick = $derived(publishesInOneClick(mine.data, [collection]));
	const asks = $derived(may(mine.data, 'content.update', collection));
	const bulk = $derived(mine.data !== undefined && (oneClick || asks));
	let picked = $state<string[]>([]);
	const pickedRows = $derived(loaded.filter((entry) => picked.includes(entry.id)));
	const publishable = $derived(pickedRows.filter((entry) => entryState(entry) !== 'live'));
	// A role that may edit only its own entries can send only those (each row says whose it is).
	const who = createQuery(meQuery);
	const notMine = $derived(
		oneClick ? [] : publishable.filter((entry) => !mayOn(mine.data, 'content.update', collection, entry.created_by === who.data?.id))
	);
	const allShown = $derived(rows.length > 0 && rows.every((entry) => picked.includes(entry.id)));
	let bulkStep = $state('');
	let bulkOutcome = $state<Outcome | null>(null);
	let bulkKeys = newKeys();
	const MAX_BULK = 100;

	function toggleAll(on: boolean) {
		const shown = rows.map((entry) => entry.id);
		picked = on ? [...new Set([...picked, ...shown])] : picked.filter((id) => !shown.includes(id));
	}

	async function publishPicked() {
		const chosen = publishable.slice(0, MAX_BULK);
		if (chosen.length === 0) return;
		bulkOutcome = null;
		const first = shownTitle(chosen[0]?.revision.title ?? '');
		bulkOutcome = await publishInOneClick({
			project,
			title: chosen.length === 1 ? first : `${first} and ${chosen.length - 1} more`,
			entries: chosen.map((entry) => entry.id),
			oneClick,
			seenGeneration: releases.data?.generation,
			keys: bulkKeys,
			step: (words) => (bulkStep = words)
		});
		bulkStep = '';
		bulkKeys = newKeys();
		if (bulkOutcome.kind !== 'stopped') picked = [];
		await Promise.all([
			queryClient.invalidateQueries({ queryKey: ['entries', project] }),
			queryClient.invalidateQueries({ queryKey: ['changesets', project] }),
			queryClient.invalidateQueries({ queryKey: ['releases', project] })
		]);
	}

	// Duplicating an entry: everything the role can read, with "(copy)" after the title and, when
	// the type has a URL name, the next free one ("-2", "-3"…; the server's uniqueness rule, D-054).
	// The copy opens in the editor. Fields the role can't read aren't copied.
	let duplicating = $state<string | null>(null);
	async function duplicate(id: string) {
		const definition = type.data?.definition;
		if (definition === undefined) return;
		duplicating = id;
		try {
			const { draft } = await getEntry(project, collection, id);
			const content: Content = { ...draft.content };
			const titleField = definition.fields.find((field) => field.id === definition.title_field);
			const title = content[definition.title_field];
			if (typeof title === 'string') {
				const limit = titleField?.type === 'text' ? titleField.max_chars : 200;
				content[definition.title_field] = `${title.slice(0, Math.max(0, limit - 7))} (copy)`;
			}
			const slug = definition.fields.find((field) => field.type === 'slug' && typeof content[field.id] === 'string' && content[field.id] !== '');
			const stem = slug === undefined ? '' : String(content[slug.id]).replace(/-\d+$/, '');
			for (let attempt = 2; ; attempt += 1) {
				if (slug !== undefined) content[slug.id] = `${stem}-${attempt}`;
				try {
					const made = await createEntry(project, collection, content, newKey());
					toast.success('Duplicated. This is the copy.');
					await queryClient.invalidateQueries({ queryKey: ['entries', project, collection] });
					await goto(resolve('/projects/[project]/[collection]/[entry]', { project, collection, entry: made.draft.id }));
					return;
				} catch (failure) {
					if (!(slug !== undefined && failure instanceof ApiError && failure.reason === 'slug_taken' && attempt < 50)) throw failure;
				}
			}
		} catch (failure) {
			toast.error(`Not duplicated. ${plainError(failure, 'create entries here')}`);
		} finally {
			duplicating = null;
		}
	}

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
		// JSON that doesn't read yet would be lost or stored as text: say so, send nothing.
		const blocked = blockingProblem(asked, form);
		if (blocked !== '') {
			error = blocked;
			return;
		}
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
		{#if picked.length > 0}
			<div class="mb-3 flex flex-wrap items-center gap-x-3 gap-y-2 rounded-xl border border-border bg-card px-4 py-2.5 shadow-xs" role="region" aria-label="Picked entries" data-testid="bulk-bar">
				<span class="text-sm font-medium tabular">{picked.length} picked</span>
				<Button size="sm" onclick={publishPicked} disabled={bulkStep !== '' || publishable.length === 0 || publishable.length > MAX_BULK || notMine.length > 0} data-testid="bulk-publish">
					{bulkStep !== '' ? bulkStep : oneClick ? `Publish ${publishable.length}` : `Ask for review of ${publishable.length}`}
				</Button>
				<Button size="sm" variant="ghost" onclick={() => (picked = [])}>Clear</Button>
				<span class="text-xs text-muted-foreground">
					{#if publishable.length > MAX_BULK}
						At most {MAX_BULK} at a time: pick fewer.
					{:else if notMine.length > 0}
						Your role can send only entries you started: unpick {notMine.length === 1 ? 'the other one' : `the other ${notMine.length}`}.
					{:else if publishable.length === 0}
						They're live already, with nothing new to publish.
					{:else if publishable.length < pickedRows.length}
						{pickedRows.length - publishable.length} live already, with nothing new, left out.
					{:else if oneClick}
						Goes live together, as one release.
					{:else}
						One {PUBLISH_REQUEST}, for someone who can publish.
					{/if}
				</span>
			</div>
		{/if}
		{#if bulkOutcome}
			<p
				role={bulkOutcome.kind === 'stopped' ? 'alert' : 'status'}
				class="mb-3 rounded-lg px-4 py-2.5 text-sm {bulkOutcome.kind === 'stopped' ? 'bg-attention-soft text-attention-ink' : bulkOutcome.kind === 'requested' ? 'bg-pending-soft text-pending-ink' : 'bg-live-soft text-live-ink'}"
				data-testid="bulk-outcome"
			>
				{#if bulkOutcome.kind === 'published' || bulkOutcome.kind === 'unpublished'}
					Live as <a class="font-medium underline underline-offset-2" href={resolve('/projects/[project]/releases/[generation]', { project, generation: String(bulkOutcome.generation) })}>{releaseName(bulkOutcome.generation)}</a>.
				{:else if bulkOutcome.kind === 'requested'}
					Sent for review. <a class="font-medium underline underline-offset-2" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: bulkOutcome.request })}>See the {PUBLISH_REQUEST}</a>
				{:else}
					Not published: {bulkOutcome.message}
					{#if bulkOutcome.request}<a class="font-medium underline underline-offset-2" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: bulkOutcome.request })}>Open the {PUBLISH_REQUEST}</a>{/if}
				{/if}
			</p>
		{/if}
		<div class="overflow-hidden rounded-xl border border-border bg-card" class:opacity-60={entries.isPlaceholderData}>
			<Table.Root>
				<Table.Header>
					<Table.Row class="hover:bg-transparent">
						{#if bulk}
							<Table.Head class="w-10 pl-4">
								<input
									type="checkbox"
									class="relative z-10 size-4 accent-primary"
									aria-label="Pick every entry shown"
									checked={allShown}
									indeterminate={!allShown && rows.some((entry) => picked.includes(entry.id))}
									onchange={(event) => toggleAll(event.currentTarget.checked)}
								/>
							</Table.Head>
						{/if}
						<Table.Head class={bulk ? '' : 'pl-4'}>Title</Table.Head>
						<Table.Head>Status</Table.Head>
						<Table.Head class="hidden md:table-cell">Live in</Table.Head>
						<Table.Head class="text-right {writes ? '' : 'pr-4'}">Updated</Table.Head>
						{#if writes}<Table.Head class="w-12 pr-4"><span class="sr-only">Actions</span></Table.Head>{/if}
					</Table.Row>
				</Table.Header>
				<Table.Body data-testid="entries" class="stagger">
					{#each rows as entry, index (entry.id)}
						<Table.Row class="relative" style="--i: {index}" data-selected={picked.includes(entry.id) || undefined}>
							{#if bulk}
								<Table.Cell class="w-10 pl-4">
									<input
										type="checkbox"
										class="relative z-10 size-4 accent-primary"
										aria-label="Pick {shownTitle(entry.revision.title)}"
										value={entry.id}
										bind:group={picked}
									/>
								</Table.Cell>
							{/if}
							<Table.Cell class="max-w-0 sm:max-w-md {bulk ? '' : 'pl-4'}">
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
							<Table.Cell class="text-right text-sm text-muted-foreground {writes ? '' : 'pr-4'}" title={new Date(entry.updated_at).toLocaleString()}>{ago(entry.updated_at)}</Table.Cell>
							{#if writes}
								<Table.Cell class="w-12 pr-4 text-right">
									<Button
										class="relative z-10"
										variant="ghost"
										size="icon-sm"
										aria-label="Duplicate {shownTitle(entry.revision.title)}"
										title="Duplicate"
										disabled={duplicating !== null}
										onclick={() => duplicate(entry.id)}
										data-testid="duplicate"><CopyIcon /></Button
									>
								</Table.Cell>
							{/if}
						</Table.Row>
					{:else}
						<Table.Row class="hover:bg-transparent">
							<Table.Cell colspan={4 + (bulk ? 1 : 0) + (writes ? 1 : 0)} class="py-10 text-center text-muted-foreground">
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
