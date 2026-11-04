<!--
  One migration plan (D-100, D-101): the definition a collection moves to, the entries the dry
  run named, and the actions that move it on — approve on the hash on screen, apply to start the
  run a worker drives, cancel while it hasn't run. A `needs_input` item takes a replacement
  document from a person, checked against the target before it lands. While it runs, the page
  follows the run; a blocked run says which entries stopped it.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createInfiniteQuery, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Label } from '@colophon/ui/ui/label';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import * as Table from '@colophon/ui/ui/table';
	import { Textarea } from '@colophon/ui/ui/textarea';
	import {
		ApiError,
		type MigrationItem,
		type MigrationOutcome,
		type MigrationPlan,
		applyMigration,
		approveMigration,
		cancelMigration,
		getEntry,
		getMigration,
		may,
		migrationItems,
		newKey,
		resolveMigrationItem
	} from '#lib/api.ts';
	import { plainError } from '#lib/errors.ts';
	import { ago, projectQuery, typesQuery } from '#lib/queries.ts';
	import MigrationBadge from '#lib/shell/MigrationBadge.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';

	const project = $derived(page.params.project ?? '');
	const id = $derived(page.params.migration ?? '');
	const queryClient = useQueryClient();
	const plan = createQuery(() => ({
		queryKey: ['migration', project, id],
		queryFn: () => getMigration(project, id),
		// While a run is under way the worker drives it: follow along.
		refetchInterval: (query) => (query.state.data?.state === 'running' ? 1000 : false)
	}));
	const mine = createQuery(() => projectQuery(project));
	const types = createQuery(() => typesQuery(project));
	const typeName = $derived(types.data?.find((type) => type.slug === plan.data?.collection)?.definition.name ?? plan.data?.collection ?? '');
	const changer = $derived(mine.data !== undefined && plan.data !== undefined && may(mine.data, 'schema.change', plan.data.collection));

	const FILTERS = [
		{ outcome: undefined, label: 'All' },
		{ outcome: 'needs_input', label: 'Needs input' },
		{ outcome: 'resolved', label: 'Resolved' },
		{ outcome: 'applied', label: 'Applied' },
		{ outcome: 'skipped', label: 'Skipped' },
		{ outcome: 'blocked', label: 'Blocked' }
	] as const;
	let outcome = $state<MigrationOutcome | undefined>(undefined);
	// Pages of 50, followed with the server's cursor, refiltered server-side.
	const items = createInfiniteQuery(() => ({
		queryKey: ['migration-items', project, id, outcome ?? 'all'],
		queryFn: ({ pageParam }: { pageParam: string | null }) => migrationItems(project, id, outcome, pageParam ?? undefined),
		initialPageParam: null as string | null,
		getNextPageParam: (last: { next_cursor: string | null }) => last.next_cursor,
		refetchInterval: plan.data?.state === 'running' ? 1000 : false
	}));
	const loaded = $derived((items.data?.pages ?? []).flatMap((one) => one.data));

	let notice = $state('');
	let error = $state('');
	let busy = $state(false);

	const STALE = 'This plan changed since you loaded it. Here is the current one: check it, then try again.';

	/** The words for a refused action on the plan on screen. */
	function refused(failure: unknown, doing: string): string {
		if (failure instanceof ApiError && failure.reason === 'plan_moved') return STALE;
		if (failure instanceof ApiError && failure.reason === 'plan_not_proposed') return 'This plan was approved already. Here is the current state.';
		if (failure instanceof ApiError && failure.reason === 'plan_not_approved') return 'This plan needs approving first. Here is the current state.';
		if (failure instanceof ApiError && failure.reason === 'cannot_cancel_running') return 'This plan is running and can’t be cancelled; let it finish.';
		if (failure instanceof ApiError && failure.reason === 'plan_finished') return 'This plan is already finished. Here is the current state.';
		return plainError(failure, doing);
	}

	async function refresh() {
		await Promise.all([
			queryClient.invalidateQueries({ queryKey: ['migration', project, id] }),
			queryClient.invalidateQueries({ queryKey: ['migration-items', project, id] }),
			queryClient.invalidateQueries({ queryKey: ['migrations', project] })
		]);
	}

	async function act(shown: MigrationPlan, action: 'approve' | 'apply' | 'cancel') {
		busy = true;
		error = '';
		notice = '';
		try {
			const next =
				action === 'approve'
					? await approveMigration(project, id, shown.hash, newKey())
					: action === 'apply'
						? await applyMigration(project, id, shown.hash, newKey()).then(async () => getMigration(project, id))
						: await cancelMigration(project, id, newKey());
			queryClient.setQueryData(['migration', project, id], next);
			await refresh();
		} catch (failure) {
			error = refused(failure, action === 'cancel' ? 'cancel this plan' : action === 'approve' ? 'approve this plan' : 'start this migration');
			await refresh();
		} finally {
			busy = false;
		}
	}

	// ---- The resolve dialog: a replacement document for one entry in the way. ----
	let resolving = $state<MigrationItem | null>(null);
	let document = $state('');
	let documentError = $state('');
	let documentBusy = $state(false);

	async function openResolve(item: MigrationItem) {
		resolving = item;
		document = '';
		documentError = '';
		// Start from the entry's current draft: the person fixes what doesn't fit rather than
		// rewriting it from nothing.
		try {
			const loaded = await getEntry(project, plan.data?.collection ?? '', item.entry_id);
			document = JSON.stringify(loaded.draft.content, null, 2);
		} catch (failure) {
			documentError = plainError(failure, 'load this entry');
		}
	}

	async function resolveItem() {
		const item = resolving;
		const shown = plan.data;
		if (item === null || shown === undefined) return;
		documentError = '';
		let resolution: unknown;
		try {
			resolution = JSON.parse(document);
		} catch (problem) {
			documentError = `That isn't JSON yet: ${problem instanceof Error ? problem.message : String(problem)}`;
			return;
		}
		documentBusy = true;
		try {
			await resolveMigrationItem(project, id, item.entry_id, resolution, shown.hash, newKey());
			resolving = null;
			await refresh();
		} catch (failure) {
			if (failure instanceof ApiError && failure.failure === 'refused') {
				documentError = failure.message;
			} else if (failure instanceof ApiError && (failure.reason === 'plan_not_open' || failure.reason === 'item_not_needs_input')) {
				documentError = refused(failure, 'resolve this entry');
				void refresh();
			} else {
				documentError = refused(failure, 'resolve this entry');
			}
		} finally {
			documentBusy = false;
		}
	}

	const STEPS = ['Proposed', 'Approved', 'Running', 'Done'] as const;
	function reached(shown: MigrationPlan): number {
		switch (shown.state) {
			case 'done':
				return 4;
			case 'running':
				return 3;
			case 'approved':
				return 2;
			default:
				return 1;
		}
	}

	const OUTCOME = {
		ok: { variant: 'live', text: 'Fits' },
		migrates: { variant: 'live', text: 'Moves cleanly' },
		needs_input: { variant: 'attention', text: 'Needs input' },
		resolved: { variant: 'pending', text: 'Resolved' },
		applied: { variant: 'outline', text: 'Applied' },
		skipped: { variant: 'draft', text: 'Skipped' },
		blocked: { variant: 'destructive', text: 'Blocked' }
	} as const;

	/** What the run's refusal on an item says, in words an editor can use. */
	function problem(item: MigrationItem): string {
		return item.problem?.message ?? '';
	}
