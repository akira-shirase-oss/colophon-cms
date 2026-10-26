<script lang="ts">
	import { call, csrfToken, envelopeOf } from '#lib/api.ts';

	interface Started {
		device_secret: string;
		user_code: string;
		verification_uri: string;
		interval: number;
	}

	let name = $state('testbed terminal');
	let started = $state<Started | null>(null);
	let status = $state('');
	let token = $state('');
	let me = $state('');

	async function start(event: SubmitEvent) {
		event.preventDefault();
		token = '';
		me = '';
		const reply = await call('POST', '/auth/device', {
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ client_name: name })
		});
		started = reply.status === 201 ? (reply.body as { data: Started }).data : null;
		status = reply.status === 201 ? 'pending' : `${reply.status} ${envelopeOf(reply.body)?.code ?? ''}`;
	}

	async function approve() {
		if (started === null) return;
		const reply = await call('POST', '/auth/device/approve', {
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken() },
			body: JSON.stringify({ user_code: started.user_code })
		});
		status = reply.status === 204 ? 'approved in the browser' : `${reply.status} ${envelopeOf(reply.body)?.code ?? ''}`;
	}

	async function poll() {
		if (started === null) return;
		const reply = await call('POST', '/auth/device/token', {
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ device_secret: started.device_secret })
		});
		if (reply.status !== 200) {
			status = `${reply.status} ${envelopeOf(reply.body)?.code ?? ''}`;
			return;
		}
		const data = (reply.body as { data: { status: string; access_token?: string } }).data;
		status = data.status;
		if (data.access_token) {
			token = data.access_token;
			// The token alone, no cookie: what the CLI does.
			const response = await fetch('/server/me', { headers: { Authorization: `Bearer ${token}` }, credentials: 'omit' });
			me = response.status === 200 ? `HTTP 200 as ${((await response.json()) as { data: { display_name: string } }).data.display_name}` : `HTTP ${response.status}`;
		}
	}
</script>

<h1>Device sign-in</h1>
<p class="lede">
	What <code>colophon login</code> does, step by step: start a device sign-in, approve its code as the signed-in person, then
	poll for the access token and use it without any cookie.
</p>

<form onsubmit={start}>
	<label class="field">Terminal name <input bind:value={name} name="name" /></label>
	<button type="submit">Start device sign-in</button>
</form>

{#if started}
	<section data-testid="device">
		<p>Code: <code data-testid="code">{started.user_code}</code></p>
		<p>Approval page: <code data-testid="link">{started.verification_uri}</code></p>
		<p>
			<button onclick={poll}>Poll</button>
			<button onclick={approve}>Approve as me</button>
		</p>
	</section>
{/if}
{#if status}<p data-testid="status">{status}</p>{/if}
{#if token}<p data-testid="token">token: {token.slice(0, 6)}…</p>{/if}
{#if me}<p data-testid="me">{me}</p>{/if}

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
	.field {
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
