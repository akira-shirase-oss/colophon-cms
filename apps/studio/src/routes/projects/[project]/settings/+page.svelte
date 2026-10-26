<!--
  A project's settings: what it is and where its content can be read, and, for owners and
  developers, the destinations every release is announced to (D-046, D-050, D-051): webhooks that
  rebuild sites, CDNs to purge, and probes that check what readers actually get.
-->
<script lang="ts">
	import CopyIcon from '@lucide/svelte/icons/copy';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { ApiError, createDestination, deliveryUrl, type Destination, setDestinationEnabled } from '#lib/api.ts';
	import { destinationsQuery, projectQuery } from '#lib/queries.ts';
	import FormRow from '#lib/shell/FormRow.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';

	const project = $derived(page.params.project ?? '');
	const queryClient = useQueryClient();
	// The role comes from the project itself: me().projects lists at most 1,000 projects.
	const mine = createQuery(() => projectQuery(project));
	const manages = $derived(mine.data?.role === 'owner' || mine.data?.role === 'developer');
	const destinations = createQuery(() => ({ ...destinationsQuery(project), enabled: manages }));

	let kind = $state<Destination['kind']>('webhook');
	let name = $state('');
	let url = $state('');
	let location = $state('');
	let error = $state('');
	let busy = $state(false);

	async function add(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		try {
			await createDestination(project, kind, name, url, kind === 'probe' && location.trim() !== '' ? location.trim() : null);
			toast.success(`Added ${name}`);
			name = '';
			url = '';
			location = '';
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

	async function copy(text: string) {
		try {
			await navigator.clipboard.writeText(text);
			toast.success('Copied');
		} catch {
			toast.error("Couldn't copy; select it and copy it yourself.");
		}
	}

	const KIND = { webhook: 'Webhook', cdn: 'CDN', probe: 'Probe' } as const;
	const ACTION = { webhook: 'Announce releases', cdn: 'Purge on release', probe: 'Check each release' } as const;
	const select =
		'h-9 w-full rounded-md border border-input bg-card px-3 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40';
	const publicUrl = $derived(`${window.location.origin}${deliveryUrl(project, 'articles')}`);
</script>

<svelte:head><title>Settings · Colophon Studio</title></svelte:head>

<PageHeader title="Settings" />

{#if mine.isError}
	<Problem error={mine.error} />
{:else if mine.data}
	<section class="max-w-4xl" aria-labelledby="general">
		<h2 id="general" class="mb-1 text-base font-semibold">General</h2>
		<p class="mb-4 text-sm text-muted-foreground">How the project is named and where anyone can read what it publishes.</p>
		<FormRow label="Name">
			<p class="text-sm">{mine.data.name}</p>
		</FormRow>
		<FormRow label="Slug" hint="Used in the CLI.">
			<p class="text-sm">{mine.data.slug}</p>
		</FormRow>
		<FormRow label="Project id">
			<div class="flex items-center gap-2">
				<code class="truncate font-mono text-sm">{mine.data.id}</code>
				<Button variant="ghost" size="icon-xs" aria-label="Copy the project id" onclick={() => copy(mine.data?.id ?? '')}><CopyIcon /></Button>
			</div>
		</FormRow>
		<FormRow label="Public API" hint="Published content, no sign-in needed. Drafts never appear here.">
			<div class="flex items-center gap-2">
				<code class="truncate font-mono text-sm">{publicUrl}</code>
				<Button variant="ghost" size="icon-xs" aria-label="Copy the public API address" onclick={() => copy(publicUrl)}><CopyIcon /></Button>
			</div>
		</FormRow>
	</section>

	<section class="mt-12 max-w-4xl" aria-label="Destinations">
		<h2 class="mb-1 text-base font-semibold">Destinations</h2>
		<p class="mb-4 text-sm text-muted-foreground">
			Every release is announced to each enabled webhook, once, with a key it can use to drop repeats. Each enabled CDN is asked to purge what
			the release changed, and each probe checks that the website really shows it.
		</p>
		{#if !manages}
			<p class="rounded-lg border border-border bg-card px-4 py-3 text-sm text-muted-foreground" data-testid="destinations-hidden">
				Only owners and developers see destinations: their addresses may carry secrets.
			</p>
		{:else}
			{#if destinations.data}
				<ul class="mb-8 overflow-hidden rounded-xl border border-border bg-card" data-testid="destinations">
					{#each destinations.data as destination (destination.id)}
						<li class="rule-dotted flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3 last:border-b-0">
							<span class="min-w-0 flex-1">
								<span class="flex items-center gap-2">
									<strong class="font-medium">{destination.name}</strong>
									<Badge variant="secondary">{KIND[destination.kind]}{destination.location ? ` at ${destination.location}` : ''}</Badge>
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
					{:else}
						<li class="px-4 py-6 text-center text-sm text-muted-foreground">None yet.</li>
					{/each}
				</ul>
			{/if}
			<form onsubmit={add} aria-label="New destination">
				<h3 class="mb-4 text-sm font-semibold">Add a destination</h3>
				<FormRow label="Kind" for="destination-kind">
					<select id="destination-kind" class="{select} max-w-md" bind:value={kind}>
						<option value="webhook">Webhook: hears about each release</option>
						<option value="cdn">CDN: purges what each release changed</option>
						<option value="probe">Probe: checks the site shows each release</option>
					</select>
				</FormRow>
				{#if kind === 'probe'}
					<FormRow label="Location (optional; only workers there run it)" hint="For example eu-west." for="destination-location">
						<Input id="destination-location" class="max-w-md" bind:value={location} maxlength={63} />
					</FormRow>
				{/if}
				<FormRow label="Destination name" for="destination-name">
					<Input id="destination-name" class="max-w-md" bind:value={name} required maxlength={100} />
				</FormRow>
				<FormRow
					label={kind === 'cdn' ? 'Purge URL (https)' : kind === 'probe' ? 'URL to check (https)' : 'Webhook URL (https)'}
					hint="Public addresses only."
					for="destination-url"
				>
					<Input id="destination-url" class="max-w-md font-mono" bind:value={url} required type="url" />
				</FormRow>
				{#if error}<p role="alert" class="mb-4 text-sm text-destructive">{error}</p>{/if}
				<div class="flex justify-end"><Button type="submit" disabled={busy}>Add destination</Button></div>
			</form>
		{/if}
	</section>
{/if}
