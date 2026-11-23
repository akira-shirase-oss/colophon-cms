<!--
  /studio/signin (D-235): the sign-in form on its own page. Signed in already, it moves on to
  `return_to` (same-site paths only) or the projects.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { me, signInUrl } from '#lib/api.ts';
	import { providersQuery } from '#lib/queries.ts';
	import SignInForm from '#lib/shell/SignInForm.svelte';

	const who = createQuery(() => ({ queryKey: ['me'], queryFn: me }));
	const providers = createQuery(providersQuery);
	const asked = page.url.searchParams.get('return_to');
	const returnTo = asked !== null && asked.startsWith('/') && !asked.startsWith('//') ? asked : resolve('/');

	$effect(() => {
		if (who.data) void goto(returnTo, { replaceState: true });
	});
</script>

<svelte:head><title>Sign in · Colophon Studio</title></svelte:head>

<div class="mx-auto max-w-md py-16">
	<h1 class="page-title text-center">Sign in to Colophon</h1>
	<div class="mt-8">
		{#if who.isPending || who.data}
			<div class="space-y-3" aria-busy="true"><Skeleton class="h-9 w-full" /><Skeleton class="h-9 w-full" /></div>
		{:else}
			<SignInForm onSignedIn={() => goto(returnTo)} />
			{#if providers.data?.registration !== 'closed'}
				<p class="pt-4 text-center text-sm text-muted-foreground">
					No account yet? <a href={resolve('/signup')} class="underline underline-offset-2 hover:text-foreground" data-testid="to-signup">Create one</a>
				</p>
			{/if}
			{#if providers.data?.sso !== false}
				<div class="my-6 flex items-center gap-3 text-sm text-muted-foreground" aria-hidden="true">
					<span class="h-px flex-1 bg-border"></span>or<span class="h-px flex-1 bg-border"></span>
				</div>
				<Button variant="outline" class="w-full" href={signInUrl(returnTo)} data-sveltekit-reload data-testid="sign-in">Sign in with your organisation</Button>
			{/if}
		{/if}
	</div>
</div>
