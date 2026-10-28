<!--
  Someone's work in one project, for the home page: entries they started that aren't live,
  publish requests waiting for review and their own still in progress, what is scheduled (and any
  schedule that was blocked, with why), and the last three releases with their labels. Every row
  links to where the work happens; every list says something when it is empty or can't load.
-->
<script lang="ts">
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import CalendarClockIcon from '@lucide/svelte/icons/calendar-clock';
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import { resolve } from '$app/paths';
	import { createQueries, createQuery } from '@tanstack/svelte-query';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { entryState, listSchedules, may, mayAny, searchEntries, type Summary } from './api.ts';
	import { mainCollection, oneOf } from './firstRun.ts';
	import { ago, changesetsQuery, projectQuery, releasesQuery, typesQuery } from './queries.ts';
	import ChangesetBadge from './shell/ChangesetBadge.svelte';
	import Problem from './shell/Problem.svelte';
	import ReleaseLabel from './shell/ReleaseLabel.svelte';
	import StatusBadge from './shell/StatusBadge.svelte';
	import { PUBLISH_REQUESTS, releaseName, shownTitle } from './words.ts';

	let { project, identity }: { project: string; identity: string } = $props();

	const mine = createQuery(() => projectQuery(project));
	const types = createQuery(() => typesQuery(project));
	const sets = createQuery(() => changesetsQuery(project));
	const releases = createQuery(() => releasesQuery(project));
	const schedules = createQuery(() => ({ queryKey: ['schedules', project], queryFn: () => listSchedules(project) }));
	const readable = $derived(mine.data === undefined ? [] : (types.data ?? []).filter((type) => may(mine.data, 'content.read', type.slug)));
	// Each collection's entries this person made, newest first, however many others there are (the
	// list's `created_by`). Keyed under the collection's entries, so saving and publishing refresh it.
	const lists = createQueries(() => ({
		queries: readable.map((type) => ({
			queryKey: ['entries', project, type.slug, 'by', identity],
			queryFn: () => searchEntries(project, type.slug, '', null, identity)
		}))
	}));

	// Read every query's data and error before testing any (svelte-query updates only what was read).
	const listed = $derived(lists.map((list) => ({ data: list.data, error: list.isError })));
	const listsSettled = $derived(mine.data !== undefined && types.data !== undefined && listed.every((list) => list.data !== undefined || list.error));
	const listsFailed = $derived(listed.some((list) => list.error));
	/** Made by this person and not live as it stands: the drafts they are in the middle of. */
	const drafts = $derived(
		readable
			.flatMap((type, index) => (listed[index]?.data?.data ?? []).map((entry: Summary) => ({ type, entry })))
			.filter(({ entry }) => entryState(entry) !== 'live')
			.sort((one, other) => other.entry.updated_at.localeCompare(one.entry.updated_at))
			.slice(0, 6)
	);
	const reviews = $derived(mayAny(mine.data, ['content.review']));
	const waiting = $derived((sets.data ?? []).filter((set) => set.status.state === 'awaiting_review').slice(0, 6));
	const yours = $derived(
		(sets.data ?? [])
			.filter((set) => (set.created_by === identity || set.submitted_by === identity) && !['awaiting_review', 'published'].includes(set.status.state))
			.slice(0, 6)
	);
	const upcoming = $derived(
		(schedules.data ?? [])
			.filter((item) => item.state === 'scheduled')
			.sort((one, other) => Date.parse(one.run_at) - Date.parse(other.run_at))
	);
	const blocked = $derived((schedules.data ?? []).filter((item) => item.state === 'blocked').slice(0, 3));
	const latest = $derived((releases.data?.releases ?? []).slice(0, 3));
	const main = $derived(mainCollection(readable));
	const writes = $derived(main !== undefined && may(mine.data, 'content.create', main.slug));
	const when = (at: string) => new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(at));
</script>

