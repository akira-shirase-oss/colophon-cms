<!--
  A project's publish requests (changesets, D-044; the words are D-059's): what each is waiting
  for, and a dialog to gather several entries into a new one. A request holds the exact versions
  it was made from; reviews approve exactly those. An entry can be picked to publish its current
  version or to take its published one down (D-049). One entry is published from its editor.
-->
<script lang="ts">
	import LayersIcon from '@lucide/svelte/icons/layers';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Input } from '@colophon/ui/ui/input';
	import { Label } from '@colophon/ui/ui/label';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import * as Table from '@colophon/ui/ui/table';
	import { ApiError, createChangeset, entryState, listContentTypes, listEntries, may, newKey } from '#lib/api.ts';
	import { ago, changesetsQuery, projectQuery, typesQuery } from '#lib/queries.ts';
	import ChangesetBadge from '#lib/shell/ChangesetBadge.svelte';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import StatusBadge from '#lib/shell/StatusBadge.svelte';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';

	const project = $derived(page.params.project ?? '');
	const sets = createQuery(() => changesetsQuery(project));
	const mine = createQuery(() => projectQuery(project));
	const types = createQuery(() => typesQuery(project));
	// Making a request needs edit rights where its entries are; the server checks each one.
	const makes = $derived(mine.data === undefined || may(mine.data, 'content.update'));
	const firstType = $derived(types.data?.find((type) => type.slug !== 'articles') ?? types.data?.[0]);
	let creating = $state(false);
	// Every entry, by content type, to pick from: loaded when the dialog opens.
	const choices = createQuery(() => ({
		queryKey: ['changeset-choices', project],
		queryFn: async () => {
			// Only collections the role may read; the server refuses the others' lists.
			const types = (await listContentTypes(project)).filter((type) => may(mine.data, 'content.read', type.slug));
			return Promise.all(types.map(async (type) => ({ type, entries: await listEntries(project, type.slug) })));
		},
		enabled: creating && mine.data !== undefined
	}));

	let title = $state('');
	let filter = $state('');
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
			creating = false;
			await goto(resolve('/projects/[project]/changesets/[changeset]', { project, changeset: made.id }));
		} catch (failure) {
			error = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>Publish requests · Colophon Studio</title></svelte:head>

<PageHeader title="Publish requests">
	{#snippet description()}
		A publish request holds exact versions of entries. Someone approves exactly those versions, and publishing makes them the next release. To publish
		one entry, press Publish in its editor; here you can gather several into one request.
	{/snippet}
	{#snippet actions()}
		{#if makes}<Button onclick={() => (creating = true)}><PlusIcon data-icon="inline-start" />New publish request</Button>{/if}
	{/snippet}
</PageHeader>

{#if sets.isPending}
	<Skeleton class="h-48 w-full rounded-xl" />
{:else if sets.isError}
	<Problem error={sets.error} />
{:else if sets.data.length === 0}
	<EmptyState icon={LayersIcon} title="No publish requests yet" testid="requests-empty">
		This is how changes reach your website. Press Publish (or Ask for review) on an entry, or gather several entries into one request here.
		{#snippet action()}
			{#if firstType}
				<Button variant="outline" href={resolve('/projects/[project]/[collection]', { project, collection: firstType.slug })}>Go to {firstType.definition.name}</Button>
			{/if}
		{/snippet}
	</EmptyState>
{:else}
	<div class="overflow-hidden rounded-xl border border-border bg-card">
		<Table.Root>
			<Table.Header>
				<Table.Row class="hover:bg-transparent">
					<Table.Head class="pl-4">Title</Table.Head>
					<Table.Head>Status</Table.Head>
					<Table.Head class="hidden sm:table-cell">Entries</Table.Head>
					<Table.Head class="pr-4 text-right">Updated</Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body data-testid="changesets" class="stagger">
				{#each sets.data as set, index (set.id)}
					<Table.Row class="relative" style="--i: {index}">
						<Table.Cell class="max-w-0 pl-4 sm:max-w-md">
							<a class="block truncate font-medium after:absolute after:inset-0" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: set.id })}
								>{set.title}</a
							>
						</Table.Cell>
						<Table.Cell><ChangesetBadge state={set.status.state} /></Table.Cell>
						<Table.Cell class="hidden text-sm text-muted-foreground sm:table-cell">
							{set.items} {set.items === 1 ? 'entry' : 'entries'}{#if set.unpublish > 0}, {set.unpublish} to unpublish{/if}
						</Table.Cell>
						<Table.Cell class="pr-4 text-right text-sm text-muted-foreground">{ago(set.updated_at)}</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	</div>
{/if}

<TechnicalDetails class="mt-8">
	<p>The API and the CLI call publish requests <em>changesets</em>: each pins exact revisions and is identified by the SHA-256 of them.</p>
	<p class="font-mono break-all">/api/v1/projects/{project}/environments/production/changesets</p>
</TechnicalDetails>

<Dialog.Root bind:open={creating}>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-2xl">
		<Dialog.Header>
			<Dialog.Title>New publish request</Dialog.Title>
			<Dialog.Description>
				It holds the current version of each entry you pick to publish, and the live version of each entry you pick to unpublish. Later edits
				don't change it until you update it.
			</Dialog.Description>
		</Dialog.Header>
		<form class="grid gap-5" onsubmit={create} aria-label="New changeset">
			<div class="grid gap-1.5">
				<Label for="request-title">Title</Label>
				<Input id="request-title" bind:value={title} required maxlength={200} placeholder="Spring launch" />
			</div>
			{#if choices.isPending}
				<Skeleton class="h-32 w-full rounded-lg" />
			{:else if choices.isError}
				<Problem error={choices.error} />
			{:else}
				<div class="grid gap-2">
					<div class="flex items-center justify-between gap-3">
						<p class="text-sm font-medium">Entries <span class="font-normal text-muted-foreground">{picked.length + takenDown.length} picked</span></p>
						<Input class="h-8 max-w-56" type="search" placeholder="Filter" aria-label="Filter entries" bind:value={filter} />
					</div>
					{#each choices.data as group (group.type.slug)}
						{@const shown = group.entries.filter((entry) => entry.revision.title.toLowerCase().includes(filter.trim().toLowerCase()))}
						{#if shown.length > 0}
							<fieldset class="overflow-hidden rounded-lg border border-border">
								<legend class="sr-only">{group.type.definition.name}</legend>
								<p class="border-b border-border bg-muted/50 px-3 py-1.5 text-xs font-medium text-muted-foreground" aria-hidden="true">{group.type.definition.name}</p>
								{#each shown as entry (entry.id)}
									{@const standing = entryState(entry)}
									<!-- Picking an entry publishes its current version; Unpublish takes its published one down. -->
									<div class="rule-dotted flex items-center gap-3 px-3 py-2 last:border-b-0">
										<label class="flex min-w-0 flex-1 items-center gap-2.5 text-sm">
											<input type="checkbox" class="size-4 accent-primary" value={entry.id} bind:group={picked} disabled={takenDown.includes(entry.id)} />
											<span class="truncate">{entry.revision.title}</span>
										</label>
										<StatusBadge state={standing} generation={entry.published?.generation ?? null} />
										<label class="flex items-center gap-1.5 text-xs text-muted-foreground" class:invisible={standing === 'draft'}>
											<input
												type="checkbox"
												class="size-3.5 accent-destructive"
												value={entry.id}
												bind:group={takenDown}
												disabled={picked.includes(entry.id) || standing === 'draft'}
												aria-label={`Unpublish ${entry.revision.title}`}
											/> Unpublish
										</label>
									</div>
								{/each}
							</fieldset>
						{/if}
					{:else}
						<p class="text-sm text-muted-foreground">Nothing is written yet: write an entry first, then come back to publish it.</p>
					{/each}
				</div>
			{/if}
			{#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
			<Dialog.Footer>
				<Button variant="outline" type="button" onclick={() => (creating = false)}>Cancel</Button>
				<Button type="submit" disabled={busy || picked.length + takenDown.length === 0}>Create publish request</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
