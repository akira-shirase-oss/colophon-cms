<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}
	interface Member {
		identity_id: string;
		display_name: string | null;
		role: string;
	}
	interface Invitation {
		id: string;
		role: string;
		label: string;
		expires_at: string;
		link?: string;
	}

	const ROLES = ['owner', 'developer', 'editor', 'publisher'];

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let members = $state<Member[]>([]);
	let pending = $state<Invitation[]>([]);
	let label = $state('');
	let role = $state('editor');
	let link = $state('');
	let token = $state('');
	let offer = $state('');
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
		const listed = await call('GET', `${base}/members`);
		members = listed.status === 200 ? (listed.body as { data: Member[] }).data : [];
		// Only owners may list invitations; anyone else gets 403 and an empty list here.
		const invited = await call('GET', `${base}/invitations`);
		pending = invited.status === 200 ? (invited.body as { data: Invitation[] }).data : [];
	}

	async function setRole(identity: string, next: string) {
		const reply = await call('PUT', `${base}/members/${identity}`, { headers: headers(), body: JSON.stringify({ role: next }) });
		message = reply.status === 204 ? `role changed to ${next}` : describe(reply);
		await load();
	}

	async function remove(identity: string) {
		const reply = await call('DELETE', `${base}/members/${identity}`, { headers: headers() });
		message = reply.status === 204 ? 'removed' : describe(reply);
		await load();
	}

	async function invite(event: SubmitEvent) {
		event.preventDefault();
		const reply = await call('POST', `${base}/invitations`, { headers: headers(), body: JSON.stringify({ role, label }) });
		link = reply.status === 201 ? ((reply.body as { data: Invitation }).data.link ?? '') : '';
		message = reply.status === 201 ? 'invited' : describe(reply);
		await load();
	}

	async function revoke(id: string) {
		const reply = await call('DELETE', `${base}/invitations/${id}`, { headers: headers() });
		message = reply.status === 204 ? 'revoked' : describe(reply);
		await load();
	}

	/** Accepts a whole link or just the token after its `#`. */
	const tokenOf = (text: string) => text.slice(text.indexOf('#') + 1).trim();

	async function lookup() {
		const reply = await call('POST', '/api/v1/invitations/lookup', { headers: headers(), body: JSON.stringify({ token: tokenOf(token) }) });
		if (reply.status === 200) {
			const data = (reply.body as { data: { project_name: string; role: string; invited_by: string | null } }).data;
			offer = `${data.project_name} as ${data.role}, from ${data.invited_by ?? 'someone'}`;
			message = 'found';
		} else {
			offer = '';
			message = describe(reply);
		}
	}

	async function accept() {
		const reply = await call('POST', '/api/v1/invitations/accept', { headers: headers(), body: JSON.stringify({ token: tokenOf(token) }) });
		message = reply.status === 200 ? `joined as ${(reply.body as { data: { role: string } }).data.role}` : describe(reply);
		await loadProjects();
	}

	onMount(loadProjects);
</script>

<h1>Members</h1>
<p class="lede">
	Members and invitations through the real API (D-042): change roles, remove people, invite with a link whose token is shown once,
	and, signed in as someone else, look up and accept an invitation. A project can never lose its last owner.
</p>

{#if signedIn === false}
	<p data-testid="signed-out">Not signed in. <a href={resolve('/session')}>Sign in on the Session page</a> first.</p>
{:else if signedIn}
	{#if message}<p data-testid="message">{message}</p>{/if}

	<section>
		<h2>Accept an invitation</h2>
		<form onsubmit={(event) => event.preventDefault()}>
			<label class="field">Invitation link or token <input bind:value={token} name="token" /></label>
			<p>
				<button type="button" onclick={lookup}>Look up</button>
				<button type="button" onclick={accept}>Accept</button>
			</p>
		</form>
		{#if offer}<p data-testid="offer">{offer}</p>{/if}
	</section>

	{#if projects.length > 0}
		<section>
			<h2>Project</h2>
			<label class="field">
				Project
				<select bind:value={projectId} onchange={load} name="project">
					{#each projects as project (project.id)}
						<option value={project.id}>{project.name} ({project.slug})</option>
					{/each}
				</select>
			</label>
			<ul data-testid="members">
				{#each members as member (member.identity_id)}
					{@const name = member.display_name ?? member.identity_id}
					<li data-role={member.role}>
						{name}
						<select aria-label="Role of {name}" value={member.role} onchange={(event) => setRole(member.identity_id, event.currentTarget.value)}>
							{#each ROLES as option (option)}<option value={option}>{option}</option>{/each}
						</select>
						<button class="link" onclick={() => remove(member.identity_id)}>Remove {name}</button>
					</li>
				{/each}
			</ul>
		</section>

		<section>
			<h2>Invite</h2>
			<form onsubmit={invite}>
				<label class="field">Label <input bind:value={label} name="label" /></label>
				<label class="field">
					Invitation role
					<select bind:value={role} name="role">
						{#each ROLES as option (option)}<option value={option}>{option}</option>{/each}
					</select>
				</label>
				<button type="submit">Create invitation</button>
			</form>
			{#if link}<p>Link (shown once): <code data-testid="link">{link}</code></p>{/if}
			<ul data-testid="invitations">
				{#each pending as invitation (invitation.id)}
					<li>
						{invitation.label} <code>{invitation.role}</code>
						<button class="link" onclick={() => revoke(invitation.id)}>Revoke {invitation.label}</button>
					</li>
				{/each}
			</ul>
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
	select {
		font: inherit;
		padding: 0.4rem 0.5rem;
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
		justify-self: start;
	}
	button.link {
		background: none;
		color: inherit;
		border: none;
		padding: 0;
		text-decoration: underline;
	}
</style>