<div class="grid gap-6 lg:grid-cols-2" data-testid="my-work">
	<section class="card" aria-labelledby="my-drafts">
		<h2 id="my-drafts" class="card-title">Your drafts</h2>
		<p class="card-hint">What you started that readers don't have yet.</p>
		{#if !listsSettled}
			<Skeleton class="h-24 w-full rounded-lg" />
		{:else}
			{#if listsFailed}
				<div class="mb-3"><Problem error={lists.find((list) => list.isError)?.error ?? new Error('')} what="your drafts" retry={() => lists.forEach((list) => list.isError && list.refetch())} /></div>
			{/if}
			<ul class="rows" data-testid="my-drafts">
				{#each drafts as { type, entry } (entry.id)}
					<li>
						<a class="row" href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: type.slug, entry: entry.id })}>
							<FileTextIcon class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
							<span class="min-w-0 flex-1">
								<span class="block truncate text-sm font-medium" data-user-content>{shownTitle(entry.revision.title)}</span>
								<span class="text-xs text-muted-foreground">{type.definition.name} · {ago(entry.updated_at)}</span>
							</span>
							<StatusBadge state={entryState(entry)} generation={entry.published?.generation ?? null} />
						</a>
					</li>
				{:else}
					<li class="empty">
						Nothing half-finished.
						{#if writes && main}<Button class="ml-1" size="sm" variant="outline" href={resolve('/projects/[project]/[collection]', { project, collection: main.slug }) + '?new=1'}>Write a new {oneOf(main.definition.name)}</Button>{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</section>

	<section class="card" aria-labelledby="my-requests">
		<h2 id="my-requests" class="card-title">Publish requests</h2>
		{#if sets.isPending}
			<Skeleton class="h-24 w-full rounded-lg" />
		{:else if sets.isError}
			<Problem error={sets.error} what={PUBLISH_REQUESTS} retry={() => sets.refetch()} />
		{:else}
			<h3 class="card-hint" id="waiting-heading">{reviews ? 'Waiting for your review' : 'Waiting for review: a publisher approves these'}</h3>
			<ul class="rows" aria-labelledby="waiting-heading" data-testid="waiting-review">
				{#each waiting as set (set.id)}
					<li>
						<a class="row" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: set.id })}>
							<span class="min-w-0 flex-1 truncate text-sm font-medium" data-user-content>{set.title}</span>
							<span class="text-xs text-muted-foreground">{set.submitted_at ? ago(set.submitted_at) : ''}</span>
						</a>
					</li>
				{:else}
					<li class="empty">Nothing is waiting for review.</li>
				{/each}
			</ul>
			{#if yours.length > 0}
				<h3 class="card-hint mt-4" id="yours-heading">Yours, still in progress</h3>
				<ul class="rows" aria-labelledby="yours-heading" data-testid="my-requests-list">
					{#each yours as set (set.id)}
						<li>
							<a class="row" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: set.id })}>
								<span class="min-w-0 flex-1 truncate text-sm font-medium" data-user-content>{set.title}</span>
								<ChangesetBadge state={set.status.state} />
							</a>
						</li>
					{/each}
				</ul>
			{/if}
		{/if}
	</section>

	<section class="card" aria-labelledby="my-schedules">
		<h2 id="my-schedules" class="card-title">Scheduled</h2>
		{#if schedules.isPending}
			<Skeleton class="h-16 w-full rounded-lg" />
		{:else if schedules.isError}
			<Problem error={schedules.error} what="what is scheduled" retry={() => schedules.refetch()} />
		{:else}
			<ul class="rows" data-testid="my-schedules-list">
				{#each upcoming.slice(0, 5) as item (item.id)}
					<li>
						<a class="row" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: item.changeset_id })}>
							<CalendarClockIcon class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
							<span class="min-w-0 flex-1">
								<span class="block truncate text-sm font-medium" data-user-content>{item.title}</span>
								<span class="text-xs text-muted-foreground">Publishes {when(item.run_at)}</span>
							</span>
							<Badge variant="pending">scheduled</Badge>
						</a>
					</li>
				{/each}
				{#each blocked as item (item.id)}
					<li>
						<a class="row" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: item.changeset_id })}>
							<CalendarClockIcon class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
							<span class="min-w-0 flex-1">
								<span class="block truncate text-sm font-medium" data-user-content>{item.title}</span>
								<span class="block text-xs text-attention-ink">Not published: {item.reason ?? 'it was refused'}</span>
							</span>
							<Badge variant="attention">not published</Badge>
						</a>
					</li>
				{/each}
				{#if upcoming.length === 0 && blocked.length === 0}
					<li class="empty">Nothing is scheduled. An approved publish request can be set to publish at a time you choose.</li>
				{/if}
			</ul>
		{/if}
	</section>

	<section class="card" aria-labelledby="my-releases">
		<h2 id="my-releases" class="card-title">Recent releases</h2>
		{#if releases.isPending}
			<Skeleton class="h-16 w-full rounded-lg" />
		{:else if releases.isError}
			<Problem error={releases.error} what="releases" retry={() => releases.refetch()} />
		{:else}
			<ul class="rows" data-testid="my-releases-list">
				{#each latest as release (release.generation)}
					<li>
						<a class="row" href={resolve('/projects/[project]/releases/[generation]', { project, generation: String(release.generation) })}>
							<span class="min-w-0 flex-1">
								<span class="block truncate text-sm font-medium">{releaseName(release.generation)}<span class="font-normal text-muted-foreground" data-user-content>: {release.title}</span></span>
								<span class="text-xs text-muted-foreground">{ago(release.published_at)}</span>
							</span>
							<ReleaseLabel {release} />
						</a>
					</li>
				{:else}
					<li class="empty">Nothing is live yet. Each publish becomes a numbered release, with a receipt.</li>
				{/each}
			</ul>
		{/if}
	</section>
</div>
<p class="mt-4">
	<Button variant="outline" href={resolve('/projects/[project]', { project })}>Open the project<ArrowRightIcon data-icon="inline-end" /></Button>
</p>

<style>
	.card {
		min-width: 0;
		padding: 1rem 1.125rem;
		border: 1px solid var(--border);
		border-radius: var(--radius-xl);
		background: var(--card);
	}
	.card-title {
		font-size: 1rem;
		font-weight: 600;
	}
	.card-hint {
		margin-block: 0.125rem 0.75rem;
		font-size: 0.875rem;
		color: var(--muted-foreground);
	}
	.rows {
		display: grid;
		gap: 0.25rem;
	}
	.row {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		min-width: 0;
		padding: 0.5rem 0.5rem;
		border-radius: var(--radius-lg);
	}
	.row:hover {
		background: var(--muted);
	}
	.row:focus-visible {
		outline: none;
		box-shadow: 0 0 0 3px color-mix(in oklch, var(--ring) 40%, transparent);
	}
	.empty {
		padding: 0.5rem;
		font-size: 0.875rem;
		color: var(--muted-foreground);
	}
</style>
