<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}
	interface RevisionMeta {
		id: string;
		base_revision_id: string | null;
		created_at: string;
		content_hash: string;
		title: string;
	}
	interface Summary {
		id: string;
		updated_at: string;
		revision: RevisionMeta;
	}
	interface Paragraph {
		type: 'paragraph';
		content?: { type: 'text'; text: string }[];
	}
	interface Draft {
		id: string;
		revision: RevisionMeta;
		content: { title: string; body: { doc: { content: Paragraph[] } } };
	}
	/** The last save, kept so it can be sent again exactly, with the same key. */
	interface SaveRequest {
		path: string;
		etag: string;
		key: string;
		body: string;
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let entries = $state<Summary[]>([]);
	let newTitle = $state('');
	let newText = $state('');
	let open = $state<Draft | null>(null);
	let etag = $state('');
	let title = $state('');
	let text = $state('');
	let history = $state<RevisionMeta[]>([]);
	let message = $state('');
	let lastSave = $state<SaveRequest | null>(null);

	const base = $derived(`/api/v1/projects/${projectId}/environments/production/collections/articles/entries`);

	/** Plain text to a colophon/rich-text v1 document: one paragraph per non-empty line. */
	function richText(plain: string) {
		const lines = plain.split('\n').filter((line) => line.trim() !== '');
		const content: Paragraph[] =
			lines.length === 0 ? [{ type: 'paragraph' }] : lines.map((line) => ({ type: 'paragraph', content: [{ type: 'text', text: line }] }));
		return { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content } };
	}

	function plainText(draft: Draft): string {
		return draft.content.body.doc.content.map((paragraph) => paragraph.content?.map((node) => node.text).join('') ?? '').join('\n');
	}

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason}${details.pointer ? ` at ${details.pointer}` : ''})` : ''}`;
	}

	function writeHeaders(extra: Record<string, string>): Record<string, string> {
		return { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), ...extra };
	}

	async function loadProjects() {
		const reply = await call('GET', '/api/v1/projects');
		signedIn = reply.status === 200;
		projects = signedIn ? (reply.body as { data: Project[] }).data : [];
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
		if (projectId !== '') await loadEntries();
	}

	async function loadEntries() {
		const reply = await call('GET', base);
		entries = reply.status === 200 ? (reply.body as { data: Summary[] }).data : [];
	}

	async function openEntry(id: string) {
		const reply = await call('GET', `${base}/${id}`);
		if (reply.status !== 200) {
			message = describe(reply);
			return;
		}
		open = (reply.body as { data: Draft }).data;
		etag = reply.headers.get('etag') ?? '';
		title = open.content.title;
		text = plainText(open);
		lastSave = null;
		const revisions = await call('GET', `${base}/${id}/revisions`);
		history = revisions.status === 200 ? (revisions.body as { data: RevisionMeta[] }).data : [];
	}

	async function create(event: SubmitEvent) {
		event.preventDefault();
		const reply = await call('POST', base, {
			headers: writeHeaders({ 'Idempotency-Key': crypto.randomUUID() }),
			body: JSON.stringify({ title: newTitle, body: richText(newText) })
		});
		if (reply.status === 201) {
			const draft = (reply.body as { data: Draft }).data;
			message = `created ${draft.content.title}`;
			newTitle = '';
			newText = '';
			await loadEntries();
			await openEntry(draft.id);
		} else {
			message = describe(reply);
		}
	}

	async function send(request: SaveRequest) {
		const reply = await call('PUT', request.path, {
			headers: writeHeaders({ 'If-Match': request.etag, 'Idempotency-Key': request.key }),
			body: request.body
		});
		if (reply.status === 200) {
			const draft = (reply.body as { data: Draft }).data;
			const replayed = reply.headers.get('idempotent-replayed') === 'true';
			message = `${replayed ? 'replayed' : 'saved'} revision ${draft.revision.id}`;
			await loadEntries();
			const keep = request;
			await openEntry(draft.id);
			lastSave = keep;
		} else {
			message = describe(reply);
		}
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		if (open === null) return;
		const request = {
			path: `${base}/${open.id}`,
			etag,
			key: crypto.randomUUID(),
			body: JSON.stringify({ title, body: richText(text) })
		};
		lastSave = request;
		await send(request);
	}

	onMount(loadProjects);
</script>

<h1>Entries</h1>
<p class="lede">
	Articles saved through the real API. Every create and save carries an <code>Idempotency-Key</code>; every save carries
	<code>If-Match</code> with the revision you loaded, so a save on top of someone else's is refused instead of overwriting it.
</p>

{#if signedIn === false}
	<p data-testid="signed-out">Not signed in. <a href={resolve('/session')}>Sign in on the Session page</a> first.</p>
{:else if signedIn}
	<label class="project">
		Project
		<select bind:value={projectId} onchange={loadEntries} name="project">
			{#each projects as project (project.id)}
				<option value={project.id}>{project.name} ({project.slug})</option>
			{/each}
		</select>
	</label>

	<section>
		<h2>New article</h2>
		<form onsubmit={create}>
			<label>New title <input bind:value={newTitle} name="new-title" /></label>
			<label>New text <textarea bind:value={newText} name="new-text" rows="3"></textarea></label>
			<button type="submit">Create article</button>
		</form>
	</section>

	{#if message}<p data-testid="message">{message}</p>{/if}

	<section>
		<h2>Articles</h2>
		<ul data-testid="entries">
			{#each entries as entry (entry.id)}
				<li data-id={entry.id}><button class="link" onclick={() => openEntry(entry.id)}>{entry.revision.title}</button></li>
			{:else}
				<li>None yet.</li>
			{/each}
		</ul>
	</section>

	{#if open}
		<section data-testid="editor" data-etag={etag}>
			<h2>Editing</h2>
			<p class="meta">Revision <code data-testid="revision">{open.revision.id}</code>, <code>{open.revision.content_hash}</code></p>
			<form onsubmit={save}>
				<label>Title <input bind:value={title} name="title" /></label>
				<label>Text <textarea bind:value={text} name="text" rows="4"></textarea></label>
				<button type="submit">Save</button>
				<button type="button" disabled={lastSave === null} onclick={() => lastSave && send(lastSave)}>Retry last save</button>
				<button type="button" onclick={() => open && openEntry(open.id)}>Reload</button>
			</form>
			<h3>History</h3>
			<ol data-testid="history">
				{#each history as revision (revision.id)}
					<li data-id={revision.id}>{revision.title} <small>{revision.created_at}</small></li>
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
	section {
		margin-top: 1.5rem;
	}
	form {
		display: flex;
		gap: 0.75rem;
		flex-wrap: wrap;
		align-items: end;
	}
	label {
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
	button {
		font: inherit;
		padding: 0.5rem 1rem;
		border-radius: 8px;
		border: 1px solid var(--line);
		background: var(--fg);
		color: var(--bg);
		cursor: pointer;
	}
	button:disabled {
		opacity: 0.5;
		cursor: default;
	}
	button.link {
		background: none;
		color: inherit;
		border: none;
		padding: 0;
		text-decoration: underline;
	}
	.meta {
		font-size: 0.85rem;
		color: var(--muted);
	}
</style>
