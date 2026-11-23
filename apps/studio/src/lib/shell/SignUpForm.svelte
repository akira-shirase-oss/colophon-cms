<!--
  Creating an email-and-password account (D-084, D-235). What the server publishes decides what
  shows: closed registration shows no form at all; invite-only asks for the invitation link unless
  this sign-up carries one or is the installation's first account; the Turnstile widget appears
  only when a site key is published. A 422 puts its message under the field it points at; a 429
  says when to try again and holds the button until then.
-->
<script lang="ts">
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { signUp, type JoinedBySignUp } from '#lib/api.ts';
	import { providersQuery } from '#lib/queries.ts';
	import { refusal, type AuthField } from './authErrors.ts';
	import Turnstile from './Turnstile.svelte';

	/**
	 * `invitation`: an invitation's token this account claims as it is made.
	 * `firstRun`: the installation has no administrator yet, so its first account may sign up
	 * without an invitation even where registration is invite-only.
	 */
	let {
		invitation = undefined,
		firstRun = false,
		onSignedUp = undefined
	}: {
		invitation?: string;
		firstRun?: boolean;
		onSignedUp?: (joined: JoinedBySignUp | null) => Promise<void> | void;
	} = $props();

	const queryClient = useQueryClient();
	const providers = createQuery(providersQuery);
	let name = $state('');
	let email = $state('');
	let password = $state('');
	let token = $state('');
	let fields = $state<Partial<Record<AuthField, string>>>({});
	let error = $state('');
	let heldUntil = $state<number | null>(null);
	let busy = $state(false);
	let check = $state<ReturnType<typeof Turnstile> | null>(null);

	const siteKey = $derived(providers.data?.turnstile_site_key ?? null);
	const registration = $derived(providers.data?.registration);
	const needsInvitation = $derived(registration === 'invite' && !invitation && !firstRun);

	/** Editing a field takes its error away: the button stays usable and the next try is fresh. */
	function edited(field: AuthField) {
		if (fields[field]) fields = { ...fields, [field]: undefined };
		error = heldUntil === null ? '' : error;
	}

	async function submit(event: SubmitEvent) {
		event.preventDefault();
		// The widget hands a new token after each attempt; until it has, say so rather than send.
		if (siteKey !== null && token === '') {
			fields = { ...fields, turnstile_token: 'Wait a moment for the check to finish, then try again.' };
			return;
		}
		busy = true;
		error = '';
		fields = {};
		try {
			const joined = await signUp(email, password, name.trim() || undefined, invitation, token || undefined);
			await queryClient.invalidateQueries({ queryKey: ['me'] });
			await onSignedUp?.(joined);
		} catch (failure) {
			const said = refusal(failure, 'signup', Date.now(), password);
			fields = said.fields;
			error = said.form;
			if (said.retryAt !== null) {
				heldUntil = said.retryAt;
				setTimeout(() => (heldUntil = null), Math.max(said.retryAt - Date.now(), 0));
			}
			// A token is good for one attempt.
			check?.reset();
		} finally {
			busy = false;
		}
	}
</script>

{#if providers.isPending}
	<div class="space-y-3" data-testid="signup-loading" aria-busy="true">
		<Skeleton class="h-9 w-full" />
		<Skeleton class="h-9 w-full" />
		<Skeleton class="h-9 w-full" />
	</div>
{:else if registration === 'closed'}
	<div class="rounded-xl border border-border bg-card px-4 py-3 text-sm text-pretty" data-testid="signup-closed">
		<p class="font-medium">This Colophon isn't taking sign-ups.</p>
		<p class="mt-1 text-muted-foreground">Ask the person who runs it for an account, then sign in.</p>
	</div>
{:else if needsInvitation}
	<div class="rounded-xl border border-border bg-card px-4 py-3 text-sm text-pretty" data-testid="signup-invite-only">
		<p class="font-medium">Sign-ups here are by invitation.</p>
		<p class="mt-1 text-muted-foreground">Open the invitation link you were sent; it brings you back here ready to create your account.</p>
	</div>
{:else}
	<form onsubmit={submit} data-testid="password-auth" class="space-y-3">
		{#if invitation}
			<p class="text-sm text-muted-foreground" data-testid="signup-with-invitation">Your account joins the project you were invited to.</p>
		{/if}
		<div class="space-y-1">
			<label for="auth-name" class="text-sm font-medium">Your name</label>
			<Input id="auth-name" bind:value={name} oninput={() => edited('display_name')} maxlength={200} autocomplete="name" aria-invalid={fields.display_name ? true : undefined} aria-describedby="auth-name-note" />
			<p id="auth-name-note" class={fields.display_name ? 'text-sm text-destructive' : 'text-sm text-muted-foreground'}>
				{fields.display_name ?? 'Shown to the people you work with.'}
			</p>
		</div>
		<div class="space-y-1">
			<label for="auth-email" class="text-sm font-medium">Email</label>
			<Input id="auth-email" type="email" bind:value={email} oninput={() => edited('email')} required maxlength={254} autocomplete="email" aria-invalid={fields.email ? true : undefined} aria-describedby={fields.email ? 'auth-email-note' : undefined} />
			{#if fields.email}<p id="auth-email-note" class="text-sm text-destructive" data-testid="field-error-email">{fields.email}</p>{/if}
		</div>
		<div class="space-y-1">
			<label for="auth-password" class="text-sm font-medium">Password</label>
			<Input
				id="auth-password"
				type="password"
				bind:value={password}
				oninput={() => edited('password')}
				required
				minlength={8}
				maxlength={512}
				autocomplete="new-password"
				aria-invalid={fields.password ? true : undefined}
				aria-describedby="auth-password-note"
			/>
			<p id="auth-password-note" class={fields.password ? 'text-sm text-destructive' : 'text-sm text-muted-foreground'} data-testid={fields.password ? 'field-error-password' : undefined}>
				{fields.password ?? 'At least 8 characters.'}
			</p>
		</div>
		{#if siteKey}
			<div class="space-y-1 pt-1">
				<Turnstile bind:this={check} {siteKey} bind:token />
				{#if fields.turnstile_token}<p class="text-sm text-destructive" data-testid="field-error-turnstile">{fields.turnstile_token}</p>{/if}
			</div>
		{/if}
		{#if error}<p role="alert" class="pt-2 text-sm text-destructive" data-testid="auth-error">{error}</p>{/if}
		<div class="pt-4">
			<Button type="submit" class="w-full" disabled={busy || heldUntil !== null} data-testid="password-submit">
				{busy ? 'Creating your account…' : 'Create account and continue'}
			</Button>
		</div>
	</form>
{/if}
