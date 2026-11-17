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
		derivatives: string[];
	}

	// The suite's real 1x1 PNG, as bytes.
	const PNG = new Uint8Array([
		137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0,
		0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 0, 0, 3, 1,
		1, 0, 201, 254, 146, 239, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130
	]);
	// A 400×4 PNG: wider than `w320`, so the worker writes one rendition.
	const WIDE = new Uint8Array([
		137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 1, 144, 0, 0, 0, 4, 8, 2,
		0, 0, 0, 47, 93, 254, 122, 0, 0, 0, 42, 73, 68, 65, 84, 120, 156, 237, 206, 65, 9, 0, 48, 12,
		4, 176, 74, 159, 243, 206, 194, 253, 142, 66, 32, 2, 50, 243, 22, 224, 134, 254, 0, 32, 212,
		31, 0, 132, 250, 3, 128, 204, 7, 24, 121, 90, 72, 220, 227, 41, 34, 0, 0, 0, 0, 73, 69, 78,
		68, 174, 66, 96, 130
	]);
	const FAKE = new TextEncoder().encode('<html><body>not an image</body></html>');

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let message = $state('');
	let removed = $state('');
	let steps = $state<string[]>([]);
	let assets = $state<Asset[]>([]);
	let librarySteps = $state<string[]>([]);
	let libraryRunning = $state(false);

	/** An upload's library record (D2): its words and the version a PATCH names. */
	interface LibraryAsset extends Asset {
		alt: string | null;
		title: string | null;
		credit: string | null;
		tags: string[];
		metadata_version: number;
	}

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
				// M.3: each recorded rendition answers under its own name.
				for (const name of record.derivatives ?? []) {
					const rendition = await fetch(`/server/media/v1/${projectId}/production/${id}/${name}`);
					steps = [...steps, `rendition ${name}: ${rendition.status} ${rendition.headers.get('content-type') ?? ''}`.trim()];
				}
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

	/** DELETE the asset: 202 is requested, not gone — poll until the `deleted` tombstone. A
	 *  refusal (still named somewhere, still quarantined, already gone) shows its reason. */
	async function remove(asset: Asset) {
		removed = '';
		const reply = await call('DELETE', `${env}/assets/${asset.id}`, {
			headers: { 'X-CSRF-Token': csrfToken(), 'Idempotency-Key': crypto.randomUUID() }
		});
		if (reply.status !== 202) {
			removed = `delete ${asset.file_name}: ${describe(reply)}`;
			return;
		}
		steps = [...steps, `delete ${asset.file_name}: ${reply.status} requested`];
		const deadline = Date.now() + 15_000;
		for (;;) {
			const check = await call('GET', `${env}/assets/${asset.id}`);
			const state = (check.body as { data?: Asset })?.data?.state;
			if (state === 'deleted') {
				steps = [...steps, 'deleted: the tombstone stays'];
				removed = `${asset.file_name}: deleted`;
				break;
			}
			if (Date.now() > deadline) {
				removed = `delete ${asset.file_name}: never reached deleted`;
				break;
			}
			await new Promise((wait) => setTimeout(wait, 200));
		}
		await list();
	}

	/** One page of the library (D2): keyset paging, `q`, `tag`. */
	async function page(query: Record<string, string>): Promise<{ reply: Reply; data: LibraryAsset[]; next: string | null }> {
		const reply = await call('GET', `${env}/assets?${new URLSearchParams(query)}`);
		const body = reply.body as { data?: LibraryAsset[]; next_cursor?: string | null } | null;
		return { reply, data: body?.data ?? [], next: body?.next_cursor ?? null };
	}

	/** PATCH an upload's library metadata under `If-Match` (D2). */
	function patch(id: string, version: number, metadata: Record<string, unknown>): Promise<Reply> {
		return call('PATCH', `${env}/assets/${id}`, {
			headers: {
				'Content-Type': 'application/json',
				'X-CSRF-Token': csrfToken(),
				'Idempotency-Key': crypto.randomUUID(),
				'If-Match': `"${version}"`
			},
			body: JSON.stringify(metadata)
		});
	}

	/**
	 * The library journey (D2), on a fresh project: three uploads read two at a time over the
	 * cursor; a search by name; metadata changed under If-Match, then refused when the version is
	 * stale; a bad tag refused at its place; the new words searchable and the tag a filter.
	 */
	async function library() {
		libraryRunning = true;
		librarySteps = [];
		const note = (step: string) => (librarySteps = [...librarySteps, step]);
		try {
			for (const [bytes, name] of [[PNG, 'first-dot.png'], [WIDE, 'wide-strip.png'], [PNG, 'last-dot.png']] as const) {
				const id = await settle(bytes, name);
				if (id === null) return note(`upload ${name}: never settled`);
			}
			note('uploaded: 3');
			const first = await page({ limit: '2' });
			note(`page 1: ${first.reply.status}, ${first.data.map((asset) => asset.file_name).join(', ')}${first.next === null ? ', end' : ', more'}`);
			if (first.next === null) return;
			const second = await page({ limit: '2', cursor: first.next });
			note(`page 2: ${second.reply.status}, ${second.data.map((asset) => asset.file_name).join(', ')}${second.next === null ? ', end' : ', more'}`);
			const found = await page({ q: 'WIDE' });
			note(`search "WIDE": ${found.data.map((asset) => asset.file_name).join(', ') || 'nothing'}`);

			const target = first.data.find((asset) => asset.file_name === 'last-dot.png');
			if (target === undefined) return note('last-dot.png missing from page 1');
			const changed = await patch(target.id, target.metadata_version, { alt: 'A single dot of ink', title: 'Dot', credit: 'The testbed', tags: ['testbed', 'dots'] });
			const updated = (changed.body as { data?: LibraryAsset } | null)?.data;
			note(`metadata: ${describe(changed)}${updated ? `, version ${updated.metadata_version > target.metadata_version ? 'moved on' : 'did not move'}, tags ${updated.tags.join(' ')}` : ''}`);
			const stale = await patch(target.id, target.metadata_version, { title: 'Lost update' });
			note(`stale metadata: ${describe(stale)}`);
			const bad = await patch(target.id, updated?.metadata_version ?? target.metadata_version, { tags: ['Not A Slug'] });
			note(`bad tag: ${describe(bad)}`);
			const byAlt = await page({ q: 'dot of ink' });
			note(`search "dot of ink": ${byAlt.data.map((asset) => asset.file_name).join(', ') || 'nothing'}`);
			const byTag = await page({ tag: 'dots' });
			note(`tag "dots": ${byTag.data.map((asset) => asset.file_name).join(', ') || 'nothing'}`);
			const read = await call('GET', `${env}/assets/${target.id}`);
			const record = (read.body as { data?: LibraryAsset } | null)?.data;
			note(`read back: ${record ? `${record.title} / ${record.alt} / ${record.credit}` : describe(read)}`);
		} finally {
			libraryRunning = false;
			await list();
		}
	}

	/** Upload `bytes` and wait for the worker's verdict; the asset's id, or null. */
	async function settle(bytes: Uint8Array, name: string): Promise<string | null> {
		const reply = await call('POST', `${env}/assets`, {
			headers: {
				'Content-Type': 'image/png',
				'X-CSRF-Token': csrfToken(),
				'Idempotency-Key': crypto.randomUUID(),
				'X-Colophon-File-Name': name
			},
			body: bytes as unknown as BodyInit
		});
		if (reply.status !== 202) return null;
		const id = (reply.body as { data: { id: string } }).data.id;
		const deadline = Date.now() + 15_000;
		while (Date.now() < deadline) {
			const asset = await call('GET', `${env}/assets/${id}`);
			if ((asset.body as { data?: Asset })?.data?.state !== 'quarantined') return id;
			await new Promise((wait) => setTimeout(wait, 200));
		}
		return null;
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
<p>Uploads land in quarantine; the worker's signature check and bounded decode move each to <code>usable</code> or <code>rejected</code>.
A delete is staged: 202 means the job is queued, and the row stays as a <code>deleted</code> tombstone (D-098).</p>

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
	<button data-testid="upload-wide" onclick={() => upload(WIDE, 'wide.png')} disabled={!projectId}>Upload a wide PNG</button>
	<button data-testid="upload-fake" onclick={() => upload(FAKE, 'photo.png')} disabled={!projectId}>Upload a fake PNG</button>
	<p data-testid="message">{message}</p>
	{#if removed}<p data-testid="removed">{removed}</p>{/if}
	<ol data-testid="steps">
		{#each steps as step, at (at)}
			<li>{step}</li>
		{/each}
	</ol>
	<h2>The library</h2>
	<p>Paging over a cursor, search by name or alt text, and metadata (alt, title, credit, tags) changed under <code>If-Match</code>, so a stale edit is refused instead of lost.</p>
	<button data-testid="library-run" onclick={library} disabled={!projectId || libraryRunning}>{libraryRunning ? 'Running…' : 'Run the library journey'}</button>
	<ol data-testid="library-steps">
		{#each librarySteps as step, at (at)}
			<li>{step}</li>
		{/each}
	</ol>
	<table data-testid="assets">
		<thead>
			<tr><th>Name</th><th>State</th><th>Detected</th><th>Size</th><th>Reason</th><th></th></tr>
		</thead>
		<tbody>
			{#each assets as asset (asset.id)}
				<tr data-asset={asset.id}>
					<td>{asset.file_name}</td>
					<td>{asset.state}</td>
					<td>{asset.detected_type ?? '—'}</td>
					<td>{asset.width ?? '—'}×{asset.height ?? '—'}</td>
					<td>{asset.rejected_reason ?? '—'}</td>
					<td>
						{#if asset.state === 'usable' || asset.state === 'rejected'}
							<button data-testid="asset-delete" onclick={() => remove(asset)}>Delete</button>
						{/if}
					</td>
				</tr>
			{/each}
		</tbody>
	</table>
{/if}
