<!--
  The caller's access tokens: mint a named one (shown in full once), list the live ones,
  revoke any. A token signs API calls as the caller — for the CLI and scripts.
-->
<script lang="ts">
	import CopyIcon from '@lucide/svelte/icons/copy';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { createQuery } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { Label } from '@colophon/ui/ui/label';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import * as Table from '@colophon/ui/ui/table';
	import { ApiError, createToken, listTokens, revokeToken, type MintedToken, type TokenInfo } from '#lib/api.ts';
	import { ago } from '#lib/queries.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';

	const tokens = createQuery(() => ({ queryKey: ['tokens'], queryFn: listTokens }));
	let name = $state('');
	let creating = $state(false);
	/** The token just minted — the only time its plaintext is visible. */
	let minted: MintedToken | null = $state(null);
	let confirming: TokenInfo | null = $state(null);

	async function create(event: SubmitEvent) {
		event.preventDefault();
		creating = true;
		try {
			minted = await createToken(name.trim());
			name = '';
			await tokens.refetch();
		} catch (error) {
			toast.error(error instanceof ApiError ? error.message : 'the token could not be created');
		} finally {
			creating = false;
		}
	}

	async function revoke() {
		const token = confirming;
		if (token === null) return;
		confirming = null;
		try {
			await revokeToken(token.id);
			toast.success(`"${token.name}" revoked`);
			await tokens.refetch();
		} catch (error) {
			toast.error(error instanceof ApiError ? error.message : 'the token could not be revoked');
		}
	}

	async function copyToken() {
		if (minted === null) return;
		await navigator.clipboard.writeText(minted.token);
		toast.success('Token copied');
	}
</script>

<PageHeader title="Access tokens">
	{#snippet meta()}
		<span>A token signs API calls as you — for the CLI and scripts.</span>
	{/snippet}
</PageHeader>

{#if minted !== null}
	<div class="mb-6 space-y-3 rounded-lg border border-primary/40 bg-card p-4" data-testid="minted-token">
		<p class="font-medium">"{minted.name}" is ready</p>
		<p class="text-sm text-muted-foreground">Copy it now — this is the only time it's shown.</p>
		<div class="flex items-start gap-2">
			<code class="max-w-2xl flex-1 rounded-md border border-border bg-muted px-3 py-2 font-mono text-sm break-all">{minted.token}</code>
			<Button variant="outline" size="sm" onclick={copyToken}><CopyIcon data-icon="inline-start" />Copy</Button>
		</div>
		<div class="flex items-center gap-3">
			<p class="text-sm text-muted-foreground">Expires {ago(minted.expires_at)}</p>
			<Button variant="ghost" size="sm" onclick={() => (minted = null)}>Done</Button>
		</div>
	</div>
{/if}

<form class="mb-8 flex max-w-xl items-end gap-3" onsubmit={create}>
	<div class="flex-1 space-y-1.5">
		<Label for="token-name">Name</Label>
		<Input id="token-name" placeholder="My laptop, the build server…" bind:value={name} maxlength={100} required />
	</div>
	<Button type="submit" disabled={creating || name.trim() === ''}><PlusIcon data-icon="inline-start" />New token</Button>
</form>

{#if tokens.isPending}
	<div class="space-y-2"><Skeleton class="h-10 w-full" /><Skeleton class="h-10 w-full" /></div>
{:else if tokens.isError}
	<Problem error={tokens.error} />
{:else if tokens.data !== undefined && tokens.data.length === 0}
	<EmptyState icon={KeyRoundIcon} title="No tokens yet">A token signs API calls as you — mint one for the CLI or a script, and it works for thirty days.</EmptyState>
{:else if tokens.data !== undefined}
	<div class="overflow-hidden rounded-lg border border-border">
		<Table.Root>
			<Table.Header>
				<Table.Row>
					<Table.Head>Name</Table.Head>
					<Table.Head>Created</Table.Head>
					<Table.Head>Last used</Table.Head>
					<Table.Head>Expires</Table.Head>
					<Table.Head class="w-24"></Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body>
				{#each tokens.data as token (token.id)}
					<Table.Row>
						<Table.Cell class="font-medium">{token.name}</Table.Cell>
						<Table.Cell>{ago(token.created_at)}</Table.Cell>
						<Table.Cell>{ago(token.last_used_at)}</Table.Cell>
						<Table.Cell>{ago(token.expires_at)}</Table.Cell>
						<Table.Cell>
							{#if confirming?.id === token.id}
								<Button variant="destructive" size="sm" onclick={revoke}>Really?</Button>
							{:else}
								<Button variant="ghost" size="sm" onclick={() => (confirming = token)}><Trash2Icon data-icon="inline-start" />Revoke</Button>
							{/if}
						</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	</div>
{/if}
