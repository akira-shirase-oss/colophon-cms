<!--
  A project at a glance: what readers have now, what's waiting for a reviewer, where the content
  stands, and what changed last. Every number links to the page that explains it.
-->
<script lang="ts">
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import CheckIcon from '@lucide/svelte/icons/check';
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQueries, createQuery } from '@tanstack/svelte-query';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { entryState, type Summary } from '#lib/api.ts';
	import { ago, changesetsQuery, entriesQuery, projectQuery, releasesQuery, typesQuery } from '#lib/queries.ts';
	import ChangesetBadge from '#lib/shell/ChangesetBadge.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import ReleaseLabel from '#lib/shell/ReleaseLabel.svelte';
	import StatusBadge from '#lib/shell/StatusBadge.svelte';

	const project = $derived(page.params.project ?? '');
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
	const recent = $derived([...all].sort((a, b) => b.entry.updated_at.localeCompare(a.entry.updated_at)).slice(0, 8));
	// Getting started: the four first things a new project needs, in order; each links to its
	// page and the card retires itself once all are done.
	const firstCollection = $derived(
		resolve('/projects/[project]/[collection]', {
			project,
			collection: (types.data ?? [])[0]?.slug ?? 'articles'
		})
	);
	const steps = $derived([
		{
			label: 'Create a collection',
			done: (types.data ?? []).length > 1,
			href: resolve('/projects/[project]/types/new', { project })
		},
		{ label: 'Write an entry', done: all.length > 0, href: firstCollection },
		{
			label: 'Ask for a review',
			done: (sets.data ?? []).length > 0,
			href: resolve('/projects/[project]/changesets', { project })
		},
		{
			label: 'Publish a release',
			done: live !== undefined,
			href: resolve('/projects/[project]/changesets', { project })
		}
	]);
	const loaded = $derived(
		!types.isPending && !sets.isPending && !releases.isPending && !lists.some((list) => list.isPending)
	);
	const ROLE_WORD = { owner: 'Owner', developer: 'Developer', editor: 'Editor', publisher: 'Publisher' } as const;
</script>

<svelte:head><title>{mine.data?.name ?? 'Overview'} · Colophon Studio</title></svelte:head>

