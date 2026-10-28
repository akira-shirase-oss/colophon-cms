<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}
	interface Token {
		id: string;
		name: string;
		kind: string;
		state: string;
		permissions: { action: string; subject: string }[];
		secret?: string;
	}
	interface Event {
		id: string;
		action: string;
		subject: string;
		actor: { kind: string; name: string | null };
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let tokens = $state<Token[]>([]);
	let events = $state<Event[]>([]);
	let older = $state<string | null>(null);
	let name = $state('Testbed token');
	let kind = $state('read-only');
	let permissions = $state('[{"action": "content.read", "subject": "articles"}]');
	let days = $state('30');
	let secret = $state('');
	let tokenId = $state('');
	let used = $state('');
	let message = $state('');

	const base = $derived(`/api/v1/projects/${projectId}`);

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason})` : ''}`.trim();
	}

	function headers(): Record<string, string> {
		return { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken() };
	}

	async function loadProjects() {
		const reply = await call('GET', '/api/v1/projects');
		signedIn = reply.status === 200;
		projects = signedIn ? (reply.body as { data: Project[] }).data : [];
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
		if (projectId !== '') await load();
	}

	async function load() {
		const listed = await call('GET', `${base}/tokens`);
		tokens = listed.status === 200 ? (listed.body as { data: Token[] }).data : [];
		const read = await call('GET', `${base}/audit?limit=5`);
		const page = read.status === 200 ? (read.body as { data: Event[]; next_cursor: string | null }) : { data: [], next_cursor: null };
		events = page.data;
		older = page.next_cursor;
	}

	async function showOlder() {
		if (older === null) return;
		const read = await call('GET', `${base}/audit?limit=5&cursor=${encodeURIComponent(older)}`);
		const page = read.body as { data: Event[]; next_cursor: string | null };
		events = [...events, ...page.data];
		older = page.next_cursor;
	}

	async function create() {
		let body: Record<string, unknown> = { name, kind, expires_in_days: Number(days) };
		if (kind === 'custom') {
			try {
				body = { ...body, permissions: JSON.parse(permissions) as unknown };
			} catch {
				message = 'permissions are not JSON';
				return;
			}
		}
		const reply = await call('POST', `${base}/tokens`, { headers: headers(), body: JSON.stringify(body) });
		if (reply.status === 201) {
			const made = (reply.body as { data: Token }).data;
			secret = made.secret ?? '';
			tokenId = made.id;
			message = `created ${made.kind} token`;
		} else {
			message = describe(reply);
		}
		await load();
	}

	/** Reads the project's articles with the secret alone, as a script would. */
	async function useIt() {
		const reply = await call('GET', `${base}/environments/production/collections/articles/entries`, {
			headers: { Authorization: `Bearer ${secret}` }
		});
		used = reply.status === 200 ? '200 read articles' : describe(reply);
	}

	async function revoke() {
		const reply = await call('DELETE', `${base}/tokens/${tokenId}`, { headers: headers() });
		message = reply.status === 200 ? `revoked (${(reply.body as { data: Token }).data.state})` : describe(reply);
		await load();
	}

	onMount(loadProjects);
</script>

<h1>API tokens</h1>
<p class="lede">
	A project's API tokens and its audit log, through the real API (step R.4, D-063): make a read-only, full-access or custom token
	(its secret is shown once), use the secret alone to read the project's articles, revoke it, and read back every change to roles,
	members, invitations and tokens.
</p>

{#if signedIn === false}
	<p data-testid="signed-out">Not signed in. <a href={resolve('/session')}>Sign in on the Session page</a> first.</p>
{:else if signedIn}
	{#if message}<p data-testid="message">{message}</p>{/if}
	{#if projects.length > 0}
		<section>
			<label class="field">
				Project
				<select bind:value={projectId} onchange={load} name="project">
					{#each projects as project (project.id)}
						<option value={project.id}>{project.name} ({project.slug})</option>
					{/each}
				</select>
			</label>
			<ul data-testid="tokens">
				{#each tokens as token (token.id)}
					<li data-id={token.id} data-state={token.state}>
						{token.name}: {token.kind}, {token.state}{token.secret ? ' (secret listed!)' : ''}
					</li>
				{/each}
			</ul>
		</section>

		<section>
			<h2>A token</h2>
			<form onsubmit={(event) => event.preventDefault()}>
				<label class="field">Name <input bind:value={name} name="name" /></label>
				<label class="field">
					Kind
					<select bind:value={kind} name="kind">
						<option value="read-only">read-only</option>
						<option value="full-access">full-access</option>
						<option value="custom">custom</option>
					</select>
				</label>
				<label class="field">Permissions for custom (JSON) <textarea bind:value={permissions} name="permissions" rows="3"></textarea></label>
				<label class="field">Expires in days <input bind:value={days} name="days" inputmode="numeric" /></label>
				<p><button type="button" onclick={create}>Create</button></p>
			</form>
			{#if secret}
				<p>Secret, shown once: <code data-testid="secret">{secret}</code></p>
				<p>
					<button type="button" onclick={useIt}>Use it</button>
					<button type="button" onclick={revoke}>Revoke it</button>
				</p>
				{#if used}<p data-testid="used">{used}</p>{/if}
			{/if}
		</section>

		<section>
			<h2>Audit log</h2>
			<ol data-testid="audit">
				{#each events as event (event.id)}
					<li data-action={event.action}>{event.action} {event.subject} by a {event.actor.kind}</li>
				{/each}
			</ol>
			{#if older}<button type="button" onclick={showOlder}>Show older</button>{/if}
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
		display: grid;
		gap: 0.75rem;
		max-width: 46rem;
	}
	.field {
		display: grid;
		gap: 0.25rem;
		font-size: 0.85rem;
		color: var(--muted);
	}
	input,
	select,
	textarea {
		font: inherit;
		padding: 0.4rem 0.5rem;
	}
	textarea {
		font-family: var(--mono, monospace);
	}
	code {
		word-break: break-all;
	}
	ul,
	ol {
		display: grid;
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
</style>
