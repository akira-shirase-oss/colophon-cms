<!--
  A project at a glance: what readers have now, what's waiting for a reviewer, where the content
  stands, and what changed last. Every number links to the page that explains it.
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
	import { entryState, may, type Summary } from '#lib/api.ts';
	import { oneOf } from '#lib/firstRun.ts';
	import FirstRun from '#lib/FirstRun.svelte';
	import { ago, changesetsQuery, entriesQuery, meQuery, projectQuery, releasesQuery, typesQuery } from '#lib/queries.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import ChangesetBadge from '#lib/shell/ChangesetBadge.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import ReleaseLabel from '#lib/shell/ReleaseLabel.svelte';
	import StatusBadge from '#lib/shell/StatusBadge.svelte';
	import { count, PUBLISH_REQUEST, PUBLISH_REQUESTS, releaseName } from '#lib/words.ts';

	const project = $derived(page.params.project ?? '');
	const who = createQuery(meQuery);
	const mine = createQuery(() => projectQuery(project));
	const types = createQuery(() => typesQuery(project));
	const sets = createQuery(() => changesetsQuery(project));
	const releases = createQuery(() => releasesQuery(project));
	const lists = createQueries(() => ({ queries: (types.data ?? []).map((type) => entriesQuery(project, type.slug)) }));

	const live = $derived(releases.data?.releases.find((release) => release.generation === releases.data?.generation));
	const waiting = $derived(
		(sets.data ?? []).filter((set) => ['awaiting_review', 'changes_requested', 'approved', 'stale'].includes(set.status.state))
	);
	const all = $derived(
		(types.data ?? []).flatMap((type, index) => (lists[index]?.data?.data ?? []).map((entry: Summary) => ({ type, entry })))
	);
	const tally = $derived(
		all.reduce(
			(counts, { entry }) => {
				counts[entryState(entry)] += 1;
				return counts;
			},
			{ draft: 0, live: 0, changed: 0 }
		)
	);
	const newest = $derived([...all].sort((a, b) => b.entry.updated_at.localeCompare(a.entry.updated_at)));
	const recent = $derived(newest.slice(0, 8));
	const loaded = $derived(types.data !== undefined && lists.every((list) => list.data !== undefined));
	// Where a first entry goes: a type the project added (Pages), else Articles.
	const main = $derived((types.data ?? []).find((type) => type.slug !== 'articles') ?? types.data?.[0]);
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
	{#if mine.data && who.data && loaded && sets.data && releases.data}
		{#key project}
			<FirstRun
				{project}
				identity={who.data.id}
				mine={mine.data}
				types={types.data ?? []}
				entries={newest}
				requests={sets.data}
				releases={releases.data.releases}
			/>
		{/key}
	{/if}
	<section class="stagger grid gap-3 sm:grid-cols-2 xl:grid-cols-4" aria-label="At a glance">
		<a class="stat lift" style="--i: 0" href={live ? resolve('/projects/[project]/releases/[generation]', { project, generation: String(live.generation) }) : resolve('/projects/[project]/releases', { project })}>
			<span class="stat-label">Live release</span>
			{#if releases.isPending}
				<Skeleton class="h-8 w-24" />
			{:else if live}
				<span class="stat-value">{releaseName(live.generation)}</span>
				<ReleaseLabel release={live} testid="overview-label" />
			{:else}
				<span class="stat-value">None yet</span>
				<span class="text-xs text-muted-foreground">Nothing is published</span>
			{/if}
		</a>
		<a class="stat lift" style="--i: 1" href={resolve('/projects/[project]/changesets', { project })}>
			<span class="stat-label">Waiting for review</span>
			<span class="stat-value tabular" data-testid="awaiting-count">{(sets.data ?? []).filter((set) => set.status.state === 'awaiting_review').length}</span>
			<span class="text-xs text-muted-foreground">{count(waiting.length, PUBLISH_REQUEST, PUBLISH_REQUESTS)} in progress</span>
		</a>
		<div class="stat" style="--i: 2">
			<span class="stat-label">Entries</span>
			<span class="stat-value tabular">{all.length}</span>
			<span class="flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted-foreground">
				<span><span class="mr-1 inline-block size-1.5 rounded-full bg-live align-middle"></span>{tally.live} live</span>
				<span><span class="mr-1 inline-block size-1.5 rounded-full bg-pending align-middle"></span>{tally.changed} changed</span>
				<span><span class="mr-1 inline-block size-1.5 rounded-full bg-draft align-middle"></span>{tally.draft} draft</span>
			</span>
		</div>
		<div class="stat" style="--i: 3">
			<span class="stat-label">Last published</span>
			<span class="stat-value">{live ? ago(live.published_at) : '—'}</span>
			<span class="truncate text-xs text-muted-foreground">{live ? `by ${live.published_by_name ?? live.published_by}` : 'No releases yet'}</span>
		</div>
	</section>

	<div class="mt-10 grid gap-10 lg:grid-cols-[minmax(0,1fr)_20rem]">
		<section aria-labelledby="recent">
			<div class="mb-3 flex items-baseline justify-between">
				<h2 id="recent" class="text-base font-semibold">Recently edited</h2>
			</div>
			<div class="stagger overflow-hidden rounded-xl border border-border bg-card" class:hidden={recent.length === 0}>
				{#each recent as { type, entry }, index (entry.id)}
					<a
						class="rule-dotted flex items-center gap-3 px-4 py-3 transition-colors last:border-b-0 hover:bg-muted/50"
						style="--i: {index}"
						href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: type.slug, entry: entry.id })}
					>
						<FileTextIcon class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
						<span class="min-w-0 flex-1">
							<span class="block truncate text-sm font-medium">{entry.revision.title}</span>
							<span class="text-xs text-muted-foreground">{type.definition.name} · {ago(entry.updated_at)}</span>
						</span>
						<StatusBadge state={entryState(entry)} generation={entry.published?.generation ?? null} />
					</a>
				{/each}
			</div>
			{#if recent.length === 0}
				{#if !loaded}
					<p class="px-4 py-8 text-center text-sm text-muted-foreground">Loading…</p>
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
			</section>

			<section aria-labelledby="content-heading">
				<h2 id="content-heading" class="mb-3 text-base font-semibold">Content</h2>
				{#if types.isError}
					<Problem error={types.error} />
				{:else}
					<ul class="space-y-1" data-testid="content-types">
						{#each types.data ?? [] as type, index (type.slug)}
							<li>
								<a class="group flex items-center gap-3 rounded-lg px-2 py-2 hover:bg-muted" href={resolve('/projects/[project]/[collection]', { project, collection: type.slug })}>
									<span class="min-w-0 flex-1">
										<span class="block text-sm font-medium">{type.definition.name}</span>
										<span class="text-xs text-muted-foreground">{type.definition.fields.length} fields</span>
									</span>
									{#if lists[index]?.data}<Badge variant="secondary" class="tabular">{lists[index]?.data?.data.length}</Badge>{/if}
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
	.stat {
		display: flex;
		flex-direction: column;
		gap: 0.375rem;
		padding: 1rem 1.125rem;
		border: 1px solid var(--border);
		border-radius: var(--radius-xl);
		background: var(--card);
		box-shadow: 0 1px 2px 0 rgb(0 0 0 / 0.04);
		align-items: flex-start;
	}
	a.stat:hover {
		border-color: color-mix(in oklch, var(--foreground) 20%, transparent);
	}
	.stat-label {
		font-size: 0.8125rem;
		color: var(--muted-foreground);
	}
	.stat-value {
		font-size: 1.75rem;
		line-height: 2.25rem;
		font-weight: 600;
		letter-spacing: -0.02em;
		font-variant-numeric: tabular-nums;
	}
</style>
