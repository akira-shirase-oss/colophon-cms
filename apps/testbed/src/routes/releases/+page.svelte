<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}
	interface Release {
		generation: number;
		title: string;
		entries: number;
	}
	interface Summary {
		id: string;
		title: string;
		hash: string;
		status: { state: string };
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let generation = $state(0);
	let releases = $state<Release[]>([]);
	let changesets = $state<Summary[]>([]);
	let changesetId = $state('');
	let base = $state(0);
	let message = $state('');
	let readAt = $state('');
	let delivered = $state('');

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
		const listed = await call('GET', `${api}/releases`);
		releases = listed.status === 200 ? (listed.body as { data: Release[] }).data : [];
		generation = Number(listed.headers.get('colophon-generation') ?? '0');
		base = generation;
		const sets = await call('GET', `${api}/changesets`);
		changesets = sets.status === 200 ? (sets.body as { data: Summary[] }).data : [];
		changesetId = changesets.find((set) => set.status.state === 'approved')?.id ?? changesets[0]?.id ?? '';
	}

	async function publish(event: SubmitEvent) {
		event.preventDefault();
		const set = changesets.find((candidate) => candidate.id === changesetId);
		if (set === undefined) return;
		const reply = await call('POST', `${api}/changesets/${set.id}/publish`, {
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), 'If-Match': `"${set.hash}"`, 'Idempotency-Key': crypto.randomUUID() },
			body: JSON.stringify({ base_generation: base })
		});
		message = reply.status === 201 ? `published as generation ${(reply.body as { data: { generation: number } }).data.generation}` : describe(reply);
		await load();
	}

	async function read(event: SubmitEvent) {
		event.preventDefault();
		// No cookie: what any reader on the internet gets.
		const query = readAt === '' ? '' : `?generation=${readAt}`;
		const response = await fetch(`/server/content/v1/${projectId}/production/articles${query}`, { credentials: 'omit', cache: 'no-store' });
		const body = (await response.json()) as { data?: { title: string }[] };
		delivered = `HTTP ${response.status}, generation ${response.headers.get('colophon-generation') ?? '?'}: ${(body.data ?? []).map((entry) => entry.title).join(', ')}`;
	}

	onMount(loadProjects);
</script>

<h1>Releases</h1>
<p class="lede">
	Publishing through the real API (D-045): commit an approved changeset as the release after a base generation, see a stale base
	refused, and read <code>/content/v1</code> with no cookie, at the latest release or pinned to an earlier one.
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
	<p>Current generation: <code data-testid="generation">{generation}</code></p>
	<ol reversed data-testid="releases">
		{#each releases as release (release.generation)}
			<li>generation {release.generation}: {release.title} ({release.entries} changed)</li>
		{/each}
	</ol>

	<form onsubmit={publish}>
		<label class="field">
			Changeset
			<select bind:value={changesetId} name="changeset">
				{#each changesets as set (set.id)}
					<option value={set.id}>{set.title} ({set.status.state})</option>
				{/each}
			</select>
		</label>
		<label class="field">Base generation <input type="number" bind:value={base} name="base" /></label>
		<button type="submit">Publish</button>
	</form>
	{#if message}<p data-testid="message">{message}</p>{/if}

	<form onsubmit={read}>
		<label class="field">Read at generation (empty for the latest) <input bind:value={readAt} name="read-at" /></label>
		<button type="submit">Read published articles</button>
	</form>
	{#if delivered}<p data-testid="delivered">{delivered}</p>{/if}
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
