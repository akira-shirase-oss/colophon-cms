<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, listProjects, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}
	interface Summary {
		id: string;
		revision: { title: string };
	}
	interface StageDef {
		key: string;
		name: string;
		different_reviewer: boolean;
	}
	interface StageState extends StageDef {
		state: string;
		by: string | null;
		by_name: string | null;
		at: string | null;
	}
	interface Changeset {
		id: string;
		title: string;
		hash: string;
		status: { state: string; by: string | null; self_review: boolean };
		stages: StageState[];
		reviews: { id: string; decision: string; stage: string | null; reviewer_name: string | null; current: boolean }[];
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let entries = $state<Summary[]>([]);
	let message = $state('');

	// The environment's ordered stage list (D-104): the ETag below names the read, and Replace
	// sends it in If-Match so a list that changed under this page is refused stale_stages.
	let defined = $state<StageDef[]>([]);
	let etag = $state('');
	let draft = $state('');

	// The changeset under test: its hash edits like on the Changesets page, the review names the
	// stage it decides, and publish names the release generation it follows.
	let open = $state<Changeset | null>(null);
	let picked = $state<string[]>([]);
	let title = $state('');
	let hash = $state('');
	let stage = $state('');
	let comment = $state('');
	let base = $state('0');

	const api = $derived(`/api/v1/projects/${projectId}/environments/production`);

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason})` : ''}`.trim();
	}

	function shown(reply: Reply) {
		if (reply.body !== null && typeof reply.body === 'object' && 'data' in reply.body) open = (reply.body as { data: Changeset }).data;
		hash = open?.hash ?? hash;
		stage = open?.stages.find((item) => item.state !== 'approved')?.key ?? open?.stages[0]?.key ?? '';
	}

	async function loadProjects() {
		const reply = await listProjects<Project>();
		signedIn = reply.status === 200;
		projects = reply.projects;
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
		if (projectId !== '') await load();
	}

	// Each load() takes a ticket; a read that resolves after a newer load — or after a write —
	// discards its answers, so a stale GET can never wipe a list this page already replaced.
	let loading = 0;

	async function load() {
		const ticket = ++loading;
		const stages = await call('GET', `${api}/review-stages`);
		if (ticket !== loading) return;
		if (stages.status === 200) {
			defined = (stages.body as { data: { stages: StageDef[] } }).data.stages;
			etag = stages.headers.get('etag') ?? '';
			draft = JSON.stringify(defined, null, 2);
		} else {
			message = describe(stages);
		}
		const [list, releases] = await Promise.all([call('GET', `${api}/collections/articles/entries?limit=100`), call('GET', `${api}/releases`)]);
		if (ticket !== loading) return;
		entries = list.status === 200 ? (list.body as { data: Summary[] }).data : [];
		base = releases.headers.get('colophon-generation') ?? '0';
		picked = [];
		open = null;
	}

	async function apply() {
		loading += 1;
		const reply = await call('PUT', `${api}/review-stages`, {
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), 'If-Match': etag, 'Idempotency-Key': crypto.randomUUID() },
			body: JSON.stringify({ stages: JSON.parse(draft) })
		});
		if (reply.status === 200) {
			defined = (reply.body as { data: { stages: StageDef[] } }).data.stages;
			etag = reply.headers.get('etag') ?? etag;
			message = 'replaced';
		} else {
			message = describe(reply);
		}
	}

	async function create(event: SubmitEvent) {
		event.preventDefault();
		loading += 1;
		const reply = await call('POST', `${api}/changesets`, {
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), 'Idempotency-Key': crypto.randomUUID() },
			body: JSON.stringify({ title, entries: picked })
		});
		if (reply.status === 201) {
			shown(reply);
			message = 'created';
		} else {
			message = describe(reply);
		}
	}

	async function reload() {
		if (open === null) return;
		loading += 1;
		const reply = await call('GET', `${api}/changesets/${open.id}`);
		if (reply.status === 200) {
			shown(reply);
			message = 'loaded';
		} else {
			message = describe(reply);
		}
	}

	async function act(action: 'submit' | 'reviews', decision?: string) {
		if (open === null) return;
		loading += 1;
		const body = decision === undefined ? undefined : { decision, comment, ...(stage === '' ? {} : { stage }) };
		const reply = await call('POST', `${api}/changesets/${open.id}/${action}`, {
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), 'If-Match': `"${hash}"` },
			...(body === undefined ? {} : { body: JSON.stringify(body) })
		});
		if (reply.status === 200) {
			shown(reply);
			message = decision ?? action;
		} else {
			message = describe(reply);
		}
	}

	async function publish() {
		if (open === null) return;
		loading += 1;
		const reply = await call('POST', `${api}/changesets/${open.id}/publish`, {
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), 'If-Match': `"${hash}"`, 'Idempotency-Key': crypto.randomUUID() },
			body: JSON.stringify({ base_generation: Number(base) })
		});
		message = reply.status === 201 ? `published (generation ${(reply.body as { data: { generation: number } }).data.generation})` : describe(reply);
		if (reply.status === 201) await reload();
	}

	onMount(loadProjects);
