<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, listProjects, type Reply } from '#lib/api.ts';
	import { latestFor } from '#lib/latest.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}
	interface Release {
		generation: number;
		title: string;
		entries: number;
		unpublished: number;
		purge: string;
		label: string;
		locations: number;
	}
	interface Receipt extends Release {
		deliveries: { name: string; kind: string; state: string }[];
		observations: { location: string; observed_generation: number | null; http_status: number | null }[];
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
	let readEntry = $state('');
	let receiptOf = $state('');
	let receipt = $state('');
	let delivered = $state('');
	let restoreOf = $state('');
	let restoreTitle = $state('');
	let restored = $state('');
	let previewed = $state('');
	let previewHash = $state<string | null>(null);
	let changesOf = $state('');
	let changesSize = $state('');
	let changesCursor = $state<string | null>(null);
	let changesRead = $state('');
	let undoOf = $state('');
	let undoKey = $state('');
	let undone = $state('');
	let pageSize = $state('');
	let pageCursor = $state<string | null>(null);
	let paged = $state('');

	const api = $derived(`/api/v1/projects/${projectId}/environments/production`);

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason})` : ''}`.trim();
	}

	async function loadProjects() {
		const reply = await listProjects<Project>();
		signedIn = reply.status === 200;
		projects = reply.projects;
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
		if (projectId !== '') await load();
	}

	// Each load reads one project and is applied only if no later load started: the first load
	// (of the first project, from onMount) used to finish after a project was picked — reading
	// the picked project's changesets half-way, since `api` had moved — and reset the changeset
	// just chosen, so Publish released another one.
	// Latest load wins: an answer for an older load, or another project, is dropped (#lib/latest.ts).
	const loads = latestFor(() => projectId);
	async function load() {
		const current = loads.start();
		const at = api;
		const listed = await call('GET', `${at}/releases`);
		const sets = await call('GET', `${at}/changesets`);
		if (!current()) return;
		releases = listed.status === 200 ? (listed.body as { data: Release[] }).data : [];
		generation = Number(listed.headers.get('colophon-generation') ?? '0');
		base = generation;
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
		const one = readEntry.trim() === '' ? '' : `/${readEntry.trim()}`;
		const response = await fetch(`/server/content/v1/${projectId}/production/articles${one}${query}`, { credentials: 'omit', cache: 'no-store' });
		const body = (await response.json()) as { data?: { title: string }[] | { title: string } };
		const error = envelopeOf(body);
		const titles = Array.isArray(body.data) ? body.data.map((entry) => entry.title).join(', ') : (body.data?.title ?? '');
		delivered = error
			? `HTTP ${response.status}, ${error.code}`
			: `HTTP ${response.status}, generation ${response.headers.get('colophon-generation') ?? '?'}: ${titles}`;
	}

	// What a rollback would do, before it is made (D-185): its totals, what stays down, its hash.
	async function preview() {
		const reply = await call('GET', `${api}/releases/${restoreOf.trim()}/rollback`);
		if (reply.status !== 200) {
			previewHash = null;
			previewed = describe(reply);
			return;
		}
		const shown = (reply.body as { data: { publish_total: number; unpublish_total: number; kept_down_total: number; too_large: boolean; max_items: number; hash: string | null } }).data;
		previewHash = shown.hash;
		previewed = `puts back ${shown.publish_total}, takes down ${shown.unpublish_total}, keeps down ${shown.kept_down_total}${shown.too_large ? `, too large (over ${shown.max_items})` : ''}`;
	}

	// Rolls back as previewed: the preview's hash in If-Match, refused 409 if the pins moved.
	async function rollbackAsPreviewed() {
		await rollbackWith(previewHash === null ? {} : { 'If-Match': `"${previewHash}"` });
	}

	async function rollback(event: SubmitEvent) {
		event.preventDefault();
		await rollbackWith({});
	}

	async function rollbackWith(extra: Record<string, string>) {
		const reply = await call('POST', `${api}/releases/${restoreOf.trim()}/rollback`, {
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), 'Idempotency-Key': crypto.randomUUID(), ...extra },
			body: JSON.stringify({ title: restoreTitle })
		});
		if (reply.status !== 201) {
			restored = describe(reply);
			return;
		}
		const made = (reply.body as { data: { id: string; status: { state: string } } }).data;
		restored = `restore changeset ${made.id} (${made.status.state})`;
		await load();
	}

	// Undo of a takedown (D-184): one request, under a key this form keeps until it is changed, so
	// sending it twice shows the replay.
	async function undo(event: SubmitEvent) {
		event.preventDefault();
		if (undoKey.trim() === '') undoKey = crypto.randomUUID().replaceAll('-', '');
		const reply = await call('POST', `${api}/releases/${undoOf.trim()}/undo`, {
			headers: { 'X-CSRF-Token': csrfToken(), 'Idempotency-Key': undoKey.trim() }
		});
		if (reply.status !== 201) {
			undone = describe(reply);
			return;
		}
		const made = (reply.body as { data: { generation: number; undo_of: number; entries_restored: number } }).data;
		const replayed = reply.headers.get('idempotent-replayed') === 'true';
		undone = `undid ${made.undo_of} as generation ${made.generation}, ${made.entries_restored} back${replayed ? ' (replayed)' : ''}`;
		await load();
	}

	async function readReceipt(event: SubmitEvent) {
		event.preventDefault();
		const reply = await call('GET', `${api}/releases/${receiptOf.trim()}`);
		if (reply.status !== 200) {
			receipt = describe(reply);
			return;
		}
		const shown = (reply.body as { data: Receipt }).data;
		const parts = shown.deliveries.map((delivery) => `${delivery.name} ${delivery.state}`).join(', ');
		const seen = shown.observations.map((look) => `${look.location} saw ${look.observed_generation ?? 'none'} (HTTP ${look.http_status ?? '-'})`);
		receipt = `${shown.label} at ${shown.locations}; ${parts || 'no destinations'}; ${seen.length} looks${seen.length > 0 ? `, last: ${seen[seen.length - 1]}` : ''}`;
	}

	// The list a page at a time (`?limit=`, `?cursor=`): which generations each page held and
	// whether another follows.
	async function readPage(next: boolean) {
		const query = new URLSearchParams();
		if (pageSize.trim() !== '') query.set('limit', pageSize.trim());
		if (next && pageCursor !== null) query.set('cursor', pageCursor);
		const reply = await call('GET', `${api}/releases${query.size > 0 ? `?${query}` : ''}`);
		if (reply.status !== 200) {
			pageCursor = null;
			paged = describe(reply);
			return;
		}
		const shown = reply.body as { data: Release[]; next_cursor: string | null };
		pageCursor = shown.next_cursor;
		const generations = shown.data.map((release) => release.generation).join(', ');
		paged = `generations ${generations || 'none'}; ${pageCursor === null ? 'last page' : 'more'}`;
	}

	// A release's changes a page at a time (D-185): which entries each page held, and whether
	// another follows.
	async function readChanges(next: boolean) {
		const query = new URLSearchParams();
		if (changesSize.trim() !== '') query.set('limit', changesSize.trim());
		if (next && changesCursor !== null) query.set('cursor', changesCursor);
		const reply = await call('GET', `${api}/releases/${changesOf.trim()}/changes${query.size > 0 ? `?${query}` : ''}`);
		if (reply.status !== 200) {
			changesCursor = null;
			changesRead = describe(reply);
			return;
		}
		const shown = reply.body as { data: { kind: string; title: string; title_hidden: boolean }[]; next_cursor: string | null };
		changesCursor = shown.next_cursor;
		const rows = shown.data.map((change) => `${change.title_hidden ? '(title hidden)' : change.title} (${change.kind})`).join(', ');
		changesRead = `${rows || 'none'}; ${changesCursor === null ? 'last page' : 'more'}`;
	}

	async function firstChanges(event: SubmitEvent) {
		event.preventDefault();
		await readChanges(false);
	}

	async function firstPage(event: SubmitEvent) {
		event.preventDefault();
		await readPage(false);
	}

	onMount(loadProjects);
