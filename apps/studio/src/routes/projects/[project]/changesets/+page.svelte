<!--
  A project's publish requests (changesets, D-044; the words are D-059's): what each is waiting
  for, and a dialog to gather several entries into a new one. A request holds the exact versions
  it was made from; reviews approve exactly those. Each picked entry publishes its current
  version; a live one can instead be taken down (D-049), with one Publish | Unpublish control on
  its row. The dialog stays inside the screen however long a title is (the audit's P1-3, F5): its
  body scrolls, its footer stays put, and its table is laid out fixed, with long titles cut to
  one line and said in full on hover. One entry is published from its editor.
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
	import * as ToggleGroup from '@colophon/ui/ui/toggle-group';
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
	/** The entries in it, and what it does with each live one: publish its current version
	 *  (the default) or take it down. A draft can only be published. */
	let picked = $state<string[]>([]);
	let unpublishing = $state<string[]>([]);
	const live = $derived(new Set((choices.data ?? []).flatMap((group) => group.entries.filter((entry) => entry.published !== null).map((entry) => entry.id))));
	const takenDown = $derived(picked.filter((entry) => unpublishing.includes(entry) && live.has(entry)));
	const publishing = $derived(picked.filter((entry) => !takenDown.includes(entry)));
	function choose(entry: string, kind: string) {
		if (kind === 'unpublish') unpublishing = [...new Set([...unpublishing, entry])];
		else if (kind === 'publish') unpublishing = unpublishing.filter((one) => one !== entry);
		if (!picked.includes(entry)) picked = [...picked, entry];
	}
	let error = $state('');
	let busy = $state(false);
	// One key per changeset being made: pressing Create again after a lost answer is safe.
	let key = newKey();

	async function create(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		try {
			const made = await createChangeset(project, title, publishing, takenDown, key);
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
	{#snippet description()}Group entries, get them approved, and publish them together as one release.{/snippet}
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
	<Dialog.Content class="sm:max-w-[45rem]" data-testid="new-request">
		<Dialog.Header>
			<Dialog.Title>New publish request</Dialog.Title>
			<Dialog.Description>It holds the version of each entry you pick as it is now; later edits wait until you update it.</Dialog.Description>
		</Dialog.Header>
		<form class="grid min-w-0 gap-5" onsubmit={create} aria-label="New publish request">
			<div class="grid gap-1.5">
				<Label for="request-title">Title</Label>
				<Input id="request-title" bind:value={title} required maxlength={200} placeholder="Spring launch" />
			</div>
			{#if choices.isPending}
				<Skeleton class="h-32 w-full rounded-lg" />
			{:else if choices.isError}
				<Problem error={choices.error} />
			{:else}
				<div class="grid min-w-0 gap-2">
					<div class="flex flex-wrap items-center justify-between gap-3">
						<p class="text-sm font-medium">
							Entries <span class="font-normal text-muted-foreground tabular" data-testid="picked-count"
								>{picked.length} picked{#if takenDown.length > 0}, {takenDown.length} to take down{/if}</span
							>
						</p>
						<Input class="h-9 w-full sm:w-56" type="search" placeholder="Filter" aria-label="Filter entries" bind:value={filter} />
					</div>
					{#each choices.data as group (group.type.slug)}
						{@const shown = group.entries.filter((entry) => entry.revision.title.toLowerCase().includes(filter.trim().toLowerCase()))}
						{#if shown.length > 0}
							<div class="min-w-0 overflow-hidden rounded-lg border border-border">
								<table class="w-full table-fixed text-sm" data-testid="request-entries">
									<caption class="border-b border-border bg-muted/50 px-3 py-1.5 text-left text-xs font-medium text-muted-foreground">{group.type.definition.name}</caption>
									<colgroup>
										<col class="w-10" />
										<col />
										<col class="w-36 max-sm:w-0" />
										<col class="w-[11.5rem] max-sm:w-0" />
									</colgroup>
									<thead class="sr-only">
										<tr><th scope="col">Pick</th><th scope="col">Entry</th><th scope="col">Now</th><th scope="col">In this request</th></tr>
									</thead>
									<tbody>
										{#each shown as entry (entry.id)}
											{@const standing = entryState(entry)}
											{@const isLive = entry.published !== null}
											{@const kind = unpublishing.includes(entry.id) && isLive ? 'unpublish' : 'publish'}
											<tr class="rule-dotted last:border-b-0" data-testid="request-entry" data-entry={entry.id}>
												<td class="py-2.5 pl-3 align-top">
													<input
														type="checkbox"
														class="mt-0.5 size-4"
														value={entry.id}
														bind:group={picked}
														aria-label={entry.revision.title}
														data-testid="request-entry-pick"
													/>
												</td>
												<td class="py-2.5 pr-3 align-top">
													<span class="block truncate" title={entry.revision.title} data-testid="request-entry-title" data-user-content>{entry.revision.title}</span>
													<!-- On a phone the state and the choice sit under the title. -->
													<span class="mt-2 flex flex-wrap items-center gap-2 sm:hidden">
														<StatusBadge state={standing} generation={entry.published?.generation ?? null} />
														{#if isLive}{@render segmented(entry.id, entry.revision.title, kind)}{/if}
													</span>
												</td>
												<td class="py-2 pr-3 align-top max-sm:hidden"><StatusBadge state={standing} generation={entry.published?.generation ?? null} /></td>
												<td class="py-1.5 pr-3 align-top max-sm:hidden">
													{#if isLive}{@render segmented(entry.id, entry.revision.title, kind)}{/if}
												</td>
											</tr>
										{/each}
									</tbody>
								</table>
							</div>
						{/if}
					{:else}
						<p class="text-sm text-muted-foreground">Nothing is written yet: write an entry first, then come back to publish it.</p>
					{/each}
				</div>
			{/if}
			{#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
			<Dialog.Footer>
				<Button variant="outline" type="button" onclick={() => (creating = false)}>Cancel</Button>
				<Button type="submit" disabled={busy || picked.length === 0} data-testid="create-request">Create publish request</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<!-- A live entry's one choice: publish its current version, or take it down. Choosing either picks it. -->
{#snippet segmented(entry: string, name: string, kind: string)}
	<ToggleGroup.Root
		type="single"
		variant="outline"
		size="sm"
		class="w-[10.5rem]"
		value={kind}
		onValueChange={(value: string) => value !== '' && choose(entry, value)}
		aria-label="What this request does with {name}"
		data-testid="request-entry-kind"
	>
		<ToggleGroup.Item value="publish" class="flex-1" aria-label="Publish {name}">Publish</ToggleGroup.Item>
		<ToggleGroup.Item value="unpublish" class="flex-1" aria-label="Unpublish {name}">Unpublish</ToggleGroup.Item>
	</ToggleGroup.Root>
{/snippet}
