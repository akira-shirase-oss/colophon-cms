<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}
	interface Destination {
		id: string;
		name: string;
		url: string;
		enabled: boolean;
	}
	interface Job {
		id: string;
		destination_name: string;
		generation: number;
		status: string;
		attempts: number;
		last_http_status: number | null;
		last_error: string | null;
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let destinations = $state<Destination[]>([]);
	let jobs = $state<Job[]>([]);
	let name = $state('');
	let url = $state('');
	let message = $state('');

	const api = $derived(`/api/v1/projects/${projectId}/environments/production`);

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason})` : ''}`.trim();
	}

	async function loadProjects() {
		const reply = await call('GET', '/api/v1/projects');
		signedIn = reply.status === 200;
		projects = signedIn ? (reply.body as { data: Project[] }).data : [];
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
		if (projectId !== '') await load();
	}

	async function load() {
		const listed = await call('GET', `${api}/destinations`);
		destinations = listed.status === 200 ? (listed.body as { data: Destination[] }).data : [];
		const recent = await call('GET', `${api}/jobs`);
		jobs = recent.status === 200 ? (recent.body as { data: Job[] }).data : [];
	}

	async function add(event: SubmitEvent) {
		event.preventDefault();
		const reply = await call('POST', `${api}/destinations`, {
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken() },
			body: JSON.stringify({ name, url })
		});
		message = reply.status === 201 ? 'added' : describe(reply);
		await load();
	}

	onMount(loadProjects);
</script>

<h1>Deliveries</h1>
<p class="lede">
	Where releases are announced, through the real API (D-046): add a webhook destination, publish on the Releases page, and watch
	each release's job move from pending to delivered, retried or failed. Every attempt carries the same idempotency key.
</p>

{#if signedIn === false}
	<p data-testid="signed-out">Not signed in. <a href={resolve('/session')}>Sign in on the Session page</a> first.</p>
{:else if signedIn}
	<label class="field">
		Project
		<select bind:value={projectId} onchange={load} name="project">
			{#each projects as project (project.id)}
				<option value={project.id}>{project.name} ({project.slug})</option>
			{/each}
		</select>
	</label>

	<form onsubmit={add}>
		<label class="field">Destination name <input bind:value={name} name="name" /></label>
		<label class="field">Webhook URL <input bind:value={url} name="url" /></label>
		<button type="submit">Add destination</button>
	</form>
	{#if message}<p data-testid="message">{message}</p>{/if}

	<h2>Destinations</h2>
	<ul data-testid="destinations">
		{#each destinations as destination (destination.id)}
			<li>{destination.name}: <code>{destination.url}</code>{destination.enabled ? '' : ' (off)'}</li>
		{/each}
	</ul>

	<h2>Jobs <button onclick={load}>Reload</button></h2>
	<ul data-testid="jobs">
		{#each jobs as job (job.id)}
			<li data-status={job.status}>
				generation {job.generation} to {job.destination_name}: {job.status}, {job.attempts}
				{job.attempts === 1 ? 'attempt' : 'attempts'}{job.last_http_status ? `, HTTP ${job.last_http_status}` : ''}
			</li>
		{/each}
	</ul>
{/if}

<style>
	.lede {
		color: var(--muted);
		max-width: 46rem;
	}
	form {
		display: grid;
		gap: 0.75rem;
		max-width: 46rem;
		margin-top: 1rem;
	}
	.field {
		display: grid;
		gap: 0.25rem;
		font-size: 0.85rem;
		color: var(--muted);
	}
	input,
	select {
		font: inherit;
		padding: 0.4rem 0.5rem;
	}
	button {
		font: inherit;
		padding: 0.5rem 1rem;
		border-radius: 8px;
		border: 1px solid var(--line);
		background: var(--fg);
		color: var(--bg);
		cursor: pointer;
		justify-self: start;
	}
</style>
