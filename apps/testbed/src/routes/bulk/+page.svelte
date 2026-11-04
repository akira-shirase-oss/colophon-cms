<script lang="ts">
	// Bulk jobs through the real HTTP API (P5, D-116): a manifest of exact entries and base
	// revisions submitted in bounded chunks, driven per item by the worker. This page submits,
	// appends, lists, opens items and cancels — against `{P}/bulk`.
	import { onMount } from 'svelte';
	import { call, csrfToken, detailsOf, envelopeOf, listProjects, type Reply } from '#lib/api.ts';

	interface Job {
		id: string;
		state: string;
		hash: string;
		items: { expected: number; done: number; blocked: number };
	}
	interface Item {
		key: string;
		entry_id: string;
		outcome: string;
		problem: { reason?: string; message?: string } | null;
	}
	interface Project {
		id: string;
		slug: string;
	}

	const key = () => `tb-${crypto.randomUUID()}`;
	const headers = (extra?: Record<string, string>) => ({
		'X-CSRF-Token': csrfToken(),
		'Content-Type': 'application/json',
		'Idempotency-Key': key(),
		...extra
	});
	const describe = (reply: Reply): string => {
		const envelope = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `HTTP ${reply.status} ${envelope?.code ?? ''}${details ? ` ${details.reason} at ${details.pointer}` : ''}`.trim();
	};

	let signedIn = $state<boolean | null>(null);
	let projects = $state<Project[]>([]);
	let projectId = $state('');
	const base = $derived(`/api/v1/projects/${projectId}/environments/production/bulk`);
	let jobs = $state<Job[]>([]);
	let job = $state<Job | null>(null);
	let items = $state<Item[]>([]);
	let outcome = $state('');
	let collection = $state('articles');
	let manifest = $state('[\n  {"key": "a", "entry": "<entry uuid>", "expected_revision": "<revision uuid>",\n   "op": {"op": "set", "path": "title", "value": "bulk"}}\n]');
	let seal = $state(true);
	let message = $state('');

	async function loadJobs() {
		const reply = await call('GET', `${base}?limit=50`);
		jobs = reply.status === 200 ? (reply.body as { data: Job[] }).data : [];
	}

	async function openJob(id: string) {
		const reply = await call('GET', `${base}/${id}`);
		if (reply.status !== 200) {
			message = describe(reply);
			return;
		}
		job = (reply.body as { data: Job }).data;
		await loadItems();
	}

	async function loadItems() {
		if (job === null) return;
		const filter = outcome === '' ? '' : `?outcome=${outcome}`;
		const reply = await call('GET', `${base}/${job.id}/items${filter}${filter === '' ? '?' : '&'}limit=100`);
		items = reply.status === 200 ? (reply.body as { data: Item[] }).data : [];
	}

	async function submit(append: boolean) {
		let parsed: unknown;
		try {
			parsed = JSON.parse(manifest);
		} catch {
			message = 'the manifest is not JSON';
			return;
		}
		const target = append && job !== null ? `${base}/${job.id}/items` : base;
		const bound: Record<string, string> = append && job !== null ? { 'If-Match': `"${job.hash}"` } : {};
		const reply = await call('POST', target, {
			headers: headers(bound),
			body: JSON.stringify(append ? { items: parsed, seal } : { collection, items: parsed, seal })
		});
		message = reply.status === 202 ? (append ? `appended; job ${job?.id}` : `submitted ${(reply.body as { data: Job }).data.id}`) : describe(reply);
		await loadJobs();
		if (job !== null) await openJob(job.id);
	}

	async function cancel() {
		if (job === null) return;
		const reply = await call('POST', `${base}/${job.id}/cancel`, { headers: headers() });
		message = reply.status === 200 ? `cancelled` : describe(reply);
		await loadJobs();
		await openJob(job.id);
	}

	onMount(async () => {
		const reply = await listProjects<Project>();
		signedIn = reply.status === 200;
		projects = reply.projects;
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
		if (projectId !== '') await loadJobs();
	});
</script>

{#if signedIn === false}
	<p>Sign in on the Session page first.</p>
{:else if signedIn}
	<h1>Bulk jobs</h1>
	<p>Bounded manifests of exact entries and base revisions, executed per item — never globally atomic.</p>
	<label>Project
		<select bind:value={projectId} onchange={loadJobs} name="project">
			{#each projects as project (project.id)}<option value={project.id}>{project.slug}</option>{/each}
		</select>
	</label>
	<form onsubmit={(event) => { event.preventDefault(); void submit(false); }}>
		<label>Collection <input bind:value={collection} name="collection" /></label>
		<label>Manifest (JSON array of items)
			<textarea bind:value={manifest} name="manifest" rows="8"></textarea>
		</label>
		<label><input type="checkbox" bind:checked={seal} name="seal" /> Seal (start the run)</label>
		<button type="submit">Submit job</button>
		<button type="button" disabled={job === null} onclick={() => void submit(true)}>Append to open job</button>
	</form>
	{#if message !== ''}<p role="status" data-testid="message">{message}</p>{/if}
	<button type="button" onclick={loadJobs}>Refresh</button>
	<ul data-testid="jobs">
		{#each jobs as listed (listed.id)}
			<li><button type="button" onclick={() => void openJob(listed.id)}>{listed.id.slice(0, 8)}</button>
				{listed.state} — {listed.items.done}/{listed.items.expected} done, {listed.items.blocked} blocked</li>
		{/each}
	</ul>
	{#if job !== null}
		<h2>Job {job.id.slice(0, 8)} — {job.state}</h2>
		<label>Outcome <select bind:value={outcome} onchange={loadItems} name="outcome">
			<option value="">all</option>{#each ['pending', 'applied', 'conflict', 'refused', 'skipped'] as value (value)}<option {value}>{value}</option>{/each}
		</select></label>
		<button type="button" disabled={job.state === 'done' || job.state === 'blocked' || job.state === 'cancelled'} onclick={cancel}>Cancel</button>
		<ul data-testid="items">
			{#each items as item (item.key)}
				<li>{item.key}: {item.outcome}{item.problem?.reason ? ` (${item.problem.reason})` : ''}</li>
			{/each}
		</ul>
	{/if}
{/if}
