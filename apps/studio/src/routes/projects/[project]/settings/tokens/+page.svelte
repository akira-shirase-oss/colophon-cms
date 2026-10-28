<!--
  A project's API tokens (step R.4): keys a script, a site's build or another tool uses instead of
  someone's sign-in. Each has a name, what it may do (read only, full access, or chosen in the same
  grid as a role, without the four powers only people hold), and an end date. Its secret is shown
  once, in the answer that made it; the server keeps only a hash. Revoking asks first, because it
  can't be undone. Only roles that manage tokens see this page's contents (D-059, rule 5).
-->
<script lang="ts">
	import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import * as AlertDialog from '@colophon/ui/ui/alert-dialog';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { Textarea } from '@colophon/ui/ui/textarea';
	import { type Action, type ApiToken, createToken, type Grant, listTokens, may, PEOPLE_ONLY, revokeToken } from '#lib/api.ts';
	import { plainError } from '#lib/errors.ts';
	import PermissionGrid from '#lib/PermissionGrid.svelte';
	import { grantWords, POWERS } from '#lib/permissions.ts';
	import { ago, projectQuery, typesQuery } from '#lib/queries.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import FormRow from '#lib/shell/FormRow.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';

	const project = $derived(page.params.project ?? '');
	const queryClient = useQueryClient();
	const mine = createQuery(() => projectQuery(project));
	const manages = $derived(may(mine.data, 'tokens.manage'));
	const types = createQuery(() => typesQuery(project));
	const tokens = createQuery(() => ({ queryKey: ['tokens', project], queryFn: () => listTokens(project), enabled: manages }));
	const active = $derived((tokens.data ?? []).filter((token) => token.state === 'active'));
	const ended = $derived((tokens.data ?? []).filter((token) => token.state !== 'active'));

	const KINDS = [
		{ kind: 'read-only', name: 'Read only', detail: 'Sees every entry, drafts included, and changes nothing.' },
		{
			kind: 'full-access',
			name: 'Full access',
			detail: 'Sees, writes, approves, publishes and deletes content, and changes content types and website connections. Your role needs all of that too.'
		},
		{ kind: 'custom', name: 'Custom', detail: 'Exactly what you tick below, and never more than your role may do.' }
	] as const;
	const LASTS = [
		{ days: 7, name: '7 days' },
		{ days: 30, name: '30 days' },
		{ days: 90, name: '90 days' },
		{ days: 180, name: '6 months' },
		{ days: 365, name: 'A year' }
	];
	// What a custom token may be given: everything a role may hold except what only people hold.
	const tokenPowers: Action[] = POWERS.map((power) => power.action).filter((action) => !PEOPLE_ONLY.includes(action));

	let name = $state('');
	let description = $state('');
	let kind = $state<ApiToken['kind']>('read-only');
	let days = $state(90);
	let grants = $state(new Set<string>());
	let busy = $state(false);
	let error = $state('');
	/** The token just made, with its secret: shown until dismissed, and never again. */
	let made = $state<ApiToken | null>(null);
	let copied = $state(false);

	const permissions = $derived(
		[...grants].map((grant): Grant => {
			const [action, subject] = grant.split('|') as [Action, string];
			return { action, subject };
		})
	);

	async function create(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		made = null;
		copied = false;
		try {
			made = await createToken(project, {
				name: name.trim(),
				...(description.trim() === '' ? {} : { description: description.trim() }),
				kind,
				...(kind === 'custom' ? { permissions } : {}),
				expires_in_days: days
			});
			name = '';
			description = '';
			grants = new Set();
			await queryClient.invalidateQueries({ queryKey: ['tokens', project] });
		} catch (failure) {
			error = plainError(failure, 'make API tokens');
		} finally {
			busy = false;
		}
	}

	async function copy() {
		if (made?.secret === undefined) return;
		try {
			await navigator.clipboard.writeText(made.secret);
			copied = true;
		} catch {
			toast.error("Couldn't copy; select the token and copy it yourself.");
		}
	}

	// Revoking can't be undone (D-059, rule 6): it asks first.
	let revoking = $state<ApiToken | null>(null);
	let confirmOpen = $state(false);
	async function revoke() {
		const token = revoking;
		if (token === null) return;
		try {
			await revokeToken(project, token.id);
			toast.success(`Revoked ${token.name}`);
		} catch (failure) {
			toast.error(`Not revoked. ${plainError(failure, 'revoke API tokens')}`);
		} finally {
			confirmOpen = false;
			revoking = null;
			await queryClient.invalidateQueries({ queryKey: ['tokens', project] });
		}
	}

	const date = (at: string) => new Date(at).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
	function what(token: ApiToken): string {
		if (token.kind === 'read-only') return 'Read only';
		if (token.kind === 'full-access') return 'Full access';
		return `Custom: ${token.permissions.map((grant) => grantWords(grant, types.data ?? [])).join(', ')}`;
	}
	function when(token: ApiToken): string {
		if (token.state === 'revoked' && token.revoked_at) return `Revoked ${date(token.revoked_at)}`;
		if (token.state === 'expired') return `Ended ${date(token.expires_at)}`;
		return `Works until ${date(token.expires_at)}`;
	}
