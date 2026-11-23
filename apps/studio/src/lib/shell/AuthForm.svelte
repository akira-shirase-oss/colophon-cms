<!--
  The front door's card (D-084): signing in, or creating an account, with a link between the two.
  The forms themselves are SignInForm and SignUpForm, which /signin and /signup use alone. The
  provider button shows only when this server actually has one configured (D-141).
-->
<script lang="ts">
	import { untrack } from 'svelte';
	import { goto } from '$app/navigation';
	import { createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { signInUrl, type JoinedBySignUp } from '#lib/api.ts';
	import { providersQuery } from '#lib/queries.ts';
	import SignInForm from './SignInForm.svelte';
	import SignUpForm from './SignUpForm.svelte';

	/**
	 * `returnTo`: where the single sign-on button comes back to afterwards, and where the card
	 * navigates when `navigateAfter` is set (D-146). `start`: the mode the card opens in.
	 * `invitation`: an invitation's token a new account claims as it is made (D-235); the project
	 * it joined goes to `onJoined`. `firstRun`: the installation's first account (D-147).
	 */
	let {
		returnTo = '/',
		navigateAfter = false,
		start = 'signin',
		invitation = undefined,
		firstRun = false,
		onJoined = undefined
	}: {
		returnTo?: string;
		navigateAfter?: boolean;
		start?: 'signin' | 'signup';
		invitation?: string;
		firstRun?: boolean;
		onJoined?: (joined: JoinedBySignUp) => Promise<void> | void;
	} = $props();

	// Until the answer arrives the button stays: only an explicit `false` hides it.
	const providers = createQuery(providersQuery);
	let mode = $state<'signin' | 'signup'>(untrack(() => start));
	const canSignUp = $derived(providers.data?.registration !== 'closed');

	async function onward() {
		// Only same-site paths, ever (the server's safe_return_path makes the same rule).
		if (navigateAfter && returnTo.startsWith('/') && !returnTo.startsWith('//')) await goto(returnTo);
	}

	async function signedUp(joined: JoinedBySignUp | null) {
		if (joined && onJoined) await onJoined(joined);
		else await onward();
	}
</script>

{#if mode === 'signup'}
	<SignUpForm {invitation} {firstRun} onSignedUp={signedUp} />
{:else}
	<SignInForm onSignedIn={onward} />
{/if}
<p class="pt-4 text-center text-sm text-muted-foreground">
	{#if mode === 'signin' && canSignUp}
		No account yet? <button type="button" class="underline underline-offset-2 hover:text-foreground" onclick={() => (mode = 'signup')}>Create one</button>
	{:else if mode === 'signup'}
		Already have an account? <button type="button" class="underline underline-offset-2 hover:text-foreground" onclick={() => (mode = 'signin')}>Sign in</button>
	{/if}
</p>
{#if providers.data?.sso !== false}
	<div class="my-6 flex items-center gap-3 text-sm text-muted-foreground" aria-hidden="true">
		<span class="h-px flex-1 bg-border"></span>or<span class="h-px flex-1 bg-border"></span>
	</div>
	<Button variant="outline" class="w-full" href={signInUrl(returnTo)} data-sveltekit-reload data-testid="sign-in">
		Sign in with your organisation
	</Button>
{/if}
