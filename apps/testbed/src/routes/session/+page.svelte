<script lang="ts">
	import { onMount } from 'svelte';
	import { call, csrfToken, envelopeOf } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
		role: string;
	}
	interface Me {
		id: string;
		issuer: string;
		subject: string;
		display_name: string | null;
		administrator: boolean;
		projects: Project[];
	}

	// The server under test: sign-in happens on its own origin, and its cookies are shared with
	// this page because both run on "localhost" (cookies are per host, not per port).
	const SERVER = import.meta.env['VITE_COLOPHON_SERVER_ORIGIN'] ?? 'http://localhost:58000';

	let me = $state<Me | null>(null);
	let status = $state<number | null>(null);
	let slug = $state('');
	let name = $state('');
	let message = $state('');
	let email = $state('');
	let password = $state('');
	let passwordMessage = $state('');

	async function refresh() {
		const reply = await call('GET', '/me');
		status = reply.status;
		me = reply.status === 200 ? (reply.body as { data: Me }).data : null;
	}

	async function createProject(event: SubmitEvent) {
		event.preventDefault();
		const reply = await call('POST', '/api/v1/projects', {
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken() },
			body: JSON.stringify({ slug, name })
		});
		message = reply.status === 201 ? `created ${slug}` : `${reply.status} ${envelopeOf(reply.body)?.code ?? ''}: ${envelopeOf(reply.body)?.message ?? ''}`;
		await refresh();
	}

	async function signOut() {
		await call('POST', '/auth/logout', { headers: { 'X-CSRF-Token': csrfToken() } });
		await refresh();
	}

	// Email-and-password accounts (D-084): the response itself sets the same session and CSRF
	// cookies an OIDC callback sets, on this host.
	async function passwordAuth(event: SubmitEvent) {
		event.preventDefault();
		const signingUp = (event.submitter as HTMLButtonElement | null)?.value === 'signup';
		const reply = await call('POST', signingUp ? '/auth/signup' : '/auth/signin', {
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ email, password })
		});
		if (reply.status === 201 || reply.status === 204) {
			passwordMessage = '';
			password = '';
			await refresh();
		} else {
			const envelope = envelopeOf(reply.body);
			passwordMessage = `${reply.status} ${envelope?.code ?? ''}: ${envelope?.message ?? ''}`;
		}
	}

	onMount(refresh);
</script>

<h1>Session</h1>
<p class="lede">
	Sign-in through the server's real OpenID Connect flow (Keycloak in the local stack), then the projects API as that person:
	every write carries the CSRF token and comes from a trusted origin.
</p>

{#if me}
	<p data-testid="signed-in">Signed in as <strong>{me.display_name ?? me.id}</strong> <small>({me.issuer})</small></p>
	<p data-testid="standing">
		subject <code>{me.subject}</code>, {me.administrator ? 'administrator: may create projects' : 'not an administrator'}
	</p>
	<h2>Projects</h2>
	<ul data-testid="projects">
		{#each me.projects as project (project.id)}
			<li data-slug={project.slug} data-id={project.id}><code>{project.slug}</code> {project.name} <small>{project.role}</small></li>
		{:else}
			<li>None yet.</li>
		{/each}
	</ul>
	<form onsubmit={createProject}>
		<label>Slug <input bind:value={slug} name="slug" /></label>
		<label>Name <input bind:value={name} name="name" /></label>
		<button type="submit">Create project</button>
	</form>
	{#if message}<p data-testid="message">{message}</p>{/if}
	<p><button onclick={signOut}>Sign out</button></p>
{:else}
	<p data-testid="signed-out">Not signed in{status === null ? '' : ` (HTTP ${status})`}.</p>
	<p><a href="{SERVER}/auth/login?return_to=/me" data-testid="sign-in">Sign in</a></p>
	<h2>Email and password</h2>
	<p class="lede">An account this Colophon keeps itself (D-084), beside the organisation's sign-in.</p>
	<form onsubmit={passwordAuth}>
		<label>Email <input bind:value={email} name="email" type="email" required /></label>
		<label>Password <input bind:value={password} name="password" type="password" required minlength="8" /></label>
		<button type="submit" name="way" value="signin">Sign in</button>
		<button type="submit" name="way" value="signup">Create account</button>
	</form>
	{#if passwordMessage}<p data-testid="password-message">{passwordMessage}</p>{/if}
{/if}

<style>
	.lede {
		color: var(--muted);
		max-width: 46rem;
	}
	form {
		display: flex;
		gap: 0.75rem;
		flex-wrap: wrap;
		align-items: end;
	}
	label {
		display: grid;
		gap: 0.25rem;
		font-size: 0.875rem;
		color: var(--muted);
	}
	input {
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
	}
</style>
