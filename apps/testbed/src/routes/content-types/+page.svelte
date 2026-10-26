<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}
	interface TypeVersion {
		slug: string;
		version: number;
		definition: { name: string; fields: unknown[] };
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let types = $state<TypeVersion[]>([]);
	let slug = $state('');
	let definition = $state('');
	let editing = $state('');
	let etag = $state('');
	let edited = $state('');
	let message = $state('');

	const base = $derived(`/api/v1/projects/${projectId}/environments/production/content-types`);

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason}${details.pointer ? ` at ${details.pointer}` : ''})` : ''}`;
	}

	function headers(extra: Record<string, string>): Record<string, string> {
		return { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), 'Idempotency-Key': crypto.randomUUID(), ...extra };
	}

	async function loadProjects() {
		const reply = await call('GET', '/api/v1/projects');
		signedIn = reply.status === 200;
		projects = signedIn ? (reply.body as { data: Project[] }).data : [];
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
		if (projectId !== '') await loadTypes();
	}

	async function loadTypes() {
		const reply = await call('GET', base);
		types = reply.status === 200 ? (reply.body as { data: TypeVersion[] }).data : [];
	}

	async function create(event: SubmitEvent) {
		event.preventDefault();
		let parsed: unknown;
		try {
			parsed = JSON.parse(definition);
		} catch {
			message = 'the definition is not JSON';
			return;
		}
		const reply = await call('POST', base, { headers: headers({}), body: JSON.stringify({ slug, definition: parsed }) });
		message = reply.status === 201 ? `created ${slug} v${(reply.body as { data: TypeVersion }).data.version}` : describe(reply);
		await loadTypes();
	}

	async function open(type: string) {
		const reply = await call('GET', `${base}/${type}`);
		if (reply.status !== 200) {
			message = describe(reply);
			return;
		}
		editing = type;
		etag = reply.headers.get('etag') ?? '';
		edited = JSON.stringify((reply.body as { data: TypeVersion }).data.definition, null, 2);
	}

	async function update(event: SubmitEvent) {
		event.preventDefault();
		const reply = await call('PUT', `${base}/${editing}`, { headers: headers({ 'If-Match': etag }), body: edited });
		if (reply.status === 200) {
			message = `updated ${editing} to v${(reply.body as { data: TypeVersion }).data.version}`;
		} else {
			message = describe(reply);
		}
		await loadTypes();
	}

	onMount(loadProjects);
</script>

<h1>Content types</h1>
<p class="lede">
	Content types through the real API: create one from a JSON definition, then change it with <code>If-Match</code>. Only changes
	that keep every stored entry valid are accepted; anything else needs a migration.
</p>

{#if signedIn === false}
	<p data-testid="signed-out">Not signed in. <a href={resolve('/session')}>Sign in on the Session page</a> first.</p>
{:else if signedIn}
	<label class="field">
		Project
		<select bind:value={projectId} onchange={loadTypes} name="project">
			{#each projects as project (project.id)}
				<option value={project.id}>{project.name} ({project.slug})</option>
			{/each}
		</select>
	</label>

	<ul data-testid="types">
		{#each types as type (type.slug)}
			<li data-slug={type.slug}>
				<button class="link" onclick={() => open(type.slug)}>{type.definition.name}</button>
				<code>{type.slug}</code> v{type.version}, {type.definition.fields.length} fields
			</li>
		{/each}
	</ul>

	{#if message}<p data-testid="message">{message}</p>{/if}

	<section>
		<h2>New content type</h2>
		<form onsubmit={create}>
			<label class="field">Slug <input bind:value={slug} name="slug" /></label>
			<label class="field">Definition <textarea bind:value={definition} name="definition" rows="8"></textarea></label>
			<button type="submit">Create content type</button>
		</form>
	</section>

	{#if editing}
		<section data-testid="editing" data-etag={etag}>
			<h2>Change {editing}</h2>
			<form onsubmit={update}>
				<label class="field">Current definition <textarea bind:value={edited} name="edited" rows="12"></textarea></label>
				<button type="submit">Save definition</button>
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
	textarea,
	select {
		font: inherit;
		padding: 0.4rem 0.5rem;
	}
	textarea {
		font-family: ui-monospace, monospace;
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
