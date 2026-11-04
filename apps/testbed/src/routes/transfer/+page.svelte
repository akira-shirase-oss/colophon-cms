<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, listProjects, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}

	interface Outcome {
		item: string;
		status: string;
		entry?: string;
		source?: string;
		unmapped?: string[];
		refusal?: { code: string; pointer: string; message: string };
		reason?: string;
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let exported = $state('');
	let imported = $state('');
	let strapiResult = $state('');
	let refused = $state('');
	let message = $state('');
	let outcomes = $state<Outcome[]>([]);
	let outcomeLabel = $state('');
	let entryId = $state('');

	const env = $derived(`/api/v1/projects/${projectId}/environments/production`);
	const collection = $derived(`${env}/collections/notes`);

	const NOTES = {
		name: 'Notes',
		title_field: 'title',
		fields: [
			{ id: 'title', label: 'Title', type: 'text', required: true },
			{ id: 'note', label: 'Note', type: 'text' }
		]
	};

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason}${details.pointer ? ` at ${details.pointer}` : ''})` : ''}`.trim();
	}

	/** The per-item report an import or a dry run answers (D-114). */
	function outcomeRows(reply: Reply): Outcome[] {
		const data = (reply.body as { data?: { outcomes?: Outcome[] } })?.data;
		return data?.outcomes ?? [];
	}

	/** One outcome row as a line: `item — status`, then what the source or refusal said. */
	function show(outcome: Outcome): string {
		const rest = [
			outcome.source ? `source ${outcome.source}` : '',
			outcome.unmapped && outcome.unmapped.length > 0 ? `unmapped ${outcome.unmapped.join(', ')}` : '',
			outcome.refusal ? `${outcome.refusal.code} at ${outcome.refusal.pointer}` : '',
			outcome.reason ?? ''
		]
			.filter((part) => part !== '')
			.join('; ');
		return `${outcome.item} — ${outcome.status}${rest === '' ? '' : ` (${rest})`}`;
	}

	function headers(extra: Record<string, string> = {}): Record<string, string> {
		return { 'X-CSRF-Token': csrfToken(), 'Idempotency-Key': crypto.randomUUID(), ...extra };
	}

	async function loadProjects() {
		const reply = await listProjects<Project>();
		signedIn = reply.status === 200;
		projects = reply.projects;
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
	}

	/** The notes collection: the type, then two entries written through the import. */
	async function setUp() {
		const made = await call('POST', `${env}/content-types`, {
			headers: headers({ 'Content-Type': 'application/json' }),
			body: JSON.stringify({ slug: 'notes', definition: NOTES })
		});
		if (made.status !== 201) {
			message = `type: ${describe(made)}`;
			return;
		}
		const file = '{"title":"First","note":"one"}\n{"title":"Second","note":"two"}\n';
		const up = await call('POST', `${collection}/import`, { headers: headers({ 'Content-Type': 'application/jsonlines' }), body: file });
		if (up.status !== 201) {
			message = `import: ${describe(up)}`;
			return;
		}
		imported = `imported ${(up.body as { data: { imported: number } }).data.imported}`;
		entryId = outcomeRows(up)[0]?.entry ?? '';
		await download();
		message = 'set up';
	}

	/** GET the export: the raw JSONL text lands in the page. */
	async function download() {
		const reply = await call('GET', `${collection}/export`);
		if (reply.status !== 200) {
			message = `export: ${describe(reply)}`;
			return;
		}
		exported = reply.text;
	}

	/** One Strapi export entity line: `{"type":"api::…","data":{…}}` unwraps to its data. */
	async function importStrapi() {
		const line = '{"type":"api::note.note","data":{"title":"From Strapi","note":"unwrapped"}}\n';
		const reply = await call('POST', `${collection}/import`, { headers: headers({ 'Content-Type': 'application/jsonlines' }), body: line });
		strapiResult = describe(reply);
		if (reply.status !== 201) return;
		await download();
	}

	/** A file whose second line fails lands nothing, refused where it failed. */
	async function importBad() {
		const file = '{"title":"Fine"}\n{"note":"no title"}\n';
		const reply = await call('POST', `${collection}/import`, { headers: headers({ 'Content-Type': 'application/jsonlines' }), body: file });
		refused = describe(reply);
		await download();
	}

	/** `?dry_run=true`: the same pipeline rolled back — the per-item plan, nothing written. */
	async function dryRun() {
		const file = '{"title":"Planned","note":"ok"}\n{"note":"no title"}\n{"_id":"p1","_type":"posts","title":"Skipped","note":"not ours"}\n';
		const reply = await call('POST', `${collection}/import?dry_run=true&type=notes`, {
			headers: headers({ 'Content-Type': 'application/jsonlines' }),
			body: file
		});
		if (reply.status !== 200) {
			outcomeLabel = `dry run: ${describe(reply)}`;
			outcomes = [];
			return;
		}
		outcomeLabel = 'dry run';
		outcomes = outcomeRows(reply);
	}

	/** A Sanity NDJSON line: `_id`/`_type` map, other `_`-keys are reported. */
	async function importSanity() {
		const line = '{"_id":"drafts.s7","_type":"notes","_rev":"r1","_createdAt":"2024-05-05","title":"From Sanity","note":"ndjson"}\n';
		const reply = await call('POST', `${collection}/import`, { headers: headers({ 'Content-Type': 'application/jsonlines' }), body: line });
		outcomeLabel = `sanity: ${describe(reply)}`;
		outcomes = outcomeRows(reply);
		if (reply.status !== 201) return;
		await download();
	}

	/** A `documentId` naming an existing entry is a save onto its draft, not a duplicate. */
	async function importUpdate() {
		const line = `{"documentId":"${entryId}","title":"Renamed","note":"by source id","status":"published"}\n`;
		const reply = await call('POST', `${collection}/import`, { headers: headers({ 'Content-Type': 'application/jsonlines' }), body: line });
		outcomeLabel = `update: ${describe(reply)}`;
		outcomes = outcomeRows(reply);
		if (reply.status !== 201) return;
		await download();
	}

	onMount(loadProjects);
