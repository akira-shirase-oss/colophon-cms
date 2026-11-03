<script lang="ts">
	import { onMount } from 'svelte';
	import { call, csrfToken, detailsOf, envelopeOf, listProjects, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}

	interface Asset {
		id: string;
		state: string;
		file_name: string;
		detected_type: string | null;
		width: number | null;
		height: number | null;
		rejected_reason: string | null;
	}

	// The suite's real 1x1 PNG, as bytes.
	const PNG = new Uint8Array([
		137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0,
		0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 0, 0, 3, 1,
		1, 0, 201, 254, 146, 239, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130
	]);
	const FAKE = new TextEncoder().encode('<html><body>not an image</body></html>');

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let message = $state('');
	let steps = $state<string[]>([]);
	let assets = $state<Asset[]>([]);

	const env = $derived(`/api/v1/projects/${projectId}/environments/production`);

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason}${details.pointer ? ` at ${details.pointer}` : ''})` : ''}`.trim();
	}

	async function loadProjects() {
		const reply = await listProjects<Project>();
		signedIn = reply.status === 200;
		projects = reply.projects;
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
	}

	/** Upload `bytes` as `name`, then poll the asset until the worker settles it. */
	async function upload(bytes: Uint8Array, name: string) {
		steps = [];
		message = '';
		const reply = await call('POST', `${env}/assets`, {
			headers: {
				'Content-Type': 'image/png',
				'X-CSRF-Token': csrfToken(),
				'Idempotency-Key': crypto.randomUUID(),
				'X-Colophon-File-Name': name
			},
			body: bytes as unknown as BodyInit
		});
		if (reply.status !== 202) {
			message = `upload: ${describe(reply)}`;
			return;
		}
		const id = ((reply.body as { data: { id: string } }).data ?? { id: '' }).id;
		steps = [...steps, `uploaded: ${reply.status} quarantined`];
		const deadline = Date.now() + 15_000;
		for (;;) {
			const asset = await call('GET', `${env}/assets/${id}`);
			const state = (asset.body as { data?: Asset })?.data?.state;
			if (state !== 'quarantined') {
				const record = (asset.body as { data: Asset }).data;
				steps = [
					...steps,
					`${record.state}: ${record.detected_type ?? record.rejected_reason ?? ''}`.trim()
				];
				// M.1: the public media origin serves usable objects only.
				const origin = await fetch(`/server/media/v1/${projectId}/production/${id}`);
				steps = [
					...steps,
					record.state === 'usable'
						? `served: ${origin.status} ${origin.headers.get('content-type') ?? ''}`.trim()
						: `served: ${origin.status}`
				];
				break;
			}
			if (Date.now() > deadline) {
				message = `still quarantined after 15s`;
				return;
			}
			await new Promise((wait) => setTimeout(wait, 200));
		}
		await list();
		message = 'settled';
	}

	async function list() {
		const reply = await call('GET', `${env}/assets`);
		if (reply.status !== 200) {
			message = `list: ${describe(reply)}`;
			return;
		}
		assets = (reply.body as { data: Asset[] }).data ?? [];
	}

	onMount(loadProjects);
</script>

<h1>Assets</h1>
<p>Uploads land in quarantine; the worker's signature check and bounded decode move each to <code>usable</code> or <code>rejected</code>.</p>

{#if signedIn === false}
	<p data-testid="signed-out">Sign in first (the Session page).</p>
{:else}
	<label>
		Project
		<select bind:value={projectId} data-testid="project">
			{#each projects as project (project.id)}
				<option value={project.id}>{project.name}</option>
			{/each}
		</select>
	</label>
	<button data-testid="upload-real" onclick={() => upload(PNG, 'dot.png')} disabled={!projectId}>Upload a real PNG</button>
	<button data-testid="upload-fake" onclick={() => upload(FAKE, 'photo.png')} disabled={!projectId}>Upload a fake PNG</button>
	<p data-testid="message">{message}</p>
	<ol data-testid="steps">
		{#each steps as step, at (at)}
			<li>{step}</li>
		{/each}
	</ol>
	<table data-testid="assets">
		<thead>
			<tr><th>Name</th><th>State</th><th>Detected</th><th>Size</th><th>Reason</th></tr>
		</thead>
		<tbody>
			{#each assets as asset (asset.id)}
				<tr>
					<td>{asset.file_name}</td>
					<td>{asset.state}</td>
					<td>{asset.detected_type ?? '—'}</td>
					<td>{asset.width ?? '—'}×{asset.height ?? '—'}</td>
					<td>{asset.rejected_reason ?? '—'}</td>
				</tr>
			{/each}
		</tbody>
	</table>
{/if}
