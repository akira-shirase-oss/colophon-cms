<!--
  Approving a terminal (D-041): `colophon login` prints a link here with its code. The page shows
  what is asking and the code to compare with the terminal, and signs it in only on "Allow".
-->
<script lang="ts">
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui';
	import { ApiError, approveDevice, deviceLogin, me, signInUrl } from '#lib/api.ts';

	const code = $derived((page.url.searchParams.get('code') ?? '').toUpperCase());
	const who = createQuery(() => ({ queryKey: ['me'], queryFn: me }));
	const asking = createQuery(() => ({
		queryKey: ['device', code],
		queryFn: () => deviceLogin(code),
		enabled: who.data != null && code !== '',
		retry: false
	}));

	let outcome = $state<'allowed' | 'refused' | null>(null);
	let error = $state('');
	let busy = $state(false);

	async function allow() {
		busy = true;
		error = '';
		try {
			await approveDevice(code);
			outcome = 'allowed';
		} catch (failure) {
			error = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>Sign in a terminal · Colophon Studio</title></svelte:head>

<h1>Sign in a terminal</h1>
{#if who.isPending}
	<p class="muted">Loading…</p>
{:else if who.data === null}
	<p data-testid="signed-out">Sign in first, then come back to this page to approve the terminal.</p>
	<a class="action" href={signInUrl(page.url.pathname + page.url.search)} data-sveltekit-reload>Sign in</a>
{:else if code === ''}
	<p role="alert">This link has no code. Run <code>colophon login</code> again and open the link it prints.</p>
{:else if outcome === 'allowed'}
	<p data-testid="device-outcome" role="status">Done. The terminal is signed in as you; you can return to it.</p>
{:else if outcome === 'refused'}
	<p data-testid="device-outcome" role="status">Not allowed. You can close this page; the code expires on its own.</p>
{:else if asking.isPending}
	<p class="muted">Loading…</p>
{:else if asking.isError}
	<p role="alert" data-testid="device-outcome">
		{asking.error instanceof ApiError && asking.error.failure === 'not-found'
			? 'This code has expired or was already used. Run colophon login again.'
			: asking.error.message}
	</p>
{:else if asking.data.approved}
	<p data-testid="device-outcome" role="status">This terminal has already been approved.</p>
{:else}
	<section class="ask" data-testid="device-ask">
		<p>
			<strong>{asking.data.client_name}</strong> wants to sign in to Colophon as
			<strong>{who.data?.display_name ?? who.data?.id ?? 'you'}</strong>.
		</p>
		<p>Check that your terminal shows this code: <code class="code" data-testid="device-code">{code}</code></p>
		<p class="muted">The code works until {new Date(asking.data.expires_at).toLocaleTimeString()}. Only allow a terminal you just used.</p>
		{#if error}<p role="alert" class="error">{error}</p>{/if}
		<div class="actions">
			<Button onclick={allow} disabled={busy}>Allow</Button>
			<Button variant="outlined" onclick={() => (outcome = 'refused')}>Don't allow</Button>
		</div>
	</section>
{/if}

<style>
	.muted {
		color: var(--md-sys-color-on-surface-variant);
	}
	.ask {
		max-width: 36rem;
	}
	.code {
		font: var(--md-sys-typescale-title-large);
		letter-spacing: 0.1em;
	}
	.actions {
		display: flex;
		gap: 0.75rem;
	}
	.error {
		color: var(--md-sys-color-error);
	}
	.action {
		display: inline-block;
		padding: 0.625rem 1.5rem;
		border-radius: var(--md-sys-shape-corner-full, 999px);
		background: var(--md-sys-color-primary);
		color: var(--md-sys-color-on-primary);
		text-decoration: none;
		font: var(--md-sys-typescale-label-large);
	}
</style>
