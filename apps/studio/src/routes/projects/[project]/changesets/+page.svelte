<!--
  A project's changesets (D-044): what each is waiting for, and a dialog to pin entries into a new
  one. A changeset holds the exact revisions it was made from; reviews approve exactly those. An
  entry can be picked to publish its current version or to take its published one down (D-049).
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
	import { ApiError, createChangeset, entryState, listContentTypes, listEntries, newKey } from '#lib/api.ts';
	import { ago, changesetsQuery } from '#lib/queries.ts';
	import ChangesetBadge from '#lib/shell/ChangesetBadge.svelte';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import StatusBadge from '#lib/shell/StatusBadge.svelte';

	const project = $derived(page.params.project ?? '');
	const sets = createQuery(() => changesetsQuery(project));
	let creating = $state(false);
	// Every entry, by content type, to pick from: loaded when the dialog opens.
	const choices = createQuery(() => ({
		queryKey: ['changeset-choices', project],
		queryFn: async () => {
			const types = await listContentTypes(project);
			return Promise.all(types.map(async (type) => ({ type, entries: await listEntries(project, type.slug) })));
		},
		enabled: creating
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

<svelte:head><title>Changesets · Colophon Studio</title></svelte:head>

<PageHeader title="Changesets">
	{#snippet description()}
		A changeset pins exact versions of entries. Someone reviews exactly those versions, and publishing makes them the next release.
	{/snippet}
	{#snippet actions()}
		<Button onclick={() => (creating = true)}><PlusIcon data-icon="inline-start" />New changeset</Button>
	{/snippet}
</PageHeader>

{#if sets.isPending}
	<Skeleton class="h-48 w-full rounded-xl" />
{:else if sets.isError}
	<Problem error={sets.error} />
{:else if sets.data.length === 0}
	<EmptyState icon={LayersIcon} title="No changesets yet">Pick the entries you want to publish, and send them for review.</EmptyState>
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

<Dialog.Root bind:open={creating}>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-2xl">
		<Dialog.Header>
			<Dialog.Title>New changeset</Dialog.Title>
			<Dialog.Description>
				It pins the current version of each entry you pick to publish, and the published version of each entry you pick to unpublish.
				Later edits don't change it until you refresh it.
			</Dialog.Description>
		</Dialog.Header>
		<form class="grid gap-5" onsubmit={create} aria-label="New changeset">
			<div class="grid gap-1.5">
				<Label for="changeset-title">Title</Label>
				<Input id="changeset-title" bind:value={title} required maxlength={200} placeholder="Spring launch" />
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
						<p class="text-sm text-muted-foreground">No entries to pick yet.</p>
					{/each}
				</div>
			{/if}
			{#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
			<Dialog.Footer>
				<Button variant="outline" type="button" onclick={() => (creating = false)}>Cancel</Button>
				<Button type="submit" disabled={busy || picked.length + takenDown.length === 0}>Create changeset</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