</script>

<h1>Review stages</h1>
<p class="lede">
	Named review stages through the real API (D-104): the environment's ordered list reads with an ETag and replaces whole
	under <code>schema.change</code> with <code>If-Match</code> and an idempotency key. With a list set, every review names
	the stage it decides and a changeset publishes only once every stage approves its current hash; with none, one
	approval publishes — the way it always worked. The flow below drives all of it.
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

	<section data-testid="stage-defs">
		<h2>Stage list</h2>
		<p>ETag <code data-testid="etag">{etag}</code></p>
		<ol data-testid="stages">
			{#each defined as item (item.key)}
				<li data-testid="stage-def"><code>{item.key}</code> — {item.name}{item.different_reviewer ? ' (someone other than the submitter)' : ''}</li>
			{:else}
				<li class="muted">No stages: one approval publishes.</li>
			{/each}
		</ol>
		<label class="field">Stage list (JSON) <textarea bind:value={draft} name="stages-json" rows="5" data-testid="stages-json"></textarea></label>
		<p class="row">
			<button onclick={load}>Reload</button>
			<button onclick={apply} data-testid="apply-stages">Replace stage list</button>
		</p>
	</section>

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
		<button type="submit">Create changeset</button>
	</form>

	{#if message}<p data-testid="message">{message}</p>{/if}

	{#if open}
		<section data-testid="changeset" data-state={open.status.state}>
			<h2>{open.title}</h2>
			<p>state <code data-testid="state">{open.status.state}</code></p>
			<ol data-testid="stage-states">
				{#each open.stages as item (item.key)}
					<li data-testid="stage-state" data-state={item.state}>{item.name}: {item.state}{item.by !== null ? ` by ${item.by_name ?? item.by}` : ''}</li>
				{/each}
			</ol>
			<label class="field">If-Match hash <input bind:value={hash} name="hash" /></label>
			<label class="field">
				Stage
				<select bind:value={stage} name="stage" data-testid="review-stage">
					<option value="">— (no stage named)</option>
					{#each open.stages as item (item.key)}
						<option value={item.key}>{item.key} ({item.state})</option>
					{/each}
				</select>
			</label>
			<label class="field">Comment <input bind:value={comment} name="comment" /></label>
			<label class="field">Base generation <input bind:value={base} name="base" /></label>
			<p class="row">
				<button onclick={reload}>Reload</button>
				<button onclick={() => act('submit')}>Submit</button>
				<button onclick={() => act('reviews', 'approve')} data-testid="approve">Approve</button>
				<button onclick={() => act('reviews', 'request_changes')}>Request changes</button>
				<button onclick={publish} data-testid="publish">Publish</button>
			</p>
			<ol data-testid="reviews">
				{#each open.reviews as review (review.id)}
					<li>{review.decision}{review.stage !== null ? ` for ${review.stage}` : ''} by {review.reviewer_name}{review.current ? '' : ' (earlier hash)'}</li>
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
		font-size: 0.875rem;
		color: var(--muted);
	}
	fieldset {
		display: grid;
		gap: 0.25rem;
	}
	input,
	select,
	textarea {
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
