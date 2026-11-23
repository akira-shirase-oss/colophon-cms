<script lang="ts">
	import { onMount } from 'svelte';
	import { call, csrfToken, envelopeOf } from '#lib/api.ts';

	// Self-serve sign-up (D-235) as a site visitor meets it: what the server publishes decides the
	// form (registration, and Turnstile only with a site key); a new account reads its own usage.
	interface Providers {
		sso: boolean;
		turnstile_site_key: string | null;
		registration: 'open' | 'invite' | 'closed';
	}
	interface Meter {
		used: number;
		limit: number | null;
	}
	type Usage = Record<'projects' | 'entries' | 'assets' | 'storage_bytes', Meter>;

	let providers = $state<Providers | null>(null);
	let usage = $state<Usage | null>(null);
	let email = $state('');
	let password = $state('');
	let message = $state('');
	let slug = $state('');

	async function readUsage() {
		const reply = await call('GET', '/api/v1/me/usage');
		usage = reply.status === 200 ? (reply.body as { data: Usage }).data : null;
	}

	async function signUp(event: SubmitEvent) {
		event.preventDefault();
		const reply = await call('POST', '/auth/signup', {
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ email, password })
		});
		const envelope = envelopeOf(reply.body);
		message = reply.status === 201 ? 'signed up' : `${reply.status} ${envelope?.code ?? ''}: ${envelope?.message ?? ''}`;
		await readUsage();
	}

	async function createProject(event: SubmitEvent) {
		event.preventDefault();
		const reply = await call('POST', '/api/v1/projects', {
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken() },
			body: JSON.stringify({ slug, name: slug })
		});
		const envelope = envelopeOf(reply.body);
		message = reply.status === 201 ? `created ${slug}` : `${reply.status} ${envelope?.code ?? ''}: ${envelope?.message ?? ''}`;
		await readUsage();
	}

	onMount(async () => {
		const reply = await call('GET', '/auth/providers');
		providers = reply.status === 200 ? (reply.body as { data: Providers }).data : null;
		await readUsage();
	});
</script>

<h1>Sign-up</h1>
<p class="lede">Self-serve sign-up (D-235): the registration mode and bot check the server publishes, a new account, and its own usage against its limits.</p>

{#if providers}
	<p data-testid="providers">
		registration <code>{providers.registration}</code>, bot check <code>{providers.turnstile_site_key === null ? 'off' : 'on'}</code>
	</p>
{/if}

{#if usage}
	<table data-testid="usage">
		<tbody>
			{#each Object.entries(usage) as [name, meter] (name)}
				<tr data-meter={name}><th>{name}</th><td>{meter.used}</td><td>{meter.limit === null ? 'unbounded' : `of ${meter.limit}`}</td></tr>
			{/each}
		</tbody>
	</table>
	<form onsubmit={createProject}>
		<label>Project slug <input bind:value={slug} name="slug" /></label>
		<button type="submit">Create project</button>
	</form>
{:else}
	<form onsubmit={signUp}>
		<label>Email <input bind:value={email} name="email" type="email" required /></label>
		<label>Password <input bind:value={password} name="password" type="password" required minlength="8" /></label>
		<button type="submit">Sign up</button>
	</form>
{/if}
{#if message}<p data-testid="message">{message}</p>{/if}

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
	td,
	th {
		padding: 0.25rem 0.75rem 0.25rem 0;
		text-align: left;
	}
</style>
