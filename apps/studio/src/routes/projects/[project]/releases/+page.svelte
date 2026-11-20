<!--
  A project's releases (D-045, D-046), one readable row each: its number, its title (which opens
  its receipt), what it did for readers in words ("2 went live, 1 taken down"), who published it
  and when, its label, and how each website connection (destination) heard about it. The live
  release says so on its own row, once. Connections are set up in
  Settings; addresses for developers are in the Technical details. Above them, what is scheduled
  to publish and how past schedules went (D-060). The newest 100 first; older ones a page at a
  time, as the list pages (`?cursor=`).
-->
<script lang="ts">
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import PackageCheckIcon from '@lucide/svelte/icons/package-check';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createInfiniteQuery, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { deliveryUrl, type Job, listJobs, listReleases, listSchedules, type Release, type Schedule } from '#lib/api.ts';
	import { ago, releasesQuery, typesQuery } from '#lib/queries.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import { failureText } from '#lib/receipts.ts';
	import ReleaseLabel from '#lib/shell/ReleaseLabel.svelte';
	import StatusBadge from '#lib/shell/StatusBadge.svelte';
	import { releaseName, STATUS_WORDS } from '#lib/words.ts';
	import { originText, releaseOrigin } from '#lib/releaseOrigin.ts';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';

	const project = $derived(page.params.project ?? '');
	// Schedules: upcoming first. A schedule running commits a release, and the live stream's
	// `schedules` and `releases` hints land here (D-108) — nothing to poll for.
	const schedules = createQuery(() => ({
		queryKey: ['schedules', project],
		queryFn: () => listSchedules(project)
	}));
	const upcoming = $derived(
		(schedules.data ?? []).filter((item) => item.state === 'scheduled').sort((one, other) => Date.parse(one.run_at) - Date.parse(other.run_at))
	);
	const recent = $derived((schedules.data ?? []).filter((item) => item.state !== 'scheduled').slice(0, 5));
	const reads = (item: Schedule) =>
		new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short', timeZone: item.time_zone }).format(new Date(item.run_at));

	const types = createQuery(() => typesQuery(project));
	const firstType = $derived(types.data?.find((type) => type.slug !== 'articles') ?? types.data?.[0]);
	// A CDN purge and a probe settle on their own: the worker's commits arrive as `releases`
	// hints (D-108), which cover this and the jobs below — nothing to poll for. Pages, newest
	// first: a hint reads every page shown again from the first, each from the fresh page before
	// it, so a release published meanwhile never leaves a gap between them.
	type ReleasePage = Awaited<ReturnType<typeof listReleases>>;
	const queryClient = useQueryClient();
	const releases = createInfiniteQuery(() => ({
		queryKey: ['releases', project, 'pages'],
		// The first page is the sidebar's release read (`releasesQuery`): one request serves both (P3-12).
		queryFn: ({ pageParam }: { pageParam: string | null }) =>
			pageParam === null ? queryClient.fetchQuery(releasesQuery(project)) : listReleases(project, pageParam),
		initialPageParam: null as string | null,
		getNextPageParam: (last: ReleasePage) => last.nextCursor
	}));
	const generation = $derived(releases.data?.pages[0]?.generation ?? 0);
	const shown = $derived((releases.data?.pages ?? []).flatMap((one) => one.releases));
	// Deliveries settle on their own; the `releases` kind carries job commits too.
	const jobs = createQuery(() => ({
		queryKey: ['jobs', project],
		queryFn: () => listJobs(project)
	}));

	const byRelease = $derived(
		(jobs.data ?? []).reduce<Map<number, Job[]>>((grouped, job) => {
			grouped.set(job.generation, [...(grouped.get(job.generation) ?? []), job]);
			return grouped;
		}, new Map())
	);

	const STATUS: Record<Job['status'], string> = {
		pending: 'waiting',
		running: 'sending',
		retrying: 'will retry',
		held: 'held: waiting on the operator after a restore',
		succeeded: 'delivered',
		failed: 'failed',
		superseded: 'skipped: a newer release went instead'
	};
	const DOT: Record<Job['status'], string> = {
		pending: 'bg-pending',
		running: 'bg-pending',
		retrying: 'bg-pending',
		held: 'bg-attention',
		succeeded: 'bg-live',
		failed: 'bg-attention',
		superseded: 'bg-draft'
	};
	/** What a release did for readers, in words: "2 went live, 1 taken down". */
	function wentLive(release: Release): string {
		const parts = [
			...(release.entries > 0 ? [`${release.entries.toLocaleString()} went live`] : []),
			...(release.unpublished > 0 ? [`${release.unpublished.toLocaleString()} taken down`] : [])
		];
		return parts.length === 0 ? 'Nothing changed for readers' : parts.join(', ');
	}
	const PURGE: Record<Exclude<Release['purge'], 'none'>, string> = {
		pending: 'CDN purge pending: copies the CDN holds may still be served',
		accepted: 'CDN purge accepted',
		failed: 'CDN purge failed: copies the CDN holds may be served until they expire (up to 5 minutes)'
	};
