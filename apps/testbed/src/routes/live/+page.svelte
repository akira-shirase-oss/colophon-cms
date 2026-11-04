<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, listProjects, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}
	interface Listed {
		id: string;
		revision: { id: string; title: string };
	}
	interface Row {
		id: number;
		at: string;
		event: string;
		data: string;
		/** Milliseconds from the hint arriving to the refetch it caused landing; null while it
		 *  runs, -1 for events that refetch nothing here. */
		ms: number | null;
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let entries = $state<Listed[]>([]);
	let comments = $state<number | null>(null);
	let stream = $state<'closed' | 'connecting' | 'live'>('closed');
	let log = $state<Row[]>([]);
	let message = $state('');
	let writes = 0;

	const base = $derived(`/api/v1/projects/${projectId}/environments/production`);

	let source: EventSource | null = null;

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason})` : ''}`.trim();
	}
	const write = () => ({
		'Content-Type': 'application/json',
		'X-CSRF-Token': csrfToken(),
		'Idempotency-Key': crypto.randomUUID()
	});

	let seq = 0;
	/** A line in the event log; `ms` fills in when the refetch the event caused lands. */
	function note(event: string, data: string): number {
		log = [{ id: ++seq, at: new Date().toISOString().slice(11, 23), event, data, ms: null }, ...log].slice(0, 50);
		return seq;
	}

	/** Time a refetch an event caused, written back on its log row. */
	function timed(id: number, refetch: () => Promise<void>) {
		const started = performance.now();
		void refetch().then(() => {
			const row = log.find((item) => item.id === id);
			if (row !== undefined) row.ms = Math.round(performance.now() - started);
		});
	}

	async function loadEntries() {
		const reply = await call('GET', `${base}/collections/articles/entries?limit=100`);
		if (reply.status === 200) entries = (reply.body as { data: Listed[] }).data;
	}

	async function loadComments() {
		const first = entries[0];
		if (first === undefined) {
			comments = null;
			return;
		}
		const reply = await call('GET', `${base}/collections/articles/entries/${first.id}/comments?limit=100`);
		if (reply.status === 200) comments = (reply.body as { data: unknown[] }).data.length;
	}

	/** A `change` hint: log it, then refetch what this page shows — the entries list, or the
	 *  first entry's comment count for a `comments` hint — and time hint → refetched. */
	function hinted(event: MessageEvent<string>) {
		const id = note('change', event.data);
		let kind = '';
		try {
			kind = (JSON.parse(event.data) as { k?: string }).k ?? '';
		} catch {
			kind = '';
		}
		timed(id, kind === 'comments' ? loadComments : loadEntries);
	}

	function listen() {
		source?.close();
		log = [];
		stream = 'connecting';
		const events = new EventSource(`/server${base}/events`);
		source = events;
		events.addEventListener('open', () => {
			stream = 'live';
		});
		events.addEventListener('hello', (event) => {
			const id = note('hello', event.data);
			const row = log.find((item) => item.id === id);
			if (row !== undefined) row.ms = -1;
		});
		events.addEventListener('change', hinted);
		events.addEventListener('resync', (event) => timed(note('resync', event.data), loadEntries));
		// The stream ending (lifetime, or a lost connection) arrives as an error; EventSource
		// reconnects on its own and `open` says when it is back.
		events.addEventListener('error', () => {
			stream = source?.readyState === EventSource.OPEN ? 'live' : 'connecting';
		});
	}

	async function loadProjects() {
		const reply = await listProjects<Project>();
		signedIn = reply.status === 200;
		projects = reply.projects;
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
		if (projectId !== '') await loadProject();
	}

	async function loadProject() {
		listen();
		await loadEntries();
		await loadComments();
	}

	async function createEntry() {
		writes += 1;
		const reply = await call('POST', `${base}/collections/articles/entries`, {
			headers: write(),
			body: JSON.stringify({
				title: `Live note ${writes}`,
				body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } }
			})
		});
		message = reply.status === 201 ? `entry ${writes} created` : describe(reply);
	}

	async function addComment() {
		const first = entries[0];
		if (first === undefined) {
			message = 'no entry to comment on';
			return;
		}
		writes += 1;
		const reply = await call('POST', `${base}/collections/articles/entries/${first.id}/comments`, {
			headers: write(),
			body: JSON.stringify({ revision: first.revision.id, body: `A comment ${writes}` })
		});
		message = reply.status === 201 ? `comment ${writes} added` : describe(reply);
	}

	onMount(loadProjects);
	onDestroy(() => source?.close());
</script>

<h1>Live lists</h1>
<p class="lede">
	<code>GET {'{P}'}/events</code> through the real API (E3, D-108): the page holds a server-sent-events stream and each
	<code>change</code> hint — a kind and a scope, never content — makes it refetch through the same authorized API. The
	log shows every frame and how long the refetch it caused took. The writes below are ordinary commits; the stream
	says they happened.
</p>

{#if signedIn === false}
	<p data-testid="signed-out">Not signed in. <a href={resolve('/session')}>Sign in on the Session page</a> first.</p>
{:else if signedIn}
	<label class="field">
		Project
		<select bind:value={projectId} onchange={loadProject} name="project">
			{#each projects as project (project.id)}
				<option value={project.id}>{project.name} ({project.slug})</option>
			{/each}
		</select>
	</label>

	<p data-testid="stream-state" data-state={stream}>stream: {stream}</p>
	<p class="row">
		<button onclick={createEntry} data-testid="create-entry">Create an entry</button>
		<button onclick={addComment} data-testid="add-comment" disabled={entries.length === 0}>Comment on the first entry</button>
	</p>
	{#if message}<p data-testid="message">{message}</p>{/if}

	<section data-testid="entries-panel">
		<h2>Articles <span class="muted">({entries.length})</span></h2>
		{#if comments !== null}<p class="muted" data-testid="comments-count">{comments} comment{comments === 1 ? '' : 's'} on {entries[0]?.revision.title}</p>{/if}
		<ol data-testid="entries">
			{#each entries as entry (entry.id)}
				<li data-testid="entry">{entry.revision.title}</li>
			{:else}
				<li class="muted">No articles yet.</li>
			{/each}
		</ol>
	</section>

	<section>
		<h2>Event log</h2>
		<ol data-testid="events" class="log">
			{#each log as row (row)}
				<li data-testid="event" data-event={row.event}>
					<code>{row.at}</code> <strong>{row.event}</strong> <code>{row.data}</code>
					{#if row.ms === null}
						<span class="muted" data-testid="latency">refetching…</span>
					{:else if row.ms >= 0}
						<span data-testid="latency">→ refreshed in {row.ms} ms</span>
					{/if}
				</li>
			{:else}
				<li class="muted">Nothing yet — the stream's hello arrives first.</li>
			{/each}
		</ol>
	</section>
{/if}

<style>
	.lede {
		color: var(--muted);
		max-width: 46rem;
	}
	.muted {
		color: var(--muted);
	}
	section {
		display: grid;
		gap: 0.75rem;
		max-width: 46rem;
		margin-top: 1rem;
	}
	.field {
		display: grid;
		gap: 0.25rem;
		font-size: 0.875rem;
		color: var(--muted);
		max-width: 24rem;
	}
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
	}
	button:disabled {
		opacity: 0.5;
		cursor: default;
	}
	.log {
		font-size: 0.875rem;
		display: grid;
		gap: 0.25rem;
	}
	.log li {
		list-style: none;
	}
</style>
