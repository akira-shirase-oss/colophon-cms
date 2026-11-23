<!--
  Connected apps (D-246, D-248): every app connected to Colophon over OAuth as this person, in all
  their projects (or an older connection's one project), since when and when last used, each with
  Disconnect (its tokens stop working at once).
-->
<script lang="ts">
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { connectedApps, disconnectApp } from '#lib/api.ts';
	import { plainError } from '#lib/errors.ts';

	const queryClient = useQueryClient();
	const apps = createQuery(() => ({ queryKey: ['connected-apps'], queryFn: connectedApps }));
	let busy = $state('');
	let error = $state('');

	const when = (at: string | null) => (at === null ? 'not yet' : new Date(at).toLocaleString());

	async function disconnect(id: string) {
		busy = id;
		error = '';
		try {
			await disconnectApp(id);
			await queryClient.invalidateQueries({ queryKey: ['connected-apps'] });
		} catch (failure) {
			error = plainError(failure, 'disconnect this app');
		} finally {
			busy = '';
		}
	}
</script>

<svelte:head><title>Connected apps · Colophon Studio</title></svelte:head>

<div class="mx-auto max-w-2xl space-y-4">
	<h1 class="page-title">Connected apps</h1>
	<p class="text-sm text-muted-foreground">
		AI apps you connected to Colophon. Each acts as you, with your role in each of your projects. Disconnect one and it stops working at once.
	</p>
	{#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
	{#if apps.isPending}
		<p class="text-sm text-muted-foreground">Loading…</p>
	{:else if apps.isError}
		<p role="alert">{plainError(apps.error, 'list your connected apps')}</p>
	{:else if apps.data.length === 0}
		<p data-testid="no-connected-apps">No apps are connected. To connect one, add this server's /mcp address to the app as a custom connector.</p>
	{:else}
		<ul class="divide-y divide-border rounded-2xl border border-border bg-card" data-testid="connected-apps">
			{#each apps.data as app (app.id)}
				<li class="flex flex-wrap items-center justify-between gap-3 p-4">
					<div class="min-w-0 space-y-0.5">
						<p class="font-medium">{app.client_name} <span class="text-sm font-normal text-muted-foreground">(unverified name)</span></p>
						<p class="text-sm text-muted-foreground">{app.project_name === null ? 'All your projects' : `In ${app.project_name}`} · connected {when(app.created_at)} · last used {when(app.last_used_at)}</p>
					</div>
					<Button variant="outline" onclick={() => disconnect(app.id)} disabled={busy === app.id} data-testid="disconnect-app">
						{busy === app.id ? 'Disconnecting…' : 'Disconnect'}
					</Button>
				</li>
			{/each}
		</ul>
	{/if}
</div>