<PageHeader title={mine.data?.name ?? 'Overview'}>
	{#snippet meta()}
		{#if mine.data}
			<span>{mine.data.slug}</span>
			<span aria-hidden="true">·</span>
			<span>{ROLE_WORD[mine.data.role]}</span>
		{/if}
	{/snippet}
</PageHeader>

{#if mine.isError}
	<Problem error={mine.error} />
{:else}
	<section class="stagger grid gap-3 sm:grid-cols-2 xl:grid-cols-4" aria-label="At a glance">
		<a class="stat lift" style="--i: 0" href={live ? resolve('/projects/[project]/releases/[generation]', { project, generation: String(live.generation) }) : resolve('/projects/[project]/releases', { project })}>
			<span class="stat-label">Live release</span>
			{#if releases.isPending}
				<Skeleton class="h-8 w-24" />
			{:else if live}
				<span class="stat-value">No. {live.generation}</span>
				<ReleaseLabel release={live} testid="overview-label" />
			{:else}
				<span class="stat-value">None yet</span>
				<span class="text-sm text-muted-foreground">Nothing is published</span>
			{/if}
		</a>
		<a class="stat lift" style="--i: 1" href={resolve('/projects/[project]/changesets', { project })}>
			<span class="stat-label">Waiting for review</span>
			<span class="stat-value tabular" data-testid="awaiting-count">{(sets.data ?? []).filter((set) => set.status.state === 'awaiting_review').length}</span>
			<span class="text-sm text-muted-foreground">{waiting.length} changeset{waiting.length === 1 ? '' : 's'} in progress</span>
		</a>
		<div class="stat" style="--i: 2">
			<span class="stat-label">Entries</span>
			<span class="stat-value tabular">{all.length}</span>
			<span class="flex flex-wrap gap-x-3 gap-y-1 text-sm text-muted-foreground">
				<span><span class="mr-1 inline-block size-1.5 rounded-full bg-live align-middle"></span>{tally.live} live</span>
				<span><span class="mr-1 inline-block size-1.5 rounded-full bg-pending align-middle"></span>{tally.changed} changed</span>
				<span><span class="mr-1 inline-block size-1.5 rounded-full bg-draft align-middle"></span>{tally.draft} draft</span>
			</span>
		</div>
		<div class="stat" style="--i: 3">
			<span class="stat-label">Last published</span>
			<span class="stat-value">{live ? ago(live.published_at) : '—'}</span>
			<span class="truncate text-sm text-muted-foreground">{live ? `by ${live.published_by_name ?? live.published_by}` : 'No releases yet'}</span>
		</div>
	</section>

	{#if loaded && steps.some((step) => !step.done)}
		<section class="mt-6 overflow-hidden rounded-xl border border-border bg-card" aria-labelledby="get-started" data-testid="get-started">
			<div class="border-b border-border px-4 py-3">
				<h2 id="get-started" class="text-base font-semibold">Getting started</h2>
				<p class="mt-0.5 text-sm text-muted-foreground">Four steps to your first published release.</p>
			</div>
			<ol>
				{#each steps as step, index (step.label)}
					<li class="flex items-center gap-3 border-b border-border px-4 py-2.5 last:border-b-0">
						{#if step.done}
							<span class="flex size-5 shrink-0 items-center justify-center rounded-full bg-live text-white" aria-hidden="true">
								<CheckIcon class="size-3.5" />
							</span>
							<span class="text-sm text-muted-foreground line-through">{step.label}</span>
						{:else}
							<span class="flex size-5 shrink-0 items-center justify-center rounded-full border border-border text-sm text-muted-foreground tabular" aria-hidden="true">{index + 1}</span>
							<a class="group flex flex-1 items-center gap-1 text-sm font-medium hover:underline" href={step.href}>
								{step.label}
								<ArrowRightIcon class="size-3.5 text-muted-foreground transition-transform group-hover:translate-x-0.5" aria-hidden="true" />
							</a>
						{/if}
					</li>
				{/each}
			</ol>
		</section>
	{/if}

	<div class="mt-10 grid gap-10 lg:grid-cols-[minmax(0,1fr)_20rem]">
		<section aria-labelledby="recent">
			<div class="mb-3 flex items-baseline justify-between">
				<h2 id="recent" class="text-base font-semibold">Recently edited</h2>
			</div>
			<div class="stagger overflow-hidden rounded-xl border border-border bg-card">
				{#each recent as { type, entry }, index (entry.id)}
					<a
						class="rule-dotted flex items-center gap-3 px-4 py-3 transition-colors last:border-b-0 hover:bg-muted/50"
						style="--i: {index}"
						href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: type.slug, entry: entry.id })}
					>
						<FileTextIcon class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
						<span class="min-w-0 flex-1">
							<span class="block truncate text-sm font-medium">{entry.revision.title}</span>
							<span class="text-sm text-muted-foreground">{type.definition.name} · {ago(entry.updated_at)}</span>
						</span>
						<StatusBadge state={entryState(entry)} generation={entry.published?.generation ?? null} />
					</a>
				{:else}
					<p class="px-4 py-8 text-center text-sm text-muted-foreground">{lists.some((list) => list.isPending) ? 'Loading…' : 'Nothing written yet.'}</p>
				{/each}
			</div>
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
									<span class="text-sm text-muted-foreground">{set.items} {set.items === 1 ? 'entry' : 'entries'}</span>
								</span>
							</a>
						</li>
					{:else}
						<li class="text-sm text-muted-foreground">Nothing is waiting.</li>
					{/each}
				</ul>
			</section>

			<section aria-labelledby="content-heading">
				<div class="mb-3 flex items-baseline justify-between">
					<h2 id="content-heading" class="text-base font-semibold">Content</h2>
					<span class="text-sm text-muted-foreground">Collections hold your entries</span>
				</div>
				{#if types.isError}
					<Problem error={types.error} />
				{:else}
					<ul class="space-y-1" data-testid="content-types">
						{#each types.data ?? [] as type, index (type.slug)}
							{@const entries = lists[index]?.data?.data ?? []}
							{@const tally = entries.reduce((c, e) => { c[entryState(e)] += 1; return c; }, { draft: 0, live: 0, changed: 0 })}
							<li>
								<a class="group flex items-center gap-3 rounded-lg px-2 py-2 hover:bg-muted" href={resolve('/projects/[project]/[collection]', { project, collection: type.slug })}>
									<span class="min-w-0 flex-1">
										<span class="block text-sm font-medium">{type.definition.name}</span>
										<span class="text-sm text-muted-foreground">
											{entries.length} {entries.length === 1 ? 'entry' : 'entries'} ·
											{#if tally.live > 0}<span class="text-live-ink">{tally.live} live</span>{:else}0 live{/if}{#if tally.changed > 0} · <span class="text-pending-ink">{tally.changed} changed</span>{/if}{#if tally.draft > 0} · <span class="text-draft-ink">{tally.draft} draft</span>{/if}
										</span>
									</span>
									<ArrowRightIcon class="size-4 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" aria-hidden="true" />
								</a>
							</li>
						{:else}
							<li class="rounded-lg border border-dashed border-border px-3 py-4 text-sm text-muted-foreground">
								No collections yet — a collection is a kind of entry (articles, authors, products).
							</li>
						{/each}
						{#if mine.data?.role === 'owner' || mine.data?.role === 'developer'}
							<li>
								<a class="group flex items-center gap-3 rounded-lg border border-dashed border-border px-2 py-2 text-sm text-muted-foreground hover:border-foreground/30 hover:text-foreground" href={resolve('/projects/[project]/types/new', { project })} data-testid="new-collection-link">
									+ New collection
								</a>
							</li>
						{/if}
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
		font-size: 0.875rem;
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
