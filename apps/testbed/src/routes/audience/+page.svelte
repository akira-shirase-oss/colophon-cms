<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, listProjects, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}
	interface SiteRole {
		name: string;
		self_assign: boolean;
	}
	interface SiteUser {
		id: string;
		email: string;
		display_name: string | null;
		role: string;
		disabled: boolean;
	}
	interface SiteSession {
		id: string;
		created_at: string;
		last_seen_at: string;
		expires_at: string;
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let message = $state('');

	let roles = $state<SiteRole[]>([]);
	let roleName = $state('');
	let roleOpen = $state(true);

	let users = $state<SiteUser[]>([]);
	let email = $state('');
	let password = $state('');
	let displayName = $state('');
	let userRole = $state('');
	let sessions = $state<SiteSession[]>([]);
	let sessionsFor = $state('');

	// The visitor arm: a live audience session, used as a bearer credential — no cookie, no CSRF.
	let visitorToken = $state('');
	let me = $state<SiteUser | null>(null);
	let gateEmail = $state('');
	let gatePassword = $state('');
	let collection = $state('articles');
	let delivery = $state('');

	// The passwordless arm (D-140): request a magic link, then spend the token it carries.
	let linkEmail = $state('');
	let devLink = $state('');

	const staff = $derived(`/api/v1/projects/${projectId}/audience`);
	const audience = $derived(`/audience/v1/${projectId}`);

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason})` : ''}`.trim();
	}

	function headers(extra: Record<string, string> = {}): Record<string, string> {
		return { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), ...extra };
	}

	async function loadProjects() {
		const reply = await listProjects<Project>();
		signedIn = reply.status === 200;
		projects = reply.projects;
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
		if (projectId !== '') await load();
	}

	async function load() {
		const [listedRoles, listedUsers] = await Promise.all([
			call('GET', `${staff}/roles`),
			call('GET', `${staff}/users`)
		]);
		roles = listedRoles.status === 200 ? (listedRoles.body as { data: SiteRole[] }).data : [];
		users = listedUsers.status === 200 ? (listedUsers.body as { data: SiteUser[] }).data : [];
		sessions = [];
		sessionsFor = '';
	}

	async function createRole() {
		const reply = await call('POST', `${staff}/roles`, {
			headers: headers(),
			body: JSON.stringify({ name: roleName, self_assign: roleOpen })
		});
		message = reply.status === 201 ? `created role ${roleName}` : describe(reply);
		await load();
	}

	async function toggleRole(role: SiteRole) {
		const reply = await call('PATCH', `${staff}/roles/${role.name}`, {
			headers: headers(),
			body: JSON.stringify({ self_assign: !role.self_assign })
		});
		message = reply.status === 200 ? `role ${role.name} ${role.self_assign ? 'closed' : 'opened'}` : describe(reply);
		await load();
	}

	async function deleteRole(role: SiteRole) {
		const reply = await call('DELETE', `${staff}/roles/${role.name}`, { headers: headers() });
		message = reply.status === 204 ? `deleted role ${role.name}` : describe(reply);
		await load();
	}

	async function createUser() {
		const reply = await call('POST', `${staff}/users`, {
			headers: headers(),
			body: JSON.stringify({
				email,
				password,
				...(displayName === '' ? {} : { display_name: displayName }),
				role: userRole
			})
		});
		message = reply.status === 201 ? `created user ${email}` : describe(reply);
		await load();
	}

	async function setDisabled(user: SiteUser, disabled: boolean) {
		const reply = await call('PATCH', `${staff}/users/${user.id}`, {
			headers: headers(),
			body: JSON.stringify({ disabled })
		});
		message = reply.status === 200 ? `${user.email} ${disabled ? 'disabled' : 'enabled'}` : describe(reply);
		await load();
	}

	async function loadSessions(user: SiteUser) {
		const reply = await call('GET', `${staff}/users/${user.id}/sessions`);
		if (reply.status === 200) {
			sessions = (reply.body as { data: SiteSession[] }).data;
			sessionsFor = user.email;
		} else {
			message = describe(reply);
		}
	}

	async function revoke(session: SiteSession) {
		const reply = await call('DELETE', `${staff}/sessions/${session.id}`, { headers: headers() });
		message = reply.status === 204 ? `revoked session ${session.id.slice(0, 8)}` : describe(reply);
		sessions = sessions.filter((kept) => kept.id !== session.id);
	}

	/* ------------------------------------------------------------- the visitor */

	async function signup() {
		const reply = await call('POST', `${audience}/signup`, {
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ email: gateEmail, password: gatePassword })
		});
		if (reply.status === 201) {
			const made = (reply.body as { data: { user: SiteUser; session: string } }).data;
			visitorToken = made.session;
			me = made.user;
			message = `signed up as ${made.user.role}`;
			await load();
		} else {
			message = describe(reply);
		}
	}

	async function signin() {
		const reply = await call('POST', `${audience}/signin`, {
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ email: gateEmail, password: gatePassword })
		});
		if (reply.status === 200) {
			const made = (reply.body as { data: { user: SiteUser; session: string } }).data;
			visitorToken = made.session;
			me = made.user;
			message = `signed in as ${made.user.role}`;
			await load();
		} else {
			message = describe(reply);
		}
	}

	async function whoami() {
		const reply = await call('GET', `${audience}/me`, {
			headers: { authorization: `Bearer ${visitorToken}` }
		});
		if (reply.status === 200) {
			me = (reply.body as { data: SiteUser }).data;
			message = `me: ${me.email}`;
		} else {
			message = describe(reply);
		}
	}

	async function signout() {
		const reply = await call('POST', `${audience}/signout`, {
			headers: { authorization: `Bearer ${visitorToken}` }
		});
		message = reply.status === 204 ? 'signed out' : describe(reply);
		visitorToken = '';
		me = null;
	}

	async function requestLink() {
		devLink = '';
		const reply = await call('POST', `${audience}/magic-link`, {
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ email: linkEmail, return_to: `${window.location.origin}/magic` })
		});
		if (reply.status === 202) {
			const data = (reply.body as { data: { dev_link?: string } }).data;
			devLink = data.dev_link ?? '';
			message =
				devLink === ''
					? 'link requested — the server mailed it (no development link to show)'
					: 'link requested — the development answer carries it below';
		} else {
			message = describe(reply);
		}
	}

	async function useLink() {
		const token = devLink.split('token=').at(-1) ?? '';
		const reply = await call('POST', `${audience}/magic-link/${token}`, {
			headers: { 'Content-Type': 'application/json' }
		});
		if (reply.status === 200) {
			const made = (reply.body as { data: { user: SiteUser; session: string } }).data;
			visitorToken = made.session;
			me = made.user;
			devLink = '';
			message = `signed in by link as ${made.user.role}`;
			await load();
		} else {
			message = describe(reply);
		}
	}

	/**
	 * One delivery read anonymous and one as the visitor: the gate, counted. The reads fetch
	 * with credentials omitted — the browser would otherwise send the `colophon_audience`
	 * cookie the proxy set at signup, and nothing would be anonymous.
	 */
	async function checkDelivery() {
		const base = `/server/content/v1/${projectId}/production/${collection}`;
		const read = async (token: string | null) => {
			const response = await fetch(base, {
				credentials: 'omit',
				cache: 'no-store',
				headers: token === null ? {} : { authorization: `Bearer ${token}` }
			});
			if (!response.ok) return `HTTP ${response.status}`;
			return String(((await response.json()) as { data?: unknown[] }).data?.length ?? 0);
		};
		delivery = `anonymous sees ${await read(null)}, this session sees ${await read(visitorToken)}`;
	}

	onMount(loadProjects);
