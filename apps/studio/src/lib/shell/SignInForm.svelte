<!--
  Signing in with an email and a password (D-084). The session cookie comes back in the response
  itself; a successful answer makes the `me` query refetch and calls `onSignedIn`.
-->
<script lang="ts">
	import { useQueryClient } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { signInPassword } from '#lib/api.ts';
	import { refusal } from './authErrors.ts';

	let { onSignedIn = undefined }: { onSignedIn?: () => Promise<void> | void } = $props();

	const queryClient = useQueryClient();
	let email = $state('');
	let password = $state('');
	let error = $state('');
	let busy = $state(false);

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		try {
			await signInPassword(email, password);
			await queryClient.invalidateQueries({ queryKey: ['me'] });
			await onSignedIn?.();
		} catch (failure) {
			error = refusal(failure, 'signin').form;
		} finally {
			busy = false;
		}
	}
</script>

<form onsubmit={submit} data-testid="password-auth" class="space-y-3">
	<div class="space-y-1">
		<label for="auth-email" class="text-sm font-medium">Email</label>
		<Input id="auth-email" type="email" bind:value={email} required maxlength={254} autocomplete="email" />
	</div>
	<div class="space-y-1">
		<label for="auth-password" class="text-sm font-medium">Password</label>
		<Input id="auth-password" type="password" bind:value={password} required minlength={8} maxlength={512} autocomplete="current-password" />
	</div>
	{#if error}<p role="alert" class="pt-2 text-sm text-destructive" data-testid="auth-error">{error}</p>{/if}
	<div class="pt-4">
		<Button type="submit" class="w-full" disabled={busy} data-testid="password-submit">{busy ? 'Signing in…' : 'Sign in'}</Button>
	</div>
</form>
