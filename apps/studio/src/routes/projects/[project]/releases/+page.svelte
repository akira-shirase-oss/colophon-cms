<!--
  A project's releases (D-045, D-046): each one numbered, what it published, who published it,
  and how its announcement to each destination went; where anyone can read the result; and, for
  owners and developers, the destinations releases are announced to.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Button, TextField } from '@colophon/ui';
	import {
		ApiError,
		createDestination,
		deliveryUrl,
		getProject,
		type Job,
		listDestinations,
		listJobs,
		listReleases,
		setDestinationEnabled
	} from '#lib/api.ts';

	const project = $derived(page.params.project ?? '');
	const queryClient = useQueryClient();
	// The role comes from the project itself: me().projects lists at most 1,000 projects.
	const mine = createQuery(() => ({ queryKey: ['project', project], queryFn: () => getProject(project) }));
	const role = $derived(mine.data?.role);
	const manages = $derived(role === 'owner' || role === 'developer');
	const releases = createQuery(() => ({ queryKey: ['releases', project], queryFn: () => listReleases(project) }));
	// Deliveries settle on their own; look again every few seconds while any is still going.
	const jobs = createQuery(() => ({
		queryKey: ['jobs', project],
		queryFn: () => listJobs(project),
		refetchInterval: (query) =>
			(query.state.data ?? []).some((job) => ['pending', 'running', 'retrying'].includes(job.status)) ? 2_000 : false
	}));
	const destinations = createQuery(() => ({
		queryKey: ['destinations', project],
		queryFn: () => listDestinations(project),
		enabled: manages
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

	let name = $state('');
	let url = $state('');
	let error = $state('');
	let busy = $state(false);

	async function add(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		try {
			await createDestination(project, name, url);
			name = '';
			url = '';
			await queryClient.invalidateQueries({ queryKey: ['destinations', project] });
		} catch (failure) {
			error = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			busy = false;
		}
	}

	async function toggle(id: string, enabled: boolean) {
		await setDestinationEnabled(project, id, enabled);
		await queryClient.invalidateQueries({ queryKey: ['destinations', project] });
	}
</script>

<svelte:head><title>Releases · Colophon Studio</title></svelte:head>

<p><a href={resolve('/projects/[project]', { project })}>← Content</a></p>
<h1>Releases</h1>

{#if releases.isPending}
	<p class="muted">Loading…</p>
{:else if releases.isError}
	<p role="alert">{releases.error.message}</p>
{:else}
	<p data-testid="current-generation">
		{#if releases.data.generation === 0}
			Nothing is published yet.
		{:else}
			Live: release {releases.data.generation}. Anyone can read it at
			<a href={deliveryUrl(project, 'articles')} data-testid="delivery-link" data-sveltekit-reload>{deliveryUrl(project, 'articles')}</a>.
		{/if}
	</p>
	<ol class="list" reversed data-testid="releases">
		{#each releases.data.releases as release (release.generation)}
			<li data-generation={release.generation}>
				<strong>Release {release.generation}</strong>:
				<a href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: release.changeset_id })}>{release.title}</a>
				<small class="muted">
					{release.entries}
					{release.entries === 1 ? 'entry' : 'entries'} changed, by {release.published_by_name ?? release.published_by},
					{new Date(release.published_at).toLocaleString()}
				</small>
				{#each byRelease.get(release.generation) ?? [] as job (job.id)}
					<div class="delivery" data-testid="delivery" data-status={job.status}>
						{job.destination_name}: {STATUS[job.status]}{job.attempts > 1 ? ` after ${job.attempts} attempts` : ''}{job.last_error &&
						job.status !== 'succeeded'
							? ` (${job.last_error})`
							: ''}
					</div>
				{/each}
			</li>
		{/each}
	</ol>
{/if}

{#if manages}
	<section class="destinations" aria-label="Destinations">
		<h2>Destinations</h2>
		<p class="muted">Every release is announced to each enabled destination, once, with a key it can use to drop repeats.</p>
		{#if destinations.data}
			<ul class="list" data-testid="destinations">
				{#each destinations.data as destination (destination.id)}
					<li>
						<strong>{destination.name}</strong> <code>{destination.url}</code>
						<label class="check">
							<input type="checkbox" checked={destination.enabled} onchange={(event) => toggle(destination.id, event.currentTarget.checked)} />
							Announce releases
						</label>
					</li>
				{:else}
					<li class="muted">None yet.</li>
				{/each}
			</ul>
		{/if}
		<form class="new" onsubmit={add} aria-label="New destination">
			<TextField label="Destination name" bind:value={name} required maxlength={100} />
			<TextField label="Webhook URL (https)" bind:value={url} required type="url" />
			{#if error}<p role="alert" class="error">{error}</p>{/if}
			<div><Button type="submit" disabled={busy}>Add destination</Button></div>
		</form>
	</section>
{/if}

<style>
	.list {
		display: grid;
		gap: 0.75rem;
		padding-left: 1.25rem;
	}
	.delivery {
		font: var(--md-sys-typescale-body-small);
		color: var(--md-sys-color-on-surface-variant);
	}
	.delivery[data-status='failed'] {
		color: var(--md-sys-color-error);
	}
	.destinations {
		margin-top: 2rem;
		max-width: 44rem;
	}
	.new {
		display: grid;
		gap: 0.75rem;
		max-width: 32rem;
	}
	.check {
		display: inline-flex;
		gap: 0.375rem;
		align-items: center;
		margin-left: 0.5rem;
	}
	.error {
		color: var(--md-sys-color-error);
	}
	.muted {
		color: var(--md-sys-color-on-surface-variant);
	}
</style>
