<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}
	interface Schedule {
		id: string;
		changeset_id: string;
		title: string;
		local_time: string;
		time_zone: string;
		run_at: string;
		state: string;
		reason?: string;
		generation?: number;
		delayed: boolean;
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let schedules = $state<Schedule[]>([]);
	let changeset = $state('');
	let hash = $state('');
	let localTime = $state('');
	let zone = $state('UTC');
	let choice = $state('');
	let candidates = $state<string[]>([]);
	let message = $state('');

	const base = $derived(`/api/v1/projects/${projectId}/environments/production`);

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
		const listed = await call('GET', `${base}/schedules`);
		schedules = listed.status === 200 ? (listed.body as { data: Schedule[] }).data : [];
	}

	async function schedule() {
		const body = { local_time: localTime, time_zone: zone, ...(choice === '' ? {} : { choice }) };
		const reply = await call('POST', `${base}/changesets/${changeset}/schedule`, {
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), 'If-Match': `"${hash}"` },
			body: JSON.stringify(body)
		});
		candidates = (reply.body as { error?: { details?: { candidates?: string[] } } } | null)?.error?.details?.candidates ?? [];
		message = reply.status === 201 ? `scheduled for ${(reply.body as { data: Schedule }).data.run_at}` : describe(reply);
		await load();
	}

	async function cancel(id: string) {
		const reply = await call('DELETE', `${base}/schedules/${id}`, { headers: { 'X-CSRF-Token': csrfToken() } });
		message = reply.status === 200 ? 'cancelled' : describe(reply);
		await load();
	}

	onMount(loadProjects);
</script>

<h1>Schedules</h1>
<p class="lede">
	Scheduled publishing through the real API (D-060): schedule an approved changeset at a local time in an IANA time zone, see a time the
	clocks show twice or skip refused with both candidate instants until a choice is given, watch the worker publish it, and cancel a
	pending one.
</p>

{#if signedIn === false}
	<p data-testid="signed-out">Not signed in. <a href={resolve('/session')}>Sign in on the Session page</a> first.</p>
{:else if signedIn}
	{#if message}<p data-testid="message">{message}</p>{/if}
	{#if candidates.length > 0}<p data-testid="candidates">{candidates.join(' or ')}</p>{/if}
	{#if projects.length > 0}
		<section>
			<label class="field">
				Project
				<select bind:value={projectId} onchange={load} name="project">
					{#each projects as project (project.id)}
						<option value={project.id}>{project.name} ({project.slug})</option>
					{/each}
				</select>
			</label>
			<button type="button" onclick={load}>Refresh</button>
			<ul data-testid="schedules">
				{#each schedules as item (item.id)}
					<li data-state={item.state}>
						{item.title}: {item.local_time} {item.time_zone} ({item.run_at}) {item.state}{item.generation ? ` as release ${item.generation}` : ''}{item.delayed
							? ', delayed'
							: ''}{item.reason ? `: ${item.reason}` : ''}
						{#if item.state === 'scheduled'}<button class="link" onclick={() => cancel(item.id)}>Cancel {item.title}</button>{/if}
					</li>
				{/each}
			</ul>
		</section>

		<section>
			<h2>Schedule</h2>
			<form onsubmit={(event) => event.preventDefault()}>
				<label class="field">Changeset id <input bind:value={changeset} name="changeset" /></label>
				<label class="field">If-Match hash <input bind:value={hash} name="hash" /></label>
				<label class="field">Local time <input bind:value={localTime} name="local_time" placeholder="2026-11-03T09:00" /></label>
				<label class="field">Time zone <input bind:value={zone} name="time_zone" /></label>
				<label class="field">
					Choice
					<select bind:value={choice} name="choice">
						<option value="">none</option>
						<option value="earlier">earlier</option>
						<option value="later">later</option>
					</select>
				</label>
				<button type="button" onclick={schedule}>Schedule</button>
			</form>
		</section>
	{/if}
{/if}

<style>
	.lede {
		color: var(--muted);
		max-width: 46rem;
	}
	section {
		margin-top: 1.5rem;
	}
	form {
		display: grid;
		gap: 0.75rem;
		max-width: 46rem;
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
	ul {
		display: grid;
		gap: 0.5rem;
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
	button.link {
		background: none;
		color: inherit;
		border: none;
		padding: 0;
		text-decoration: underline;
	}
</style>
