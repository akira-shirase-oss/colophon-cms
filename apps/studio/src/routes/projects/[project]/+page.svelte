<!--
  A project at a glance: what readers have now, what's waiting for a reviewer, where the content
  stands, and what changed last. Every number links to the page that explains it, and a number
  still loading shows as a placeholder, never as a zero or "Never" it doesn't know yet. Entries
  are counted by lib/entryState.ts, as the lists and the sidebar count them: live includes live
  with unpublished changes.
-->
<script lang="ts">
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import PenLineIcon from '@lucide/svelte/icons/pen-line';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQueries, createQuery } from '@tanstack/svelte-query';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { may, type Summary } from '#lib/api.ts';
	import { entryState, tally as tallyOf, tallyText } from '#lib/entryState.ts';
	import { mainCollection, oneOf } from '#lib/firstRun.ts';
	import FirstRun from '#lib/FirstRun.svelte';
	import { ago, changesetsQuery, count as countOf, entriesQuery, meQuery, projectQuery, releasesQuery, typesQuery } from '#lib/queries.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import ChangesetBadge from '#lib/shell/ChangesetBadge.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import ReleaseLabel from '#lib/shell/ReleaseLabel.svelte';
	import StatusBadge from '#lib/shell/StatusBadge.svelte';
	import { count, PUBLISH_REQUEST, PUBLISH_REQUESTS, releaseName, shownTitle } from '#lib/words.ts';

	const project = $derived(page.params.project ?? '');
	const who = createQuery(meQuery);
	const mine = createQuery(() => projectQuery(project));
	const types = createQuery(() => typesQuery(project));
	const sets = createQuery(() => changesetsQuery(project));
	const releases = createQuery(() => releasesQuery(project));
	// Only the collections this role may read: the server refuses the others' lists (D-058), and
	// waiting for an answer that will never be a list kept this page "Loading…" for good.
	const readable = $derived(mine.data === undefined ? [] : (types.data ?? []).filter((type) => may(mine.data, 'content.read', type.slug)));
	const lists = createQueries(() => ({ queries: readable.map((type) => entriesQuery(project, type.slug)) }));

	const live = $derived(releases.data?.releases.find((release) => release.generation === releases.data?.generation));
	const waiting = $derived(
		(sets.data ?? []).filter((set) => ['awaiting_review', 'changes_requested', 'approved', 'stale'].includes(set.status.state))
	);
	const all = $derived(
		readable.flatMap((type, index) => (lists[index]?.data?.data ?? []).map((entry: Summary) => ({ type, entry })))
	);
	// Each list holds the newest 100; when any has more, the totals say "at least".
	const more = $derived(lists.some((list) => list.data?.next_cursor != null));
	const tally = $derived(tallyOf(all.map(({ entry }) => entry)));
	const newest = $derived([...all].sort((a, b) => b.entry.updated_at.localeCompare(a.entry.updated_at)));
	const recent = $derived(newest.slice(0, 8));
	// Each query's data and error are read before any is tested: svelte-query only updates what has
	// been read, so a `.data` first read behind a short-circuit can stay empty for good (see
	// ItemDiff.svelte). A list that failed is settled too: its problem is shown, not waited for.
	const settled = $derived(lists.map((list) => list.data !== undefined || list.isError));
	const failed = $derived(readable.filter((_, index) => lists[index]?.isError === true));
	const firstError = $derived(lists.find((list) => list.isError)?.error ?? null);
	// Each section waits only for what it shows: the entries for the recent edits, everything for
	// the checklist.
	const entriesSettled = $derived(
		[mine.data !== undefined || mine.isError, types.data !== undefined || types.isError, ...settled].every(Boolean)
	);
	// The tile counts every readable list, so it waits for all of them, as the checklist does.
	const counted = $derived(entriesSettled && failed.length === 0);
	const answers = $derived([mine.data, who.data, sets.data, releases.data, types.data]);
	const loaded = $derived(answers.every((answer) => answer !== undefined) && entriesSettled);
	// Where a first entry goes: the first collection the project made (Pages), else Articles.
	const main = $derived(mainCollection(readable));
</script>

<svelte:head><title>{mine.data?.name ?? 'Overview'} · Colophon Studio</title></svelte:head>