</script>

<svelte:head><title>{typeName || 'Migration'} migration · Colophon Studio</title></svelte:head>

{#if plan.isPending}
	<Skeleton class="mb-8 h-10 w-1/2" />
	<Skeleton class="h-64 w-full rounded-xl" />
{:else if plan.isError}
	<Problem error={plan.error} what="this migration" />
{:else}
	{@const shown = plan.data}
	<PageHeader title="{typeName} migration">
		{#snippet meta()}
			<span data-testid="migration-status" data-state={shown.state} class="inline-flex flex-wrap items-center gap-2">
				<MigrationBadge state={shown.state} />
				<span>v{shown.base_version} → {shown.target_version === null ? 'not stamped yet' : `v${shown.target_version}`}</span>
				{#if shown.items.expected > 0}
					<span>· {shown.items.done}/{shown.items.expected} done{#if shown.items.blocked > 0}, {shown.items.blocked} in the way{/if}</span>
				{/if}
			</span>
		{/snippet}
		{#snippet description()}
			<p>
				A plan to change {typeName} while its entries stay readable: computed on version {shown.base_version}, it names {shown.items.expected}
				{shown.items.expected === 1 ? 'entry' : 'entries'}{#if shown.items.blocked > 0}, {shown.items.blocked} of which {shown.items.blocked === 1 ? 'needs' : 'need'} input{/if}.
			</p>
		{/snippet}
		{#snippet actions()}
			{#if changer && shown.state === 'proposed'}
				<Button variant="outline" onclick={() => act(shown, 'cancel')} disabled={busy}>Cancel plan</Button>
				<Button onclick={() => act(shown, 'approve')} disabled={busy}>Approve plan</Button>
			{:else if changer && shown.state === 'approved'}
				<Button variant="outline" onclick={() => act(shown, 'cancel')} disabled={busy}>Cancel plan</Button>
				<Button onclick={() => act(shown, 'apply')} disabled={busy}>Apply — start the migration</Button>
			{:else if shown.state === 'running'}
				<p class="max-w-xs text-sm text-muted-foreground" data-testid="migration-running">A worker is applying this plan; it can't be cancelled now.</p>
			{/if}
		{/snippet}
	</PageHeader>

	<ol class="mb-8 grid grid-cols-4 gap-2" aria-label="Progress">
		{#each STEPS as step, index (step)}
			<li class="grid gap-1.5">
				<span class="h-1 rounded-full {index < reached(shown) ? 'bg-foreground' : 'bg-border'}"></span>
				<span class="text-xs {index < reached(shown) ? 'text-foreground' : 'text-muted-foreground'}">{step}</span>
			</li>
		{/each}
	</ol>

	{#if shown.error}
		<div role="alert" class="mb-4 rounded-lg bg-destructive/10 px-4 py-3 text-sm text-destructive" data-testid="migration-error">
			The run stopped: {shown.error}
		</div>
	{/if}
	{#if shown.republish_required}
		<div class="mb-4 rounded-lg bg-attention-soft px-4 py-3 text-sm text-attention-ink" data-testid="migration-republish">
			The migration rewrote drafts that readers still see as they were. Publish {typeName}'s entries again to put the new shape live.
		</div>
	{/if}
	{#if notice}<p role="status" class="mb-4 rounded-lg bg-pending-soft px-4 py-3 text-sm text-pending-ink">{notice}</p>{/if}
	{#if error}<p role="alert" class="mb-4 rounded-lg bg-destructive/10 px-4 py-3 text-sm text-destructive" data-testid="plan-error">{error}</p>{/if}

	<section aria-labelledby="items-heading">
		<div class="mb-3 flex flex-wrap items-center gap-2">
			<h2 id="items-heading" class="text-base font-semibold">Entries <span class="font-normal text-muted-foreground tabular">{shown.items.expected}</span></h2>
			<div class="ml-auto flex flex-wrap gap-1" role="group" aria-label="Filter by outcome">
				{#each FILTERS as filter (filter.label)}
					<Button variant={outcome === filter.outcome ? 'secondary' : 'ghost'} size="xs" onclick={() => (outcome = filter.outcome)} data-testid="filter-{filter.outcome ?? 'all'}">{filter.label}</Button>
				{/each}
			</div>
		</div>
		{#if items.isPending}
			<Skeleton class="h-40 w-full rounded-xl" />
		{:else if items.isError}
			<Problem error={items.error} what="this plan's entries" />
		{:else if loaded.length === 0}
			<p class="rounded-xl border border-dashed border-border px-6 py-8 text-center text-sm text-muted-foreground" data-testid="items-empty">
				{shown.items.expected === 0 ? 'Nothing is in this migration’s way: every stored entry already fits the change.' : 'No entries with this outcome.'}
			</p>
		{:else}
			<div class="overflow-hidden rounded-xl border border-border bg-card">
				<Table.Root>
					<Table.Header>
						<Table.Row class="hover:bg-transparent">
							<Table.Head class="pl-4">Entry</Table.Head>
							<Table.Head>Outcome</Table.Head>
							<Table.Head class="hidden md:table-cell">Why</Table.Head>
							<Table.Head class="pr-4 text-right"><span class="sr-only">Actions</span></Table.Head>
						</Table.Row>
					</Table.Header>
					<Table.Body data-testid="migration-items" class="stagger">
						{#each loaded as item, index (item.entry_id)}
							{@const look = OUTCOME[item.outcome]}
							<Table.Row style="--i: {index}">
								<Table.Cell class="max-w-0 pl-4 sm:max-w-md">
									<a class="block truncate font-medium hover:underline" href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: shown.collection, entry: item.entry_id })}
										>{item.title ?? 'No title to show'}</a
									>
								</Table.Cell>
								<Table.Cell>
									<span class="inline-flex flex-wrap items-center gap-1.5">
										<Badge variant={look.variant} data-testid="item-outcome" data-outcome={item.outcome}>{look.text}</Badge>
										{#if item.republish_required}<Badge variant="attention" data-testid="item-republish">republish</Badge>{/if}
									</span>
								</Table.Cell>
								<Table.Cell class="hidden max-w-md text-sm text-muted-foreground md:table-cell">{problem(item)}</Table.Cell>
								<Table.Cell class="pr-4 text-right">
									{#if item.outcome === 'needs_input' && (shown.state === 'proposed' || shown.state === 'approved')}
										{#if changer}
											<Button variant="outline" size="xs" onclick={() => openResolve(item)} data-testid="resolve-item">Resolve</Button>
										{:else}
											<span class="text-xs text-muted-foreground">needs someone who may change this type</span>
										{/if}
									{/if}
								</Table.Cell>
							</Table.Row>
						{/each}
					</Table.Body>
				</Table.Root>
			</div>
			{#if items.hasNextPage}
				<div class="mt-4 text-center">
					<Button variant="outline" onclick={() => items.fetchNextPage()} disabled={items.isFetchingNextPage}>
						{items.isFetchingNextPage ? 'Loading more…' : 'Load more'}
					</Button>
				</div>
			{/if}
		{/if}
	</section>

	<TechnicalDetails class="mt-10">
		<p>Migration plan <span class="font-mono break-all">{shown.id}</span>, hash <span class="font-mono break-all">{shown.hash}</span></p>
		<p>Target definition <span class="font-mono break-all">{shown.target_definition_hash}</span> of <span class="font-mono">{shown.collection}</span>, from version {shown.base_version}{shown.target_version === null ? '' : ` to version ${shown.target_version}`}.</p>
		{#if shown.operations !== null}
			<p>Declared operations: <span class="font-mono break-all">{JSON.stringify(shown.operations)}</span></p>
		{/if}
		{#if shown.approved_by}<p>Approved by {shown.approved_by}{shown.approved_at ? `, ${ago(shown.approved_at)}` : ''}.</p>{/if}
		{#if shown.started_at}<p>Started {ago(shown.started_at)}{shown.finished_at ? `, finished ${ago(shown.finished_at)}` : ''}.</p>{/if}
	</TechnicalDetails>
{/if}

<Dialog.Root open={resolving !== null} onOpenChange={(open) => !open && (resolving = null)}>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-2xl">
		<Dialog.Header>
			<Dialog.Title>Resolve {resolving?.title ?? 'this entry'}</Dialog.Title>
			<Dialog.Description>
				The migration can’t carry this entry over as it is. Give the whole document as it should read under the new definition — it’s checked
				against it before it lands, and this exact document is what the run applies.
			</Dialog.Description>
		</Dialog.Header>
		{#if resolving !== null && plan.data !== undefined}
			{#if resolving.problem}
				<p class="rounded-lg bg-attention-soft px-3 py-2 text-sm text-attention-ink" data-testid="resolve-problem">{resolving.problem.message}</p>
			{/if}
			<form class="grid gap-4" onsubmit={(event) => { event.preventDefault(); void resolveItem(); }} aria-label="Resolve this entry">
				<div class="grid gap-1.5">
					<Label for="resolution">Replacement document (JSON)</Label>
					<Textarea id="resolution" bind:value={document} rows={12} class="font-mono text-xs" data-testid="resolution" />
				</div>
				{#if documentError}<p role="alert" class="text-sm text-destructive" data-testid="resolution-error">{documentError}</p>{/if}
				<Dialog.Footer>
					<Button variant="outline" type="button" onclick={() => (resolving = null)}>Keep it in the way</Button>
					<Button type="submit" disabled={documentBusy || document === ''} data-testid="resolve-submit">{documentBusy ? 'Checking…' : 'Resolve this entry'}</Button>
				</Dialog.Footer>
			</form>
		{/if}
	</Dialog.Content>
</Dialog.Root>
