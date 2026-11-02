<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, listProjects, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let entryId = $state('');
	let siblingId = $state('');
	let group = $state('');
	let repeat = $state('');
	let frenchList = $state('');
	let message = $state('');

	const env = $derived(`/api/v1/projects/${projectId}/environments/production`);

	const NOTES = {
		name: 'Notes',
		title_field: 'title',
		fields: [
			{ id: 'title', label: 'Title', type: 'text', required: true },
			{ id: 'slug', label: 'Slug', type: 'slug' }
		]
	};

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason}${details.pointer ? ` at ${details.pointer}` : ''})` : ''}`.trim();
	}

	function headers(extra: Record<string, string> = {}): Record<string, string> {
		return { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), 'Idempotency-Key': crypto.randomUUID(), ...extra };
	}

	async function loadProjects() {
		const reply = await listProjects<Project>();
		signedIn = reply.status === 200;
		projects = reply.projects;
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
	}

	/** The collection and its English entry, localized once so the group exists. */
	async function setUp() {
		const made = await call('POST', `${env}/content-types`, { headers: headers(), body: JSON.stringify({ slug: 'notes', definition: NOTES }) });
		if (made.status !== 201) {
			message = `type: ${describe(made)}`;
			return;
		}
		const created = await call('POST', `${env}/collections/notes/entries?locale=en`, {
			headers: headers(),
			body: JSON.stringify({ title: 'Welcome', slug: 'welcome' })
		});
		if (created.status !== 201) {
			message = `entry: ${describe(created)}`;
			return;
		}
		entryId = (created.body as { data: { id: string } }).data.id;
		const localized = await call('POST', `${env}/collections/notes/entries/${entryId}/localizations?locale=fr`, { headers: headers() });
		if (localized.status !== 201) {
			message = `localize: ${describe(localized)}`;
			return;
		}
		siblingId = (localized.body as { data: { id: string } }).data.id;
		await readGroup();
		message = 'set up';
	}

	/** The source's group, as `locale → id` lines. */
	async function readGroup() {
		const read = await call('GET', `${env}/collections/notes/entries/${entryId}`);
		const localizations = (read.body as { data: { localizations: { id: string; locale: string }[] } }).data.localizations;
		group = localizations.map((row) => `${row.locale} → ${row.id}`).join(', ');
	}

	/** A sibling in a locale the group already holds must be refused. */
	async function repeatIt() {
		const again = await call('POST', `${env}/collections/notes/entries/${entryId}/localizations?locale=fr`, { headers: headers() });
		repeat = describe(again);
	}

	/** The list's locale filter, keeping only the French row. */
	async function listFrench() {
		const listed = await call('GET', `${env}/collections/notes/entries?limit=100&locale=fr`);
		const rows = (listed.body as { data: { id: string; locale: string }[] }).data;
		frenchList = rows.map((row) => `${row.locale} → ${row.id}`).join(', ') || '(none)';
	}

	onMount(loadProjects);
</script>

<h1>Locales</h1>
<p class="lede">
	Entries carry a locale tag, and a localization group holds one entry per tag (D-087): an English note, its French sibling made
	by copying the draft, the group's list on each member, the refused duplicate, and the list's locale filter — all through the
	real API.
</p>

{#if signedIn === false}
	<p data-testid="signed-out">Not signed in. <a href={resolve('/session')}>Sign in on the Session page</a> first.</p>
{:else if signedIn}
	{#if message}<p data-testid="message">{message}</p>{/if}
	{#if projects.length > 0}
		<section>
			<label class="field">
				Project
				<select bind:value={projectId} name="project">
					{#each projects as project (project.id)}
						<option value={project.id}>{project.name} ({project.slug})</option>
					{/each}
				</select>
			</label>
			<p>
				<button type="button" onclick={setUp}>Set up an English note and its French version</button>
				<button type="button" onclick={repeatIt} disabled={!siblingId}>Localize it in French again</button>
				<button type="button" onclick={listFrench} disabled={!siblingId}>List the French entries</button>
			</p>
			{#if entryId}<p>English: <code data-testid="entry-id">{entryId}</code></p>{/if}
			{#if siblingId}<p>French: <code data-testid="sibling-id">{siblingId}</code></p>{/if}
			{#if group}<p>Group: <span data-testid="group">{group}</span></p>{/if}
			{#if repeat}<p>Again: <span data-testid="repeat">{repeat}</span></p>{/if}
			{#if frenchList}<p>French list: <span data-testid="french-list">{frenchList}</span></p>{/if}
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
	.field {
		display: grid;
		gap: 0.25rem;
		font-size: 0.875rem;
		color: var(--muted);
		max-width: 46rem;
	}
	select {
		font: inherit;
		padding: 0.4rem 0.5rem;
	}
</style>
