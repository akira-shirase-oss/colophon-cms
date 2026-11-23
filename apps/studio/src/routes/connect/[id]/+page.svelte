<!--
  Connecting an app over OAuth (D-246): an AI app (Claude, ChatGPT, Cursor…) that was pointed at
  /mcp sends its person here through /oauth/authorize. The page says what is asking (its name is its
  own, unverified claim), where the answer goes (the host, so a name can't hide it), whose account
  it acts as, and in which project, with what that role allows. Only "Approve" connects it.
-->
<script lang="ts">
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { ApiError, answerOAuth, getProject, listContentTypes, me, oauthRequest, signInUrl } from '#lib/api.ts';
	import { grantWords } from '#lib/permissions.ts';

	const id = $derived(page.params.id ?? '');
	const who = createQuery(() => ({ queryKey: ['me'], queryFn: me }));
	const asking = createQuery(() => ({
		queryKey: ['oauth-request', id],
		queryFn: () => oauthRequest(id),
		enabled: who.data != null && id !== '',
		retry: false
	}));
	const projects = $derived(who.data?.projects ?? []);
	let chosen = $state('');
	const project = $derived(chosen || projects[0]?.id || '');
	const role = createQuery(() => ({
		queryKey: ['project', project],
		queryFn: () => getProject(project),
		enabled: project !== ''
	}));
	const types = createQuery(() => ({
		queryKey: ['content-types', project],
		queryFn: () => listContentTypes(project),
		enabled: project !== ''
	}));
	const allowed = $derived(
		(role.data?.permissions ?? []).map((grant) => grantWords(grant, types.data ?? []))
	);

	let busy = $state(false);
	let error = $state('');
	let leaving = $state('');

	async function decide(decision: 'approve' | 'deny') {
		busy = true;
		error = '';
		try {
			const redirect = await answerOAuth(id, decision, decision === 'approve' ? project : undefined);
			leaving = decision === 'approve' ? 'Connected. Taking you back to the app…' : 'Not connected. Taking you back to the app…';
			window.location.assign(redirect);
		} catch (failure) {
			error = failure instanceof ApiError && failure.failure === 'not-found'
				? 'This request has expired or was already answered. Start connecting again from the app.'
				: failure instanceof Error ? failure.message : String(failure);
			busy = false;
		}
	}
</script>

<svelte:head><title>Connect an app · Colophon Studio</title></svelte:head>

<div class="mx-auto max-w-lg space-y-4 rounded-2xl border border-border bg-card p-8">
	<h1 class="page-title">Connect an app</h1>
	{#if who.isPending}
		<p class="text-sm text-muted-foreground">Loading…</p>
	{:else if who.data === null}
		<p data-testid="signed-out">Sign in first, then come back to connect the app.</p>
		<Button href={signInUrl(page.url.pathname)} data-sveltekit-reload>Sign in</Button>
	{:else if leaving}
		<p role="status" data-testid="connect-outcome">{leaving}</p>
	{:else if asking.isPending}
		<p class="text-sm text-muted-foreground">Loading…</p>
	{:else if asking.isError}
		<p role="alert" data-testid="connect-outcome">
			{asking.error instanceof ApiError && asking.error.failure === 'not-found'
				? 'This request has expired or was already answered. Start connecting again from the app.'
				: asking.error.message}
		</p>
	{:else if projects.length === 0}
		<p role="alert" data-testid="connect-outcome">You aren't in any project yet, so there is nothing to connect the app to.</p>
		<Button variant="outline" onclick={() => decide('deny')} disabled={busy}>Don't connect</Button>
	{:else}
		<section class="space-y-4" data-testid="connect-ask">
			<p>
				<strong data-testid="client-name">{asking.data.client_name}</strong>
				<span class="rounded-md bg-muted px-1.5 py-0.5 text-sm text-muted-foreground" title="The app named itself; Colophon can't check that name.">unverified</span>
				wants to act as <strong>{who.data?.display_name ?? 'you'}</strong>.
			</p>
			<p class="text-sm">
				It will send you back to <strong data-testid="redirect-host">{asking.data.redirect_host}</strong>. If that isn't the app you were just using, don't connect it.
			</p>
			<label class="block space-y-1 text-sm">
				<span class="font-medium">In this project</span>
				<select class="w-full rounded-md border border-input bg-background px-3 py-2" bind:value={chosen} data-testid="connect-project">
					{#each projects as one (one.id)}
						<option value={one.id} selected={one.id === project}>{one.name} ({one.role_name})</option>
					{/each}
				</select>
			</label>
			<div class="space-y-1 text-sm">
				<p class="font-medium">It can do what your role there allows:</p>
				{#if role.isPending || types.isPending}
					<p class="text-muted-foreground">Loading…</p>
				{:else if allowed.length === 0}
					<p class="text-muted-foreground">Read what you can read.</p>
				{:else}
					<ul class="list-disc space-y-0.5 ps-5" data-testid="connect-permissions">
						{#each allowed as words (words)}<li>{words}</li>{/each}
					</ul>
				{/if}
				<p class="text-muted-foreground">Before anything that deletes or can't be undone, Colophon makes the app ask you first. You can disconnect it any time under Connected apps.</p>
			</div>
			{#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
			<div class="flex gap-2 pt-2">
				<Button onclick={() => decide('approve')} disabled={busy || project === ''} data-testid="connect-approve">Connect</Button>
				<Button variant="outline" onclick={() => decide('deny')} disabled={busy} data-testid="connect-deny">Don't connect</Button>
			</div>
		</section>
	{/if}
</div>