</script>

<h1>Audience</h1>
<p class="lede">
	The site's own visitors (D-137): project-scoped accounts that sign up and sign in at
	<code>/audience/v1</code>, audience roles, and gated published content —
	<code>authenticated</code> or <code>role:&lt;name&gt;</code> visibility that Postgres itself
	enforces on delivery reads. Staff manage roles and users here under
	<code>audience.manage</code>; the visitor arm below drives the public API with a bearer
	credential, the way a site does.
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
		</section>

		<section>
			<h2>Audience roles</h2>
			<ul data-testid="roles">
				{#each roles as role (role.name)}
					<li data-name={role.name}>
						<code>{role.name}</code> — {role.self_assign ? 'open to signup' : 'staff-assigned'}
						<button type="button" onclick={() => toggleRole(role)}>{role.self_assign ? 'Close' : 'Open'}</button>
						<button type="button" onclick={() => deleteRole(role)}>Delete</button>
					</li>
				{/each}
			</ul>
			<form onsubmit={(event) => event.preventDefault()} class="row">
				<label class="field">Name <input bind:value={roleName} name="role-name" /></label>
				<label class="check"><input type="checkbox" bind:checked={roleOpen} name="role-open" /> open to signup</label>
				<button type="button" onclick={createRole}>Create role</button>
			</form>
		</section>

		<section>
			<h2>Audience users</h2>
			<ul data-testid="users">
				{#each users as user (user.id)}
					<li data-email={user.email}>
						<code>{user.email}</code> — role <code>{user.role}</code>{user.disabled ? ', disabled' : ''}
						<button type="button" onclick={() => setDisabled(user, !user.disabled)}>{user.disabled ? 'Enable' : 'Disable'}</button>
						<button type="button" onclick={() => loadSessions(user)}>Sessions</button>
					</li>
				{/each}
			</ul>
			<form onsubmit={(event) => event.preventDefault()} class="grid">
				<label class="field">Email <input bind:value={email} name="user-email" type="email" /></label>
				<label class="field">Password <input bind:value={password} name="user-password" type="password" /></label>
				<label class="field">Display name <input bind:value={displayName} name="user-name" /></label>
				<label class="field">Role <input bind:value={userRole} name="user-role" /></label>
				<button type="button" onclick={createUser}>Create user</button>
			</form>
			{#if sessionsFor !== ''}
				<h3>Sessions for {sessionsFor}</h3>
				<ul data-testid="sessions">
					{#each sessions as session (session.id)}
						<li data-session={session.id}>
							<code>{session.id.slice(0, 8)}</code> expires {session.expires_at}
							<button type="button" onclick={() => revoke(session)}>Revoke</button>
						</li>
					{/each}
				</ul>
			{/if}
		</section>

		<section>
			<h2>A visitor</h2>
			<p data-testid="visitor">
				{#if me !== null}
					Signed in as <strong>{me.email}</strong> (role <code>{me.role}</code>)
				{:else}
					Signed out
				{/if}
			</p>
			<form onsubmit={(event) => event.preventDefault()} class="grid">
				<label class="field">Email <input bind:value={gateEmail} name="visitor-email" type="email" /></label>
				<label class="field">Password <input bind:value={gatePassword} name="visitor-password" type="password" /></label>
				<p class="row">
					<button type="button" onclick={signup}>Sign up</button>
					<button type="button" onclick={signin}>Sign in</button>
					<button type="button" onclick={whoami}>Who am I</button>
					<button type="button" onclick={signout}>Sign out</button>
				</p>
			</form>
		</section>

		<section>
			<h2>A sign-in link</h2>
			<p>
				Passwordless (D-140): requesting a link answers the same whether or not the email
				has an account. Development mail hands the link back in the answer; a production
				server emails it.
			</p>
			<form onsubmit={(event) => event.preventDefault()} class="row">
				<label class="field">Email <input bind:value={linkEmail} name="link-email" type="email" /></label>
				<button type="button" onclick={requestLink}>Send the link</button>
				{#if devLink !== ''}<button type="button" onclick={useLink}>Use the link</button>{/if}
			</form>
			{#if devLink !== ''}<p data-testid="dev-link"><code>{devLink}</code></p>{/if}
		</section>

		<section>
			<h2>The gate, counted</h2>
			<p>Read one collection anonymously and as the visitor's session.</p>
			<form onsubmit={(event) => event.preventDefault()} class="row">
				<label class="field">Collection <input bind:value={collection} name="collection" /></label>
				<button type="button" onclick={checkDelivery}>Check delivery</button>
			</form>
			{#if delivery !== ''}<p data-testid="delivery">{delivery}</p>{/if}
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
	.grid {
		display: grid;
		gap: 0.75rem;
		max-width: 46rem;
	}
	.row {
		display: flex;
		gap: 0.75rem;
		align-items: end;
		margin-top: 0.5rem;
	}
	.field {
		display: grid;
		gap: 0.25rem;
		font-size: 0.875rem;
		color: var(--muted);
	}
	.check {
		display: flex;
		gap: 0.4rem;
		align-items: center;
		font-size: 0.875rem;
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
	}
</style>