</script>

<h1>Releases</h1>
<p class="lede">
	Publishing through the real API (D-045): commit an approved changeset as the release after a base generation, see a stale base
	refused, and read <code>/content/v1</code> with no cookie, at the latest release or pinned to an earlier one. Rolling back to a
	release builds an ordinary changeset restoring what that release published — it is reviewed and published like any other, and
	the generation still moves forward (D-097). Undoing a takedown puts back exactly what it took down, in one request, while it is
	the newest release and by its author (D-184). Each release has a label and a receipt (D-051): what its destinations did and what
	its probes saw. The list pages newest first with <code>?limit=</code> and <code>?cursor=</code>.
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
			<li>
				generation {release.generation}: {release.title} ({release.entries} changed{release.unpublished > 0
					? `, ${release.unpublished} unpublished`
					: ''}){release.purge === 'none' ? '' : `, purge ${release.purge}`}, {release.label}{release.label === 'verified'
					? ` at ${release.locations}`
					: ''}
			</li>
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
		<label class="field">One article's id (empty for the list) <input bind:value={readEntry} name="read-entry" /></label>
		<button type="submit">Read published articles</button>
	</form>
	{#if delivered}<p data-testid="delivered">{delivered}</p>{/if}

	<form onsubmit={rollback}>
		<label class="field">Roll back to generation <input bind:value={restoreOf} name="restore-of" /></label>
		<label class="field">Restore changeset title <input bind:value={restoreTitle} name="restore-title" /></label>
		<button type="submit">Roll back to this release</button>
		<button type="button" onclick={preview}>Preview the rollback</button>
		<button type="button" onclick={rollbackAsPreviewed}>Roll back as previewed</button>
	</form>
	{#if previewed}<p data-testid="previewed">{previewed}</p>{/if}
	{#if restored}<p data-testid="restored">{restored}</p>{/if}

	<form onsubmit={firstChanges}>
		<label class="field">Changes of generation <input bind:value={changesOf} name="changes-of" /></label>
		<label class="field">Changes per page (empty for the default, 100) <input bind:value={changesSize} name="changes-size" /></label>
		<button type="submit">Read its changes</button>
	</form>
	{#if changesRead}
		<p data-testid="changes">{changesRead}</p>
		{#if changesCursor !== null}<button type="button" onclick={() => readChanges(true)}>Next changes</button>{/if}
	{/if}
	<form onsubmit={undo}>
		<label class="field">Undo the takedown in generation <input bind:value={undoOf} name="undo-of" /></label>
		<label class="field">Idempotency key (empty for a new one; kept for a resend) <input bind:value={undoKey} name="undo-key" /></label>
		<button type="submit">Undo this takedown</button>
	</form>
	{#if undone}<p data-testid="undone">{undone}</p>{/if}

	<form onsubmit={firstPage}>
		<label class="field">Releases per page (empty for the default, 100) <input bind:value={pageSize} name="page-size" /></label>
		<button type="submit">Read the first page</button>
	</form>
	{#if paged}
		<p data-testid="release-page">{paged}</p>
		{#if pageCursor !== null}<button type="button" onclick={() => readPage(true)}>Next page</button>{/if}
	{/if}

	<form onsubmit={readReceipt}>
		<label class="field">Receipt of generation <input bind:value={receiptOf} name="receipt-of" /></label>
		<button type="submit">Read receipt</button>
	</form>
	{#if receipt}<p data-testid="receipt">{receipt}</p>{/if}
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
		font-size: 0.875rem;
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
