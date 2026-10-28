<!--
  Settings → Website connections (destinations: D-046, D-050, D-051; D-081): what happens to the
  website after each release. A site rebuild (webhook), a CDN clear (purge) and a site check
  (probe) that confirms what readers actually get. Only roles that manage them see them, since
  their addresses may carry secrets. Add a connection opens a dialog: the kind as three cards, its
  name and address; each connection is turned on or off in the list.
-->
<script lang="ts">
	import GlobeIcon from '@lucide/svelte/icons/globe';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Input } from '@colophon/ui/ui/input';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { ApiError, createDestination, type Destination, may, setDestinationEnabled } from '#lib/api.ts';
	import { destinationsQuery, projectQuery } from '#lib/queries.ts';
	import SettingsFrame from '#lib/SettingsFrame.svelte';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import { CONNECTION_KIND } from '#lib/words.ts';

	const project = $derived(page.params.project ?? '');
	const queryClient = useQueryClient();
	const mine = createQuery(() => projectQuery(project));
	const manages = $derived(may(mine.data, 'destinations.manage'));
	const destinations = createQuery(() => ({ ...destinationsQuery(project), enabled: manages }));
	const any = $derived((destinations.data ?? []).length > 0);

	const KINDS: Destination['kind'][] = ['webhook', 'cdn', 'probe'];
	const ACTION = { webhook: 'Announce releases', cdn: 'Purge on release', probe: 'Check each release' } as const;
	const URL_LABEL = { webhook: 'Webhook URL (https)', cdn: 'Purge URL (https)', probe: 'URL to check (https)' } as const;

	let adding = $state(false);
	let kind = $state<Destination['kind']>('webhook');
	let name = $state('');
	let url = $state('');
	let location = $state('');
	let error = $state('');
	let busy = $state(false);

	function start() {
		adding = true;
		kind = 'webhook';
		name = '';
		url = '';
		location = '';
		error = '';
	}

	async function add(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		try {
			await createDestination(project, kind, name, url, kind === 'probe' && location.trim() !== '' ? location.trim() : null);
			toast.success(`Added ${name}`);
			adding = false;
			await queryClient.invalidateQueries({ queryKey: ['destinations', project] });
		} catch (failure) {
			error = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			busy = false;
		}
	}

	async function toggle(id: string, enabled: boolean) {
		try {
			await setDestinationEnabled(project, id, enabled);
		} catch (failure) {
			toast.error(failure instanceof ApiError ? failure.message : String(failure));
		}
		await queryClient.invalidateQueries({ queryKey: ['destinations', project] });
	}
</script>

<svelte:head><title>Website connections · Colophon Studio</title></svelte:head>