</script>

<h1>Import &amp; export</h1>
<p class="lede">
	A collection's drafts export as JSONL — one entry's canonical content per line — and JSONL, a JSON array, or Strapi and
	Sanity documents import back in one transaction (D-086, D-114), through the real API. `?dry_run=true` plans instead:
	the same pipeline rolled back, with a per-item report.
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
				<button type="button" onclick={setUp}>Set up notes and import two</button>
				<button type="button" onclick={dryRun} disabled={exported === ''}>Dry-run a mixed file</button>
				<button type="button" onclick={importStrapi} disabled={exported === ''}>Import a Strapi entity</button>
				<button type="button" onclick={importSanity} disabled={exported === ''}>Import a Sanity document</button>
				<button type="button" onclick={importUpdate} disabled={entryId === ''}>Update by source id</button>
				<button type="button" onclick={importBad} disabled={exported === ''}>Import a bad file</button>
				<button type="button" onclick={download} disabled={exported === ''}>Export again</button>
			</p>
			{#if imported}<p>Setup: <span data-testid="imported">{imported}</span></p>{/if}
			{#if strapiResult}<p>Strapi entity: <span data-testid="strapi-result">{strapiResult}</span></p>{/if}
			{#if refused}<p>Bad file: <span data-testid="refused">{refused}</span></p>{/if}
			{#if outcomeLabel}
				<p><span data-testid="outcome-label">{outcomeLabel}</span></p>
				{#if outcomes.length > 0}
					<ul data-testid="outcomes">
						{#each outcomes as outcome (outcome.item)}
							<li data-testid="outcome" data-status={outcome.status}>{show(outcome)}</li>
						{/each}
					</ul>
				{/if}
			{/if}
			{#if exported}
				<p>Export (<span data-testid="export-count">{exported.split('\n').filter((line) => line !== '').length}</span> lines):</p>
				<pre data-testid="export-body">{exported}</pre>
			{/if}
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
	pre {
		font-size: 0.8rem;
		border: 1px solid var(--line);
		border-radius: 8px;
		padding: 0.75rem;
		overflow-x: auto;
		max-width: 46rem;
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
</style>
