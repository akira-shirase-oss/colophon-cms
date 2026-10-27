<!--
  A project's settings: what it is and where its content can be read, and, for owners and
  developers, the destinations every release is announced to (D-046, D-050, D-051): webhooks that
  rebuild sites, CDNs to purge, and probes that check what readers actually get.
-->
<script lang="ts">
	import CopyIcon from '@lucide/svelte/icons/copy';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import {
		ApiError,
		createContentType,
		createDestination,
		deliveryUrl,
		type Destination,
		newKey,
		setDestinationEnabled
	} from '#lib/api.ts';
	import { slugify } from '#lib/content.ts';
	import { destinationsQuery, projectQuery, typesQuery } from '#lib/queries.ts';
	import FormRow from '#lib/shell/FormRow.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';

	const project = $derived(page.params.project ?? '');
	const queryClient = useQueryClient();
	// The role comes from the project itself: me().projects lists at most 1,000 projects.
	const mine = createQuery(() => projectQuery(project));
	const manages = $derived(mine.data?.role === 'owner' || mine.data?.role === 'developer');
	const destinations = createQuery(() => ({ ...destinationsQuery(project), enabled: manages }));
	const types = createQuery(() => typesQuery(project));

	// A new content type: a name, the URL name its entries live under, and a title field to start.
	let typeName = $state('');
	let typeSlug = $state('');
	let typeSlugTouched = $state(false);
	let typeError = $state('');
	let typeBusy = $state(false);
	let typeKey = newKey();
	const suggestedSlug = $derived(slugify(typeName).slice(0, 63).replace(/-+$/, ''));
	const effectiveSlug = $derived(typeSlugTouched ? typeSlug : suggestedSlug);

	async function addType(event: SubmitEvent) {
		event.preventDefault();
		typeBusy = true;
		typeError = '';
		try {
			const created = await createContentType(
				project,
				effectiveSlug,
				{
					name: typeName.trim(),
					title_field: 'title',
					fields: [{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false }]
				},
				typeKey
			);
			typeKey = newKey();
			await queryClient.invalidateQueries({ queryKey: ['types', project] });
			toast.success(`Created ${created.type.definition.name}`);
			await goto(resolve('/projects/[project]/settings/types/[type]', { project, type: created.type.slug }));
		} catch (failure) {
			typeError = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			typeBusy = false;
		}
	}

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

	<section class="mt-12 max-w-4xl" aria-labelledby="types-title">
		<h2 id="types-title" class="mb-1 text-base font-semibold">Content types</h2>
		<p class="mb-4 text-sm text-muted-foreground">
			The kinds of content this project holds and their fields. Changes that could break saved entries are checked against them first.
		</p>
		{#if types.data}
			<ul class="mb-6 overflow-hidden rounded-xl border border-border bg-card" data-testid="content-types">
				{#each types.data as item (item.slug)}
					<li class="rule-dotted last:border-b-0">
						<a
							class="flex items-center gap-4 px-4 py-3 hover:bg-accent"
							href={resolve('/projects/[project]/settings/types/[type]', { project, type: item.slug })}
						>
							<span class="min-w-0 flex-1">
								<strong class="font-medium">{item.definition.name}</strong>
								<span class="block text-xs text-muted-foreground">
									{item.definition.fields.length} field{item.definition.fields.length === 1 ? '' : 's'} · version {item.version}
								</span>
							</span>
							<span class="text-sm text-muted-foreground">{manages ? 'Edit' : 'View'}</span>
						</a>
					</li>
				{/each}
			</ul>
		{/if}
		{#if manages}
			<form onsubmit={addType} aria-label="New content type">
				<h3 class="mb-4 text-sm font-semibold">Add a content type</h3>
				<FormRow label="Name" hint="For example Pages, Authors or Recipes." for="type-name">
					<Input id="type-name" class="max-w-md" bind:value={typeName} required maxlength={100} />
				</FormRow>
				<FormRow label="URL name" hint="Where its entries live in addresses. It can't change later." for="type-slug">
					<Input
						id="type-slug"
						class="max-w-md font-mono"
						value={effectiveSlug}
						oninput={(event) => {
							typeSlugTouched = true;
							typeSlug = event.currentTarget.value;
						}}
						required
						maxlength={63}
					/>
				</FormRow>
				{#if typeError}<p role="alert" class="mb-4 text-sm text-destructive">{typeError}</p>{/if}
				<div class="flex justify-end">
					<Button type="submit" disabled={typeBusy || typeName.trim() === '' || effectiveSlug === ''}>Create and add fields</Button>
				</div>
			</form>
		{/if}
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