</script>

<svelte:head><title>Releases · Colophon Studio</title></svelte:head>

<PageHeader title="Releases">
	{#snippet description()}
		Every publish becomes a numbered release, with a receipt of what reached your website.
	{/snippet}
</PageHeader>

{#if releases.isPending}
	<Skeleton class="h-48 w-full rounded-xl" />
{:else if releases.isError && releases.data === undefined}
	<Problem error={releases.error} />
{:else}
	{#if schedules.isError}
		<div class="mb-6"><Problem error={schedules.error} what="what is scheduled" retry={() => schedules.refetch()} /></div>
	{/if}
	<!-- A failed refresh, or a failed older page, keeps the releases already shown: the first says so
	     here, the second beside its button. -->
	{#if releases.isRefetchError}
		<div class="mb-6"><Problem error={releases.error} what="the latest releases" retry={() => releases.refetch()} /></div>
	{/if}
	{#if jobs.isError}
		<div class="mb-6"><Problem error={jobs.error} what="how each release reached your website" retry={() => jobs.refetch()} /></div>
	{/if}

	{#if upcoming.length > 0 || recent.length > 0}
		<section class="mb-8" aria-labelledby="scheduled-heading" data-testid="schedules">
			<h2 id="scheduled-heading" class="mb-3 text-base font-semibold">Scheduled</h2>
			<ul class="overflow-hidden rounded-xl border border-border bg-card">
				{#each [...upcoming, ...recent] as item (item.id)}
					<li class="rule-dotted flex flex-wrap items-center gap-x-4 gap-y-1 px-4 py-3 text-sm last:border-b-0" data-state={item.state}>
						<a class="min-w-0 flex-1 truncate font-medium hover:underline" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: item.changeset_id })}
							>{item.title}</a
						>
						<span class="text-muted-foreground">{reads(item)} <span class="text-xs">({item.time_zone})</span></span>
						{#if item.state === 'scheduled'}
							<StatusBadge state="scheduled" />
						{:else if item.state === 'published'}
							<StatusBadge
								state={item.delayed ? 'published_late' : 'published'}
								text="{item.delayed ? STATUS_WORDS.published_late : STATUS_WORDS.published} · {releaseName(item.generation ?? 0)}"
								class="tabular"
							/>
						{:else if item.state === 'blocked'}
							<StatusBadge state="not_published" title={item.reason} />
							<span class="basis-full text-xs text-muted-foreground">{item.reason}</span>
						{:else}
							<StatusBadge state="cancelled" />
						{/if}
					</li>
				{/each}
			</ul>
		</section>
	{/if}

	{#if shown.length === 0}
		<EmptyState icon={PackageCheckIcon} title="Nothing is live yet" testid="releases-empty">
			Each time something is published it becomes a numbered release, with a receipt of what reached your website. Publish an entry to make release 1.
			{#snippet action()}
				{#if firstType}<Button href={resolve('/projects/[project]/[collection]', { project, collection: firstType.slug })}>Go to {firstType.definition.name}</Button>{/if}
			{/snippet}
		</EmptyState>
	{:else}
		<ol class="stagger overflow-hidden rounded-xl border border-border bg-card" reversed data-testid="releases">
			{#each shown as release, index (release.generation)}
				{@const receiptHref = resolve('/projects/[project]/releases/[generation]', { project, generation: String(release.generation) })}
				{@const jobs = byRelease.get(release.generation) ?? []}
				<li class="rule-dotted relative grid gap-x-4 gap-y-1 px-4 py-3.5 last:border-b-0 hover:bg-muted/40 sm:grid-cols-[6rem_minmax(0,1fr)_auto] sm:items-start" style="--i: {index}" data-generation={release.generation}>
					<span class="text-sm text-muted-foreground tabular sm:pt-0.5">{releaseName(release.generation)}</span>
					<div class="min-w-0">
						<a class="block truncate font-medium after:absolute after:inset-0 hover:underline" href={receiptHref} title={release.title} data-user-content>{release.title}</a>
						<p class="mt-0.5 text-sm text-muted-foreground">
							{#if originText(releaseOrigin(release))}<span data-testid="release-origin">{originText(releaseOrigin(release))}</span> · {/if}{wentLive(release)} · by {release.published_by_name ?? release.published_by} ·
							<span title={new Date(release.published_at).toLocaleString()}>{ago(release.published_at)}</span>
						</p>
						{#if release.purge !== 'none' || jobs.length > 0}
							<ul class="mt-2 space-y-1 text-sm text-muted-foreground">
								{#if release.purge !== 'none'}
									<li class="flex items-center gap-2" data-testid="purge" data-state={release.purge}>
										<span class="size-1.5 rounded-full {release.purge === 'accepted' ? 'bg-live' : release.purge === 'failed' ? 'bg-attention' : 'bg-pending'}" aria-hidden="true"></span>
										{#if release.unpublished > 0 && release.purge === 'pending'}Removal pending at the CDN: copies it holds may still be served{:else}{PURGE[release.purge]}{/if}
									</li>
								{/if}
								{#each jobs as job (job.id)}
									<li class="flex items-center gap-2" data-testid="delivery" data-status={job.status} class:text-destructive={job.status === 'failed'}>
										<span class="size-1.5 rounded-full {DOT[job.status]}" aria-hidden="true"></span>
										{job.destination_name}: {STATUS[job.status]}{job.attempts > 1 ? ` after ${job.attempts} attempts` : ''}{job.last_error && job.status !== 'succeeded'
											? ` (${failureText(job.last_error)})`
											: ''}
									</li>
								{/each}
							</ul>
						{/if}
					</div>
					<span class="flex flex-wrap items-center gap-2 sm:justify-end">
						{#if release.generation === generation}<StatusBadge state="live" text="Live now" title="Readers get this release" testid="live-now" />{/if}
						<ReleaseLabel {release} />
					</span>
				</li>
			{/each}
		</ol>
		{#if releases.hasNextPage || releases.isFetchingNextPage || releases.isFetchNextPageError}
			<div class="mt-4 flex flex-wrap items-center gap-3">
				<Button variant="outline" size="sm" onclick={() => releases.fetchNextPage()} disabled={releases.isFetchingNextPage} data-testid="releases-older">
					{releases.isFetchingNextPage ? 'Loading older releases…' : 'Show older releases'}
				</Button>
				{#if releases.isFetchNextPageError}
					<p role="alert" class="text-sm text-destructive" data-testid="releases-older-failed">Couldn't load older releases. Try again.</p>
				{/if}
			</div>
		{:else if shown.length > 100}
			<p class="mt-4 text-sm text-muted-foreground" data-testid="releases-all">That's every release, back to release {shown.at(-1)?.generation}.</p>
		{/if}
	{/if}
	<TechnicalDetails class="mt-8">
		<p>Releases are generations: each publish takes the next one, and a reader's request can pin one with <span class="font-mono">?generation=n</span>.</p>
		{#each types.data ?? [] as type (type.slug)}
			<p>
				<a class="inline-flex items-center gap-1 font-mono underline underline-offset-2" href={deliveryUrl(project, type.slug)} data-testid="delivery-link" data-sveltekit-reload
					>{deliveryUrl(project, type.slug)}<ArrowUpRightIcon class="size-3" aria-hidden="true" /></a
				>
			</p>
		{/each}
	</TechnicalDetails>
{/if}
