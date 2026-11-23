<!--
  /studio/signup (D-235): creating an account on its own page. An invitation's token may ride in
  the link's fragment (never sent to a server), as on /invite; the page takes it out of the address
  bar and the new account claims it. Closed and invite-only registration show their own states.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { goto, replaceState } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { installation, me, type JoinedBySignUp } from '#lib/api.ts';
	import { rememberProject } from '#lib/lastProject.ts';
	import SignUpForm from '#lib/shell/SignUpForm.svelte';

	const invitation = page.url.hash.slice(1) || undefined;
	onMount(() => {
		if (invitation) replaceState(page.url.pathname, {});
	});

	const who = createQuery(() => ({ queryKey: ['me'], queryFn: me }));
	const install = createQuery(() => ({ queryKey: ['installation'], queryFn: installation }));
	let done = $state(false);

	$effect(() => {
		if (who.data && !done) void goto(resolve('/'), { replaceState: true });
	});

	async function signedUp(joined: JoinedBySignUp | null) {
		done = true;
		const signedIn = who.data ?? (await me());
		if (joined && signedIn) rememberProject(signedIn.id, joined.project_id);
		await goto(resolve('/'));
	}
</script>

<svelte:head><title>Create an account · Colophon Studio</title></svelte:head>

<div class="mx-auto max-w-md py-16">
	<h1 class="page-title text-center">Create your Colophon account</h1>
	<div class="mt-8">
		{#if who.isPending || install.isPending || who.data}
			<div class="space-y-3" aria-busy="true" data-testid="signup-loading"><Skeleton class="h-9 w-full" /><Skeleton class="h-9 w-full" /></div>
		{:else}
			<SignUpForm {invitation} firstRun={install.data?.has_administrator === false} onSignedUp={signedUp} />
			<p class="pt-4 text-center text-sm text-muted-foreground">
				Already have an account? <a href={resolve('/signin')} class="underline underline-offset-2 hover:text-foreground" data-testid="to-signin">Sign in</a>
			</p>
		{/if}
	</div>
</div>
