<script lang="ts">
	import { onMount } from 'svelte';
	import { call, csrfToken, detailsOf, envelopeOf } from '#lib/api.ts';

	// Step 3.7, D-061: a fresh installation's setup code and the identity provider's sign-up form,
	// through the real API. On an installation that already has an administrator (this suite's,
	// which scripts/migrate.sh bootstraps), every code is refused with 409 `already_set_up`.
	const SERVER = import.meta.env['VITE_COLOPHON_SERVER_ORIGIN'] ?? 'http://localhost:58000';

	let status = $state<number | null>(null);
	let hasAdministrator = $state<boolean | null>(null);
	let administrator = $state<boolean | null>(null);
	let code = $state('');
	let outcome = $state('');

	async function refresh() {
		const reply = await call('GET', '/me');
		status = reply.status;
		const data = reply.status === 200 ? (reply.body as { data: { administrator: boolean; installation: { has_administrator: boolean } } }).data : null;
		administrator = data?.administrator ?? null;
		hasAdministrator = data?.installation.has_administrator ?? null;
	}

	async function claim(event: SubmitEvent) {
		event.preventDefault();
		const reply = await call('POST', '/api/v1/setup', {
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken() },
			body: JSON.stringify({ code })
		});
		const retry = reply.headers.get('retry-after');
		outcome =
			reply.status === 200
				? '200 administrator'
				: `${reply.status} ${envelopeOf(reply.body)?.code ?? ''} ${detailsOf(reply.body)?.reason ?? ''}${retry === null ? '' : ` retry-after ${retry}`}`.trim();
		await refresh();
	}

	onMount(refresh);
</script>

<h1>Setup</h1>
<p class="lede">
	A fresh installation's first administrator: the server prints a one-time setup code to its terminal when it starts with nobody in charge,
	and a signed-in person presents it here (<code>POST /api/v1/setup</code>). Single-use, expiring, rate-limited, and refused once anyone is an
	administrator. New people make their account at the identity provider.
</p>

<p><a href="{SERVER}/auth/login?start=create&return_to=/me" data-testid="create-account">Create an account at the identity provider</a></p>

{#if status === 200}
	<p data-testid="installation">
		{hasAdministrator ? 'This installation has an administrator' : 'This installation has no administrator yet'}; you are
		{administrator ? 'an administrator' : 'not an administrator'}.
	</p>
{:else if status !== null}
	<p data-testid="installation">Not signed in (HTTP {status}): presenting a code needs a session.</p>
{/if}

<form onsubmit={claim}>
	<label>Setup code <input bind:value={code} name="code" autocomplete="off" /></label>
	<button type="submit">Present the code</button>
</form>
{#if outcome}<p data-testid="setup-outcome">{outcome}</p>{/if}

<style>
	.lede {
		color: var(--muted);
		max-width: 46rem;
	}
	form {
		display: flex;
		gap: 0.75rem;
		align-items: end;
	}
	label {
		display: grid;
		gap: 0.25rem;
		font-size: 0.85rem;
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