</script>

<svelte:head><title>API tokens · Colophon Studio</title></svelte:head>

<PageHeader title="API tokens">
	{#snippet eyebrow()}
		<Button variant="ghost" size="xs" href={resolve('/projects/[project]/settings', { project })}><ArrowLeftIcon data-icon="inline-start" />Settings</Button>
	{/snippet}
	{#snippet description()}
		A token lets a script, your site's build or another tool use this project's content without anyone's password. Each has its own powers and an end
		date, and can be revoked at any time.
	{/snippet}
</PageHeader>

{#if mine.isError}
	<Problem error={mine.error} what="this project" retry={() => mine.refetch()} />
{:else if mine.isPending}
	<Skeleton class="h-40 w-full max-w-4xl rounded-xl" />
{:else if !manages}
	<p class="max-w-4xl rounded-lg border border-border bg-card px-4 py-3 text-sm text-muted-foreground" data-testid="tokens-hidden">
		Your role doesn't manage API tokens, so they stay hidden. An owner can make one for you.
	</p>
{:else}
	{#if made?.secret}
		<section class="mb-10 max-w-4xl rounded-xl border border-live/40 bg-live-soft/50 p-5" aria-labelledby="secret-heading" data-testid="token-secret">
			<h2 id="secret-heading" class="font-semibold">Copy “{made.name}” now</h2>
			<p class="mt-1 text-sm">
				<strong>You won't see it again.</strong> Colophon keeps only a fingerprint of it, so a lost token can't be shown again: revoke it and make a new
				one. Keep it somewhere safe, such as your site's settings or a password manager.
			</p>
			<div class="mt-3 flex flex-col gap-2 sm:flex-row">
				<Input readonly value={made.secret} aria-label="The new token" class="font-mono text-xs" onfocus={(event) => event.currentTarget.select()} data-testid="token-secret-value" />
				<Button variant="secondary" onclick={copy}><CopyIcon data-icon="inline-start" />{copied ? 'Copied' : 'Copy'}</Button>
			</div>
			<div class="mt-3 flex flex-wrap items-center justify-between gap-2">
				<TechnicalDetails>
					<p>Send it with each request to this project's content: <span class="font-mono">Authorization: Bearer &lt;token&gt;</span>.</p>
					<p>It works only on this project's content routes, never on people, roles, tokens or the activity log.</p>
				</TechnicalDetails>
				<Button variant="ghost" size="sm" onclick={() => (made = null)}>I've stored it</Button>
			</div>
		</section>
	{/if}

	<section class="max-w-4xl" aria-labelledby="tokens-heading">
		<h2 id="tokens-heading" class="mb-3 text-base font-semibold">Tokens</h2>
		{#if tokens.isPending}
			<Skeleton class="h-24 w-full rounded-xl" />
		{:else if tokens.isError}
			<Problem error={tokens.error} what="API tokens" retry={() => tokens.refetch()} />
		{:else if (tokens.data ?? []).length === 0}
			<EmptyState icon={KeyRoundIcon} title="No API tokens yet" testid="tokens-empty">
				Make one below for each script or site that reads or writes this project's content, so each can be revoked on its own.
			</EmptyState>
		{:else}
			<ul class="overflow-hidden rounded-xl border border-border bg-card" data-testid="tokens">
				{#each [...active, ...ended] as token (token.id)}
					<li class="rule-dotted flex flex-wrap items-start gap-x-4 gap-y-2 px-4 py-3 last:border-b-0" data-state={token.state} data-testid="token">
						<span class="min-w-0 flex-1">
							<span class="flex flex-wrap items-center gap-2">
								<strong class="font-medium" data-user-content>{token.name}</strong>
								<Badge variant={token.state === 'active' ? 'live' : 'draft'} data-testid="token-state">
									{token.state === 'active' ? 'Active' : token.state === 'expired' ? 'Ended' : 'Revoked'}
								</Badge>
							</span>
							{#if token.description}<span class="block text-sm text-muted-foreground" data-user-content>{token.description}</span>{/if}
							<span class="block text-sm">{what(token)}</span>
							<span class="block text-xs text-muted-foreground">
								<span data-testid="token-used">{token.last_used_at ? `Last used ${ago(token.last_used_at)}` : 'Never used'}</span> ·
								{when(token)} · Made by {token.created_by.name ?? 'someone'}, {ago(token.created_at)}
							</span>
						</span>
						{#if token.state === 'active'}
							<Button
								variant="outline"
								size="sm"
								onclick={() => {
									revoking = token;
									confirmOpen = true;
								}}
								aria-label="Revoke {token.name}">Revoke</Button
							>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</section>

	<section class="mt-12 max-w-4xl" aria-labelledby="new-token-heading">
		<h2 id="new-token-heading" class="mb-4 text-base font-semibold">Make a token</h2>
		<form onsubmit={create} aria-label="Make a token">
			<FormRow label="Name" hint="What uses it, so you know which to revoke: “Site build”, “Import script”." for="token-name">
				<Input id="token-name" class="max-w-md" bind:value={name} required maxlength={100} />
			</FormRow>
			<FormRow label="Description (optional)" for="token-description">
				<Textarea id="token-description" class="max-w-md" bind:value={description} rows={2} maxlength={500} />
			</FormRow>
			<FormRow label="What it may do">
				<fieldset class="grid gap-2">
					<legend class="sr-only">What it may do</legend>
					{#each KINDS as option (option.kind)}
						<label class="flex items-start gap-3 rounded-lg border border-border bg-card px-3 py-2.5 has-[:checked]:border-foreground/40">
							<input type="radio" name="token-kind" class="mt-1 size-4 accent-primary" value={option.kind} bind:group={kind} />
							<span>
								<span class="block text-sm font-medium">{option.name}</span>
								<span class="block text-xs text-muted-foreground">{option.detail}</span>
							</span>
						</label>
					{/each}
				</fieldset>
			</FormRow>
			{#if kind === 'custom'}
				<div class="mb-6" data-testid="token-grid">
					<PermissionGrid types={types.data ?? []} bind:grants editable powers={tokenPowers} label="What the token may do" />
				</div>
			{/if}
			<FormRow label="Works for" hint="It stops working by itself after this; you can revoke it sooner." for="token-days">
				<select
					id="token-days"
					class="h-9 w-full max-w-56 rounded-md border border-input bg-card px-3 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40"
					bind:value={days}
				>
					{#each LASTS as last (last.days)}<option value={last.days}>{last.name}</option>{/each}
				</select>
			</FormRow>
			{#if error}<p role="alert" class="mb-4 text-sm text-destructive" data-testid="token-error">{error}</p>{/if}
			<div class="flex justify-end">
				<Button type="submit" disabled={busy || name.trim() === '' || (kind === 'custom' && grants.size === 0)}>{busy ? 'Making it…' : 'Make the token'}</Button>
			</div>
		</form>
	</section>
{/if}

<AlertDialog.Root bind:open={confirmOpen}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Revoke “{revoking?.name}”?</AlertDialog.Title>
			<AlertDialog.Description>
				Anything using it stops working at once, and this can't be undone. To keep that tool working, make a new token and give it that one first.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Keep it</AlertDialog.Cancel>
			<AlertDialog.Action variant="destructive" onclick={revoke}>Revoke it</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
