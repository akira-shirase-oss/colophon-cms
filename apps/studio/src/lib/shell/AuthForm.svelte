<!--
  Signing in with an email and a password, or creating that account. One card, two modes; the
  session cookie comes back in the response itself, and a successful answer makes the parent's
  `me` query refetch, so the page moves on without a redirect.
-->
<script lang="ts">
	import { useQueryClient } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { ApiError, signInPassword, signInUrl, signUp } from '#lib/api.ts';
	import FormRow from '#lib/shell/FormRow.svelte';

	/** Where the single sign-on button comes back to afterwards. */
	let { returnTo = '/' }: { returnTo?: string } = $props();

	const queryClient = useQueryClient();
	let mode = $state<'signin' | 'signup'>('signin');
	let name = $state('');
	let email = $state('');
	let password = $state('');
	let error = $state('');
	let busy = $state(false);

	function words(failure: unknown): string {
		// The server's sign-in failure is generic by design; for a password it means one thing.
		if (failure instanceof ApiError && failure.status === 400) {
			return 'That email and password did not match.';
		}
		return failure instanceof ApiError ? failure.message : String(failure);
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		try {
			if (mode === 'signup') await signUp(email, password, name.trim() || undefined);
			else await signInPassword(email, password);
			await queryClient.invalidateQueries({ queryKey: ['me'] });
		} catch (failure) {
			error = words(failure);
		} finally {
			busy = false;
		}
	}

	function swap(next: typeof mode) {
		mode = next;
		error = '';
	}
</script>

<form onsubmit={submit} data-testid="password-auth">
	{#if mode === 'signup'}
		<FormRow label="Your name" hint="Shown to the people you work with." for="auth-name">
			<Input id="auth-name" bind:value={name} maxlength={200} autocomplete="name" />
		</FormRow>
	{/if}
	<FormRow label="Email" for="auth-email">
		<Input id="auth-email" type="email" bind:value={email} required maxlength={254} autocomplete="email" />
	</FormRow>
	<FormRow
		label="Password"
		hint={mode === 'signup' ? 'At least 8 characters.' : undefined}
		for="auth-password"
	>
		<Input
			id="auth-password"
			type="password"
			bind:value={password}
			required
			minlength={8}
			maxlength={512}
			autocomplete={mode === 'signup' ? 'new-password' : 'current-password'}
		/>
	</FormRow>
	{#if error}<p role="alert" class="pt-2 text-sm text-destructive">{error}</p>{/if}
	<div class="pt-4">
		<Button type="submit" class="w-full" disabled={busy} data-testid="sign-in">
			{mode === 'signup' ? 'Create account and continue' : 'Sign in'}
		</Button>
	</div>
</form>
<p class="pt-4 text-center text-sm text-muted-foreground">
	{#if mode === 'signin'}
		No account yet? <button type="button" class="underline underline-offset-2 hover:text-foreground" onclick={() => swap('signup')}>Create one</button>
	{:else}
		Already have an account? <button type="button" class="underline underline-offset-2 hover:text-foreground" onclick={() => swap('signin')}>Sign in</button>
	{/if}
</p>
<div class="my-6 flex items-center gap-3 text-sm text-muted-foreground" aria-hidden="true">
	<span class="h-px flex-1 bg-border"></span>or<span class="h-px flex-1 bg-border"></span>
</div>
<Button variant="outline" class="w-full" href={signInUrl(returnTo)} data-sveltekit-reload>
	Sign in with your organisation
</Button>