<SettingsFrame {project} current="connections">
	<PageHeader title="Website connections">
		{#snippet description()}
			What happens to your website after each release.
			{#if manages}
				A site rebuild tells your site a release is out (a webhook, sent once, with a key it can use to drop repeats); a cache clear asks the cache in front of your site (a CDN) to drop the
				copies the release changed; a site check reads the live site to confirm readers see it.
			{/if}
		{/snippet}
		{#snippet actions()}
			{#if manages && any}
				<Button onclick={start} data-testid="add-connection"><PlusIcon data-icon="inline-start" />Add a connection</Button>
			{/if}
		{/snippet}
	</PageHeader>

	{#if mine.isError}
		<Problem error={mine.error} what="this project" retry={() => mine.refetch()} />
	{:else if mine.isPending}
		<Skeleton class="h-32 w-full max-w-3xl rounded-xl" />
	{:else if !manages}
		<p class="max-w-3xl rounded-lg border border-border bg-card px-4 py-3 text-sm text-muted-foreground" data-testid="destinations-hidden">
			Your role doesn't manage website connections, so they stay hidden: their addresses may carry secrets.
		</p>
	{:else if destinations.isError}
		<Problem error={destinations.error} what="website connections" retry={() => destinations.refetch()} />
	{:else if destinations.isPending}
		<Skeleton class="h-32 w-full max-w-3xl rounded-xl" />
	{:else if !any}
		<div class="max-w-3xl">
			<EmptyState icon={GlobeIcon} title="No website connections yet" testid="connections-empty">
				Connect your website so each release rebuilds it, clears its cache, or is checked on the live site. Until then, releases are published in Colophon and
				your site reads them whenever it asks.
				{#snippet action()}
					<Button onclick={start} data-testid="add-connection"><PlusIcon data-icon="inline-start" />Add a connection</Button>
				{/snippet}
			</EmptyState>
		</div>
	{:else}
		<ul class="max-w-3xl overflow-hidden rounded-xl border border-border bg-card" data-testid="destinations">
			{#each destinations.data as destination (destination.id)}
				<li class="rule-dotted flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3 last:border-b-0">
					<span class="min-w-0 flex-1">
						<span class="flex flex-wrap items-center gap-2">
							<strong class="font-medium" data-user-content>{destination.name}</strong>
							<Badge variant="secondary">{CONNECTION_KIND[destination.kind].name}{destination.location ? ` at ${destination.location}` : ''}</Badge>
						</span>
						<code class="block truncate font-mono text-xs text-muted-foreground">{destination.url}</code>
					</span>
					<label class="flex items-center gap-2 text-sm">
						<input
							type="checkbox"
							class="size-4 accent-primary"
							checked={destination.enabled}
							onchange={(event) => toggle(destination.id, event.currentTarget.checked)}
						/>
						{ACTION[destination.kind]}
					</label>
				</li>
			{/each}
		</ul>
	{/if}
</SettingsFrame>

<Dialog.Root bind:open={adding}>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-lg">
		<form class="grid gap-5" onsubmit={add} aria-label="New website connection">
			<Dialog.Header>
				<Dialog.Title>Add a website connection</Dialog.Title>
				<Dialog.Description>Choose what Colophon does after each release, then where. Only public https addresses work.</Dialog.Description>
			</Dialog.Header>
			<fieldset class="grid gap-2 text-sm">
				<legend class="mb-1.5 font-medium">Kind</legend>
				{#each KINDS as option (option)}
					<label class="flex items-start gap-2.5 rounded-xl border border-border bg-card px-3 py-2.5 has-checked:border-ring">
						<input type="radio" class="mt-0.5 size-4 accent-primary" name="connection-kind" value={option} bind:group={kind} />
						<span>
							<span class="font-medium">{CONNECTION_KIND[option].name}</span>
							<span class="block text-xs text-muted-foreground">{CONNECTION_KIND[option].detail}</span>
						</span>
					</label>
				{/each}
			</fieldset>
			<div class="grid gap-1.5 text-sm">
				<label class="font-medium" for="destination-name">Connection name</label>
				<!-- svelte-ignore a11y_autofocus -->
				<Input id="destination-name" bind:value={name} required maxlength={100} autofocus aria-describedby="destination-name-hint" />
				<span id="destination-name-hint" class="text-xs text-muted-foreground">So you know it in the list and on each release’s receipt: “Site build”, “Edge cache”.</span>
			</div>
			<div class="grid gap-1.5 text-sm">
				<label class="font-medium" for="destination-url">{URL_LABEL[kind]}</label>
				<Input id="destination-url" class="font-mono" bind:value={url} required type="url" aria-describedby="destination-url-hint" />
				<span id="destination-url-hint" class="text-xs text-muted-foreground">Public addresses only.</span>
			</div>
			{#if kind === 'probe'}
				<div class="grid gap-1.5 text-sm">
					<label class="font-medium" for="destination-location">Location (optional; only workers there run it)</label>
					<Input id="destination-location" bind:value={location} maxlength={63} aria-describedby="destination-location-hint" />
					<span id="destination-location-hint" class="text-xs text-muted-foreground">For example eu-west.</span>
				</div>
			{/if}
			{#if error}<p role="alert" class="text-sm text-destructive" data-testid="connection-error">{error}</p>{/if}
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (adding = false)}>Cancel</Button>
				<Button type="submit" disabled={busy}>{busy ? 'Adding…' : 'Add website connection'}</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
