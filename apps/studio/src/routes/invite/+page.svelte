<!--
  Joining a project with an invitation link (D-042). The token is in the link's fragment, which
  browsers never send to a server. The page keeps it in this tab's session storage, so it
  survives signing in without ever appearing in a URL the server sees, takes it out of the
  address bar, shows what joining means, and joins only on "Join".
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { replaceState } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { acceptInvitation, ApiError, lookupInvitation, me, signInUrl } from '#lib/api.ts';

	const KEPT = 'colophon-invitation';

	function kept(): string {
		try {
			return sessionStorage.getItem(KEPT) ?? '';
		} catch {
			return '';
		}
	}

	function keep(token: string) {
		try {
			if (token === '') sessionStorage.removeItem(KEPT);
			else sessionStorage.setItem(KEPT, token);
		} catch {
			// Without storage the token only lives in this page; signing in first then needs the link again.
		}
	}

	const fromLink = page.url.hash.slice(1);
	const token = fromLink === '' ? kept() : fromLink;
	if (fromLink !== '') keep(fromLink);
	onMount(() => {
		if (fromLink !== '') replaceState(page.url.pathname, {});
	});

	const queryClient = useQueryClient();
	const who = createQuery(() => ({ queryKey: ['me'], queryFn: me }));
	const offer = createQuery(() => ({
		queryKey: ['invitation', token],
		queryFn: () => lookupInvitation(token),
		enabled: who.data != null && token !== '',
		retry: false
	}));

	let outcome = $state<'joined' | 'declined' | null>(null);
	let error = $state('');
	let busy = $state(false);

	async function join() {
		busy = true;
		error = '';
		try {
			await acceptInvitation(token);
			keep('');
			outcome = 'joined';
			await queryClient.invalidateQueries({ queryKey: ['me'] });
		} catch (failure) {
			error = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			busy = false;
		}
	}


	function decline() {
		keep('');
		outcome = 'declined';
	}
</script>

<svelte:head><title>Join a project · Colophon Studio</title></svelte:head>

<div class="mx-auto max-w-lg space-y-4 rounded-2xl border border-border bg-card p-8 shadow-xs">
<h1 class="text-2xl font-semibold tracking-tight">Join a project</h1>
{#if token === ''}
	<p role="alert" data-testid="invite-outcome">This link has no invitation in it. Ask the person who invited you for a new link.</p>
{:else if who.isPending}
	<p class="text-sm text-muted-foreground">Loading…</p>
{:else if who.data === null}
	<p data-testid="signed-out">Sign in first; this page will show the invitation afterwards.</p>
	<Button href={signInUrl(resolve('/invite'))} data-sveltekit-reload>Sign in</Button>
{:else if outcome === 'joined' && offer.data}
	<p data-testid="invite-outcome" role="status">
		You joined <a class="underline underline-offset-2" href={resolve("/projects/[project]", { project: offer.data.project_id })}>{offer.data.project_name}</a>
		as {offer.data.role_name}.
	</p>
{:else if outcome === 'declined'}
	<p data-testid="invite-outcome" role="status">Not joined. The invitation stays unused until it expires.</p>
{:else if offer.isPending}
	<p class="text-sm text-muted-foreground">Loading…</p>
{:else if offer.isError}
	<p role="alert" data-testid="invite-outcome">
		{offer.error instanceof ApiError && offer.error.failure === 'not-found'
			? 'This invitation has expired, was revoked, or was already used. Ask the person who sent it for a new one.'
			: offer.error.message}
	</p>
{:else}
	<section class="space-y-3" data-testid="invite-ask">
		<p>
			{offer.data.invited_by ?? 'Someone'} invited you to join <strong>{offer.data.project_name}</strong> as
			<strong>{offer.data.role_name}</strong>{offer.data.role_description ? `: ${offer.data.role_description}` : '.'}
		</p>
		<p class="text-sm text-muted-foreground">
			You're signed in as {who.data?.display_name ?? who.data?.id}. The invitation works once, until
			{new Date(offer.data.expires_at).toLocaleString()}.
		</p>
		{#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
		<div class="flex gap-2 pt-2">
			<Button onclick={join} disabled={busy}>Join</Button>
			<Button variant="outline" onclick={decline}>Don't join</Button>
		</div>
	</section>
{/if}
</div>
