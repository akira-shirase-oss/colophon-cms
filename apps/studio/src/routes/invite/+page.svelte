<!--
  Joining a project with an invitation link (D-042). The token is in the link's fragment, which
  browsers never send to a server. The page keeps it in this tab's session storage, so it
  survives signing in without ever appearing in a URL the server sees, takes it out of the
  address bar, shows what joining means, and joins only on "Join". Joined, it goes straight to that
  project's Your work, with a note saying so (D-082); opened again without a link by someone signed
  in, it says there's nothing to join and offers their projects.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { goto, replaceState } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Button } from '@colophon/ui/ui/button';
	import { acceptInvitation, ApiError, lookupInvitation, me, signInUrl, signUpUrl } from '#lib/api.ts';
	import { rememberProject } from '#lib/lastProject.ts';

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

	let outcome = $state<'declined' | null>(null);
	let error = $state('');
	let busy = $state(false);

	async function join() {
		busy = true;
		error = '';
		try {
			const joined = await acceptInvitation(token);
			keep('');
			// Straight to the work in the project just joined: the home page shows the project last
			// opened here, so this one is remembered first.
			if (who.data) rememberProject(who.data.id, joined.project_id);
			await queryClient.invalidateQueries({ queryKey: ['me'] });
			toast.success(`You joined ${offer.data?.project_name ?? 'the project'} as ${offer.data?.role_name ?? 'a member'}`);
			await goto(resolve('/'));
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
{#if who.isPending}
	<p class="text-sm text-muted-foreground">Loading…</p>
{:else if token === '' && who.data}
	<p data-testid="invite-outcome" role="status">Nothing to join here. Invitations open from the link someone sends you; you're already signed in.</p>
	<div><Button href={resolve('/')} data-testid="open-projects">Open your projects</Button></div>
{:else if token === ''}
	<p role="alert" data-testid="invite-outcome">This link has no invitation in it. Ask the person who invited you for a new link.</p>
{:else if who.data === null}
	<p data-testid="signed-out">Sign in first, or create an account if you don't have one; this page shows the invitation afterwards.</p>
	<div class="flex flex-wrap gap-2">
		<Button href={signInUrl(resolve('/invite'))} data-sveltekit-reload>Sign in</Button>
		<Button variant="outline" href={signUpUrl(resolve('/invite'))} data-sveltekit-reload data-testid="create-account">Create an account</Button>
	</div>
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
