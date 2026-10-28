<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}
	interface Role {
		key: string;
		name: string;
		version: number;
		fixed: boolean;
		members: number;
		permissions: { action: string; subject: string }[];
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let roles = $state<Role[]>([]);
	let key = $state('');
	let name = $state('');
	let permissions = $state('[{"action": "content.read", "subject": "articles"}]');
	let version = $state('1');
	let message = $state('');

	const base = $derived(`/api/v1/projects/${projectId}/roles`);

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason})` : ''}`.trim();
	}

	function headers(extra: Record<string, string> = {}): Record<string, string> {
		return { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), ...extra };
	}

	/** The permissions box as JSON, or `null` (and a message) if it isn't. */
	function parsed(): unknown {
		try {
			return JSON.parse(permissions) as unknown;
		} catch {
			message = 'permissions are not JSON';
			return null;
		}
	}

	async function loadProjects() {
		const reply = await call('GET', '/api/v1/projects');
		signedIn = reply.status === 200;
		projects = signedIn ? (reply.body as { data: Project[] }).data : [];
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
		if (projectId !== '') await load();
	}

	async function load() {
		const listed = await call('GET', base);
		roles = listed.status === 200 ? (listed.body as { data: Role[] }).data : [];
	}

	async function create() {
		const body = parsed();
		if (body === null) return;
		const reply = await call('POST', base, { headers: headers(), body: JSON.stringify({ key, name, permissions: body }) });
		message = reply.status === 201 ? `created ${key} v${(reply.body as { data: Role }).data.version}` : describe(reply);
		await load();
	}

	async function save() {
		const body = parsed();
		if (body === null) return;
		const reply = await call('PUT', `${base}/${key}`, {
			headers: headers({ 'If-Match': `"v${version}"` }),
			body: JSON.stringify({ name, permissions: body })
		});
		message = reply.status === 200 ? `saved ${key} v${(reply.body as { data: Role }).data.version}` : describe(reply);
		await load();
	}

	async function remove() {
		const reply = await call('DELETE', `${base}/${key}`, { headers: headers({ 'If-Match': `"v${version}"` }) });
		message = reply.status === 204 ? `deleted ${key}` : describe(reply);
		await load();
	}

	onMount(loadProjects);
</script>

<h1>Roles</h1>
<p class="lede">
	Roles owners define, through the real API (D-058): create one with a list of permissions, change it naming the version you edited
	(If-Match), and delete it once nobody holds it. The owner role is fixed, field lists and conditions are refused until they're
	enforced, and nobody grants more than their own role holds.
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
			<ul data-testid="roles">
				{#each roles as role (role.key)}
					<li data-key={role.key}>
						<code>{role.key}</code> v{role.version}: {role.permissions.map((grant) => `${grant.action} on ${grant.subject}`).join(', ') || 'nothing'}
						({role.members} member{role.members === 1 ? '' : 's'}){role.fixed ? ', fixed' : ''}
					</li>
				{/each}
			</ul>
		</section>

		<section>
			<h2>A role</h2>
			<form onsubmit={(event) => event.preventDefault()}>
				<label class="field">Key <input bind:value={key} name="key" /></label>
				<label class="field">Name <input bind:value={name} name="name" /></label>
				<label class="field">Permissions (JSON) <textarea bind:value={permissions} name="permissions" rows="4"></textarea></label>
				<label class="field">Version (If-Match) <input bind:value={version} name="version" inputmode="numeric" /></label>
				<p>
					<button type="button" onclick={create}>Create</button>
					<button type="button" onclick={save}>Save</button>
					<button type="button" onclick={remove}>Delete</button>
				</p>
			</form>
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
	ul {
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
