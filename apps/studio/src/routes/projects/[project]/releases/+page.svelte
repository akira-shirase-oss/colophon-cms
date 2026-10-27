<!--
  A project's releases (D-045, D-046): each one numbered, what it published, who published it, its
  label, and how its announcement to each destination went. Destinations are set up in Settings.
  Above them, what is scheduled to publish and how past schedules went (D-060).
-->
<script lang="ts">
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import PackageCheckIcon from '@lucide/svelte/icons/package-check';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { Badge } from '@colophon/ui/ui/badge';
	import { deliveryUrl, type Job, listJobs, listReleases, listSchedules, type Release, type Schedule } from '#lib/api.ts';
	import { ago } from '#lib/queries.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import ReleaseLabel from '#lib/shell/ReleaseLabel.svelte';

	const project = $derived(page.params.project ?? '');
	// Schedules: upcoming first; look again while one is due within a minute, to show how it went.
	const schedules = createQuery(() => ({
		queryKey: ['schedules', project],
		queryFn: () => listSchedules(project),
		refetchInterval: (query) =>
			(query.state.data ?? []).some((item) => item.state === 'scheduled' && Date.parse(item.run_at) - Date.now() < 60_000) ? 2_000 : false
	}));
	const upcoming = $derived(
		(schedules.data ?? []).filter((item) => item.state === 'scheduled').sort((one, other) => Date.parse(one.run_at) - Date.parse(other.run_at))
	);
	const recent = $derived((schedules.data ?? []).filter((item) => item.state !== 'scheduled').slice(0, 5));
	const reads = (item: Schedule) =>
		new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short', timeZone: item.time_zone }).format(new Date(item.run_at));

	// A schedule that just published made a release this page hasn't read yet: read it now.
	const queryClient = useQueryClient();
	let newest: number | null = null;
	$effect(() => {
		const latest = Math.max(0, ...(schedules.data ?? []).map((item) => item.generation ?? 0));
		if (newest !== null && latest > newest) {
			void queryClient.invalidateQueries({ queryKey: ['releases', project] });
			void queryClient.invalidateQueries({ queryKey: ['jobs', project] });
		}
		newest = latest;
	});
	/** Whether a schedule publishes within the minute: its release is about to appear. */
	const dueSoon = () => (schedules.data ?? []).some((item) => item.state === 'scheduled' && Date.parse(item.run_at) - Date.now() < 60_000);
	// A CDN purge and a probe settle on their own: look again while one is pending.
	const releases = createQuery(() => ({
		queryKey: ['releases', project],
		queryFn: () => listReleases(project),
		refetchInterval: (query) =>
			query.state.data?.releases.some((release) => release.purge === 'pending' || release.label === 'pending') || dueSoon() ? 2_000 : false
	}));
	// Deliveries settle on their own; look again every few seconds while any is still going.
	const jobs = createQuery(() => ({
		queryKey: ['jobs', project],
		queryFn: () => listJobs(project),
		refetchInterval: (query) =>
			(query.state.data ?? []).some((job) => ['pending', 'running', 'retrying'].includes(job.status)) ? 2_000 : false
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
		succeeded: 'delivered',
		failed: 'failed',
		superseded: 'skipped: a newer release went instead'
	};
	const DOT: Record<Job['status'], string> = {
		pending: 'bg-pending',
		running: 'bg-pending',
		retrying: 'bg-pending',
		succeeded: 'bg-live',
		failed: 'bg-attention',
		superseded: 'bg-draft'
	};
	const PURGE: Record<Exclude<Release['purge'], 'none'>, string> = {
		pending: 'CDN purge pending: copies the CDN holds may still be served',
		accepted: 'CDN purge accepted',
		failed: 'CDN purge failed: copies the CDN holds may be served until they expire (up to 5 minutes)'
	};
</script>

<svelte:head><title>Releases · Colophon Studio</title></svelte:head>

<PageHeader title="Releases">
	{#snippet description()}
		Every publish is a numbered release. Its label says what is proven: in the CMS, on its way to the website, or verified where a probe looked.
	{/snippet}
</PageHeader>

{#if releases.isPending}
	<Skeleton class="h-48 w-full rounded-xl" />
{:else if releases.isError}
	<Problem error={releases.error} />
{:else}
	<p class="mb-6 flex flex-wrap items-center gap-x-2 gap-y-1 rounded-xl border border-border bg-card px-4 py-3 text-sm" data-testid="current-generation">
		{#if releases.data.generation === 0}
			Nothing is published yet.
		{:else}
			<span>Live: release {releases.data.generation}.</span>
			<span class="text-muted-foreground">Anyone can read it at</span>
			<a class="inline-flex items-center gap-1 font-mono text-xs underline underline-offset-2" href={deliveryUrl(project, 'articles')} data-testid="delivery-link" data-sveltekit-reload
				>{deliveryUrl(project, 'articles')}<ArrowUpRightIcon class="size-3" aria-hidden="true" /></a
			>
		{/if}
	</p>

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
							<Badge variant="pending">scheduled</Badge>
						{:else if item.state === 'published'}
							<Badge variant={item.delayed ? 'attention' : 'live'}>{item.delayed ? 'published late' : 'published'} · No. {item.generation}</Badge>
						{:else if item.state === 'blocked'}
							<Badge variant="attention" title={item.reason}>not published</Badge>
							<span class="basis-full text-xs text-muted-foreground">{item.reason}</span>
						{:else}
							<Badge variant="secondary">cancelled</Badge>
						{/if}
					</li>
				{/each}
			</ul>
		</section>
	{/if}

	{#if releases.data.releases.length === 0}
		<EmptyState icon={PackageCheckIcon} title="No releases yet">Publish an approved changeset to make release No. 1.</EmptyState>
	{:else}
		<ol class="stagger space-y-3" reversed data-testid="releases">
			{#each releases.data.releases as release, index (release.generation)}
				<li class="rounded-xl border border-border bg-card p-4" style="--i: {index}" data-generation={release.generation}>
					<div class="flex flex-wrap items-start gap-x-4 gap-y-2">
						<span class="text-2xl leading-none font-semibold tracking-tight tabular">
							<span aria-hidden="true" class="text-muted-foreground">No.&nbsp;</span><span class="sr-only">Release </span>{release.generation}
						</span>
						<div class="min-w-0 flex-1">
							<a class="font-medium hover:underline" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: release.changeset_id })}>{release.title}</a>
							<p class="mt-0.5 text-sm text-muted-foreground">
								{release.entries}
								{release.entries === 1 ? 'entry' : 'entries'} changed{#if release.unpublished > 0},
									<span data-testid="unpublished">{release.unpublished} unpublished</span>{/if} · by {release.published_by_name ?? release.published_by} ·
								<span title={new Date(release.published_at).toLocaleString()}>{ago(release.published_at)}</span>
							</p>
						</div>
						<ReleaseLabel {release} href={resolve('/projects/[project]/releases/[generation]', { project, generation: String(release.generation) })} />
					</div>
					{#if release.purge !== 'none' || (byRelease.get(release.generation) ?? []).length > 0}
						<ul class="mt-3 space-y-1 border-t border-dotted border-border pt-3 text-sm text-muted-foreground">
							{#if release.purge !== 'none'}
								<li class="flex items-center gap-2" data-testid="purge" data-state={release.purge}>
									<span class="size-1.5 rounded-full {release.purge === 'accepted' ? 'bg-live' : release.purge === 'failed' ? 'bg-attention' : 'bg-pending'}" aria-hidden="true"></span>
									{#if release.unpublished > 0 && release.purge === 'pending'}Removal pending at the CDN: copies it holds may still be served{:else}{PURGE[release.purge]}{/if}
								</li>
							{/if}
							{#each byRelease.get(release.generation) ?? [] as job (job.id)}
								<li class="flex items-center gap-2" data-testid="delivery" data-status={job.status} class:text-destructive={job.status === 'failed'}>
									<span class="size-1.5 rounded-full {DOT[job.status]}" class:breathe={['pending', 'running', 'retrying'].includes(job.status)} aria-hidden="true"></span>
									{job.destination_name}: {STATUS[job.status]}{job.attempts > 1 ? ` after ${job.attempts} attempts` : ''}{job.last_error && job.status !== 'succeeded'
										? ` (${job.last_error})`
										: ''}
								</li>
							{/each}
						</ul>
					{/if}
				</li>
			{/each}
		</ol>
	{/if}
{/if}
