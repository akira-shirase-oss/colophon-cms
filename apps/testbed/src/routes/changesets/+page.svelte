<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}
	interface Summary {
		id: string;
		revision: { title: string };
	}
	interface Changeset {
		id: string;
		title: string;
		hash: string;
		status: { state: string; by: string | null; self_review: boolean; edited_items: number };
		items: { kind: string; entry_id: string; title: string; edited: boolean }[];
		reviews: { id: string; decision: string; reviewer_name: string | null; current: boolean }[];
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let entries = $state<Summary[]>([]);
	let picked = $state<string[]>([]);
	// Entries to take down (D-049). Nothing stops picking one in both lists: the API's answer is the point.
	let takenDown = $state<string[]>([]);
	let title = $state('');
	let open = $state<Changeset | null>(null);
	// The hash actions send in If-Match: the one on screen, editable to try a stale one.
	let hash = $state('');
	let comment = $state('');
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
		if (projectId !== '') await loadEntries();
	}

	async function loadEntries() {
		const reply = await call('GET', `${base}/collections/articles/entries?limit=100`);
		entries = reply.status === 200 ? (reply.body as { data: Summary[] }).data : [];
		picked = [];
		takenDown = [];
		open = null;
	}

	function show(reply: Reply, ok: number, what: string) {
		if (reply.status === ok) {
			open = (reply.body as { data: Changeset }).data;
			hash = open.hash;
			message = what;
		} else {
			message = describe(reply);
		}
	}

	async function create(event: SubmitEvent) {
		event.preventDefault();
		const reply = await call('POST', `${base}/changesets`, {
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), 'Idempotency-Key': crypto.randomUUID() },
			body: JSON.stringify({ title, entries: picked, unpublish: takenDown })
		});
		show(reply, 201, 'created');
	}

	async function reload() {
		if (open === null) return;
		show(await call('GET', `${base}/changesets/${open.id}`), 200, 'loaded');
	}

	async function act(action: 'submit' | 'refresh' | 'reviews', decision?: string) {
		if (open === null) return;
		const reply = await call('POST', `${base}/changesets/${open.id}/${action}`, {
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), 'If-Match': `"${hash}"` },
			...(decision === undefined ? {} : { body: JSON.stringify({ decision, comment }) })
		});
		show(reply, 200, decision ?? action);
	}

	onMount(loadProjects);
</script>

<h1>Changesets</h1>
<p class="lede">
	Changesets through the real API (D-044): pin entries' current revisions, submit, and review the exact hash in
	<code>If-Match</code>. Edit the hash below to see a stale one refused; edit an article on the Entries page to make a changeset
	stale until it is refreshed. Articles picked under Unpublish pin their published revision to be taken down (D-049).
</p>

{#if signedIn === false}
	<p data-testid="signed-out">Not signed in. <a href={resolve('/session')}>Sign in on the Session page</a> first.</p>
{:else if signedIn}
	<label class="field">
		Project
		<select bind:value={projectId} onchange={loadEntries} name="project">
			{#each projects as project (project.id)}
				<option value={project.id}>{project.name} ({project.slug})</option>
			{/each}
		</select>
	</label>

	<form onsubmit={create}>
		<label class="field">Changeset title <input bind:value={title} name="title" /></label>
		<fieldset>
			<legend>Articles</legend>
			{#each entries as entry (entry.id)}
				<label><input type="checkbox" value={entry.id} bind:group={picked} /> {entry.revision.title}</label>
			{:else}
				<p class="muted">No articles in this project.</p>
			{/each}
		</fieldset>
		<fieldset>
			<legend>Unpublish</legend>
			{#each entries as entry (entry.id)}
				<label><input type="checkbox" value={entry.id} bind:group={takenDown} /> Unpublish {entry.revision.title}</label>
			{/each}
		</fieldset>
		<button type="submit">Create changeset</button>
	</form>

	{#if message}<p data-testid="message">{message}</p>{/if}

	{#if open}
		<section data-testid="changeset" data-state={open.status.state}>
			<h2>{open.title}</h2>
			<p>
				state <code data-testid="state">{open.status.state}</code>{#if open.status.by}, by {open.status.by}{/if}{#if open.status.self_review}, self-review{/if}
			</p>
			<ul>
				{#each open.items as item (item.entry_id)}
					<li>{item.title}{item.kind === 'unpublish' ? ' (unpublish)' : ''}{item.edited ? ' (moved on since pinned)' : ''}</li>
				{/each}
			</ul>
			<label class="field">If-Match hash <input bind:value={hash} name="hash" /></label>
			<label class="field">Comment <input bind:value={comment} name="comment" /></label>
			<p class="row">
				<button onclick={reload}>Reload</button>
				<button onclick={() => act('submit')}>Submit</button>
				<button onclick={() => act('refresh')}>Refresh</button>
				<button onclick={() => act('reviews', 'approve')}>Approve</button>
				<button onclick={() => act('reviews', 'request_changes')}>Request changes</button>
			</p>
			<ol data-testid="reviews">
				{#each open.reviews as review (review.id)}
					<li>{review.decision} by {review.reviewer_name}{review.current ? '' : ' (earlier hash)'}</li>
				{/each}
			</ol>
		</section>
	{/if}
{/if}

<style>
	.lede {
		color: var(--muted);
		max-width: 46rem;
	}
	.muted {
		color: var(--muted);
	}
	form,
	section {
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
	fieldset {
		display: grid;
		gap: 0.25rem;
	}
	input,
	select {
		font: inherit;
		padding: 0.4rem 0.5rem;
	}
	.row {
		display: flex;
		flex-wrap: wrap;
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
</style>