<PageHeader title={mine.data?.name ?? 'Overview'}>
	{#snippet meta()}
		{#if mine.data}
			<span>{mine.data.slug}</span>
			<span aria-hidden="true">·</span>
			<span>{mine.data.role_name}</span>
		{/if}
	{/snippet}
</PageHeader>

{#if mine.isError}
	<Problem error={mine.error} />
{:else}
	{#if loaded && mine.data && who.data && sets.data && releases.data}
		{#key project}
			<FirstRun
				{project}
				identity={who.data.id}
				mine={mine.data}
				types={readable}
				entries={newest}
				requests={sets.data}
				releases={releases.data.releases}
			/>
		{/key}
	{/if}
	<section class="stagger grid gap-3 sm:grid-cols-2 xl:grid-cols-3" aria-label="At a glance" data-testid="at-a-glance">
		<a class="stat lift" style="--i: 0" href={live ? resolve('/projects/[project]/releases/[generation]', { project, generation: String(live.generation) }) : resolve('/projects/[project]/releases', { project })} data-testid="live-tile">
			<span class="stat-label">Live on your site</span>
			{#if releases.isPending}
				<span class="stat-value" aria-busy="true"><Skeleton class="h-7 w-10" data-testid="loading-figure" /><span class="sr-only">Loading</span></span>
				<Skeleton class="h-4 w-40" />
			{:else if releases.isError}
				<span class="stat-value">Unknown</span>
				<span class="text-xs text-destructive" data-testid="releases-problem">Couldn't check what's live</span>
			{:else if live}
				<span class="stat-value tabular">{releaseName(live.generation)}</span>
				<span class="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
					<ReleaseLabel release={live} testid="overview-label" />
					<span title={new Date(live.published_at).toLocaleString()}>{ago(live.published_at)}, by {live.published_by_name ?? live.published_by}</span>
				</span>
			{:else}
				<span class="stat-value">Nothing yet</span>
				<span class="text-xs text-muted-foreground">Nothing is published</span>
			{/if}
		</a>
		<a class="stat lift" style="--i: 1" href={resolve('/projects/[project]/changesets', { project })} data-testid="waiting-tile">
			<span class="stat-label">Waiting for review</span>
			{#if sets.isError}
				<span class="stat-value">Unknown</span>
				<span class="text-xs text-destructive">Couldn't load {PUBLISH_REQUESTS}</span>
			{:else if sets.isPending}
				<span class="stat-value" aria-busy="true"><Skeleton class="h-7 w-10" data-testid="loading-figure" /><span class="sr-only">Loading</span></span>
				<Skeleton class="h-4 w-36" />
			{:else}
				<span class="stat-value tabular" data-testid="awaiting-count">{sets.data.filter((set) => set.status.state === 'awaiting_review').length}</span>
				<span class="text-xs text-muted-foreground">{count(waiting.length, PUBLISH_REQUEST, PUBLISH_REQUESTS)} in progress</span>
			{/if}
		</a>
		<div class="stat" style="--i: 2" data-testid="entries-tile">
			<span class="stat-label">Entries</span>
			{#if failed.length > 0}
				<span class="stat-value">Unknown</span>
				<span class="text-xs text-destructive">Couldn't count {failed.map((type) => type.definition.name).join(', ')}</span>
			{:else if !counted}
				<span class="stat-value" aria-busy="true"><Skeleton class="h-7 w-10" data-testid="loading-figure" /><span class="sr-only">Loading</span></span>
				<Skeleton class="h-4 w-44" />
			{:else}
				<span class="stat-value tabular" data-testid="entry-count">{tally.total.toLocaleString()}{more ? '+' : ''}</span>
				<span class="flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted-foreground" data-testid="entry-tally" title={tallyText(tally, more)}>
					<span
						><span class="tally-dot bg-live ring-live-ring" aria-hidden="true"></span>{tally.live.toLocaleString()}{more ? '+' : ''} live{#if tally.withChanges > 0}&nbsp;({tally.withChanges.toLocaleString()}{more ? '+' : ''} with unpublished changes){/if}</span
					>
					<span><span class="tally-dot bg-draft ring-draft-ring" aria-hidden="true"></span>{tally.drafts.toLocaleString()}{more ? '+' : ''} {tally.drafts === 1 && !more ? 'draft' : 'drafts'}</span>
				</span>
			{/if}
		</div>
	</section>

	<div class="mt-10 grid gap-10 lg:grid-cols-[minmax(0,1fr)_20rem]">
		<section aria-labelledby="recent">
			<div class="mb-3 flex items-baseline justify-between">
				<h2 id="recent" class="text-base font-semibold">Recently edited</h2>
			</div>
			{#if failed.length > 0}
				<div class="mb-3" data-testid="lists-problem">
					<Problem error={firstError ?? new Error('')} what={failed.map((type) => type.definition.name).join(', ')} retry={() => lists.forEach((list) => list.isError && list.refetch())} />
				</div>
			{/if}
			<div class="stagger overflow-hidden rounded-xl border border-border bg-card" class:hidden={recent.length === 0}>
				{#each recent as { type, entry }, index (entry.id)}
					<a
						class="rule-dotted flex items-center gap-3 px-4 py-3 transition-colors last:border-b-0 hover:bg-muted/50"
						style="--i: {index}"
						href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: type.slug, entry: entry.id })}
					>
						<FileTextIcon class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
						<span class="min-w-0 flex-1">
							<span class="block truncate text-sm font-medium">{shownTitle(entry.revision.title)}</span>
							<span class="text-xs text-muted-foreground">{type.definition.name} · {ago(entry.updated_at)}</span>
						</span>
						<StatusBadge state={entryState(entry)} generation={entry.published?.generation ?? null} />
					</a>
				{/each}
			</div>
			{#if recent.length === 0 && failed.length === 0}
				{#if !entriesSettled}
					<!-- The shape of the list it will be, not a word: rows of a title and a line. -->
					<div class="overflow-hidden rounded-xl border border-border bg-card" aria-busy="true" data-testid="recent-loading">
						<span class="sr-only">Loading the recent edits</span>
						{#each [0, 1, 2, 3] as row (row)}
							<div class="rule-dotted flex items-center gap-3 px-4 py-3 last:border-b-0">
								<Skeleton class="size-4 shrink-0" />
								<span class="grid flex-1 gap-1.5"><Skeleton class="h-4 w-2/3" /><Skeleton class="h-3 w-1/4" /></span>
								<Skeleton class="h-6 w-16 rounded-full" />
							</div>
						{/each}
					</div>
				{:else}
					<EmptyState icon={PenLineIcon} title="Nothing written yet" testid="overview-empty">
						Everything written here appears in this list, newest first.
						{#snippet action()}
							{#if main && may(mine.data, 'content.create', main.slug)}
								<Button href={resolve('/projects/[project]/[collection]', { project, collection: main.slug }) + '?new=1'}>Write the first {oneOf(main.definition.name)}</Button>
							{/if}
						{/snippet}
					</EmptyState>
				{/if}
			{/if}
		</section>

		<div class="space-y-10">
			<section aria-labelledby="waiting">
				<h2 id="waiting" class="mb-3 text-base font-semibold">In review</h2>
				{#if sets.isError}
					<Problem error={sets.error} what={PUBLISH_REQUESTS} retry={() => sets.refetch()} />
				{:else if sets.isPending}
					<Skeleton class="h-16 w-full rounded-lg" />
				{:else}
				<ul class="space-y-2">
					{#each waiting.slice(0, 5) as set (set.id)}
						<li>
							<a class="lift block rounded-lg border border-border bg-card p-3 hover:border-foreground/20" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: set.id })}>
								<span class="block truncate text-sm font-medium">{set.title}</span>
								<span class="mt-1.5 flex items-center justify-between gap-2">
									<ChangesetBadge state={set.status.state} />
									<span class="text-xs text-muted-foreground">{set.items} {set.items === 1 ? 'entry' : 'entries'}</span>
								</span>
							</a>
						</li>
					{:else}
						<li class="text-sm text-muted-foreground">Nothing is waiting for review. Publish requests appear here until they go live.</li>
					{/each}
				</ul>
				{/if}
				{#if releases.isError}
					<div class="mt-4"><Problem error={releases.error} what="releases" retry={() => releases.refetch()} /></div>
				{/if}
			</section>

			<section aria-labelledby="content-heading">
				<h2 id="content-heading" class="mb-3 text-base font-semibold">Content</h2>
				{#if types.isError}
					<Problem error={types.error} />
				{:else}
					<ul class="space-y-1" data-testid="content-types">
						{#each readable as type, index (type.slug)}
							<li>
								<a class="group flex items-center gap-3 rounded-lg px-2 py-2 hover:bg-muted" href={resolve('/projects/[project]/[collection]', { project, collection: type.slug })}>
									<span class="min-w-0 flex-1">
										<span class="block text-sm font-medium">{type.definition.name}</span>
										<span class="text-xs text-muted-foreground">{type.definition.fields.length} fields</span>
									</span>
									{#if lists[index]?.data}<Badge variant="secondary" class="tabular">{countOf(lists[index].data)}</Badge>{/if}
									<ArrowRightIcon class="size-4 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" aria-hidden="true" />
								</a>
							</li>
						{/each}
					</ul>
				{/if}
			</section>
		</div>
	</div>
{/if}

<style>
	/* The design system's card (D-182): its padding and radius tokens, a hairline, flat. */
	.stat {
		display: flex;
		flex-direction: column;
		gap: 0.375rem;
		padding: var(--card-padding-sm) var(--card-padding);
		border: 1px solid var(--border);
		border-radius: var(--radius-card);
		background: var(--card);
		align-items: flex-start;
	}
	a.stat:hover {
		border-color: color-mix(in oklch, var(--foreground) 20%, transparent);
	}
	/* A status mark as on Colophon's site: a dot in a soft ring of its own colour. */
	.tally-dot {
		display: inline-block;
		width: 0.5rem;
		height: 0.5rem;
		margin-right: 0.5rem;
		border-radius: 9999px;
		vertical-align: middle;
		/* The ring's colour comes from the `ring-*-ring` class beside it. */
		box-shadow: 0 0 0 3px var(--tw-ring-color);
	}
	.stat-label {
		font-size: 0.875rem;
		color: var(--muted-foreground);
	}
	.stat-value {
		font-family: var(--font-display);
		font-size: 1.75rem;
		line-height: 2.25rem;
		font-weight: 560;
		letter-spacing: -0.015em;
		font-variant-numeric: tabular-nums;
	}
</style>
