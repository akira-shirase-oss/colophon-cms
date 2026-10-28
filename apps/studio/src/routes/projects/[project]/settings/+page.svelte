<!--
  A project's settings, the developer area (D-059, rule 5): what it is and where its content can
  be read, its content types, its roles (D-058), and, for people whose role manages them, its
  website connections (destinations, D-046, D-050, D-051): a site rebuild (webhook), a CDN clear
  (purge), and a site check (probe) that confirms what readers actually get. Each section asks the
  role for its own permission.
-->
<script lang="ts">
	import CopyIcon from '@lucide/svelte/icons/copy';
	import GlobeIcon from '@lucide/svelte/icons/globe';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import {
		ApiError,
		createContentType,
		createDestination,
		createRole,
		deliveryUrl,
		type Destination,
		may,
		newKey,
		setDestinationEnabled
	} from '#lib/api.ts';
	import { slugify } from '#lib/content.ts';
	import { destinationsQuery, projectQuery, rolesQuery, typesQuery } from '#lib/queries.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import FormRow from '#lib/shell/FormRow.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import { CONNECTION_KIND } from '#lib/words.ts';

	const project = $derived(page.params.project ?? '');
	const queryClient = useQueryClient();
	// The role comes from the project itself: me().projects lists at most 1,000 projects.
	const mine = createQuery(() => projectQuery(project));
	const changesTypes = $derived(may(mine.data, 'schema.change'));
	const managesDestinations = $derived(may(mine.data, 'destinations.manage'));
	const managesRoles = $derived(may(mine.data, 'roles.manage'));
	const managesTokens = $derived(may(mine.data, 'tokens.manage'));
	const destinations = createQuery(() => ({ ...destinationsQuery(project), enabled: managesDestinations }));
	const types = createQuery(() => typesQuery(project));
	const roles = createQuery(() => rolesQuery(project));

	// A new role: a name, and the key memberships use, suggested from it. It starts with no
	// permissions; the role's page sets them.
	let roleName = $state('');
	let roleKey = $state('');
	let roleKeyTouched = $state(false);
	let roleError = $state('');
	let roleBusy = $state(false);
	const suggestedKey = $derived(
		slugify(roleName)
			.replace(/^[^a-z]+/, '')
			.slice(0, 63)
	);
	const effectiveKey = $derived(roleKeyTouched ? roleKey : suggestedKey);

	async function addRole(event: SubmitEvent) {
		event.preventDefault();
		roleBusy = true;
		roleError = '';
		try {
			const created = await createRole(project, effectiveKey, { name: roleName.trim(), description: '', permissions: [] });
			await queryClient.invalidateQueries({ queryKey: ['roles', project] });
			toast.success(`Created ${created.name}`);
			await goto(resolve('/projects/[project]/settings/roles/[role]', { project, role: created.key }));
		} catch (failure) {
			roleError = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			roleBusy = false;
		}
	}

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

	const ACTION = { webhook: 'Announce releases', cdn: 'Purge on release', probe: 'Check each release' } as const;
	const select =
		'h-9 w-full rounded-md border border-input bg-card px-3 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40';
	const publicUrl = $derived(`${window.location.origin}${deliveryUrl(project, 'articles')}`);
</script>

<svelte:head><title>Settings · Colophon Studio</title></svelte:head>

<PageHeader title="Settings" />

{#if mine.isError}
	<Problem error={mine.error} what="this project's settings" retry={() => mine.refetch()} />
{:else if mine.isPending}
	<Skeleton class="mb-4 h-6 w-40" />
	<Skeleton class="h-40 w-full max-w-4xl rounded-xl" />
{:else if mine.data}
	<section class="max-w-4xl" aria-labelledby="general">
		<h2 id="general" class="mb-1 text-base font-semibold">General</h2>
		<p class="mb-4 text-sm text-muted-foreground">How the project is named and where anyone can read what it publishes.</p>
		<FormRow label="Name">
			<p class="text-sm">{mine.data.name}</p>
		</FormRow>
		<FormRow label="URL name" hint="Used in addresses and the command line.">
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
		{#if types.isError}
			<div class="mb-6"><Problem error={types.error} what="content types" retry={() => types.refetch()} /></div>
		{:else if types.isPending}
			<Skeleton class="mb-6 h-24 w-full rounded-xl" />
		{:else if types.data}
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
							<span class="text-sm text-muted-foreground">{may(mine.data, 'schema.change', item.slug) ? 'Edit' : 'View'}</span>
						</a>
					</li>
				{/each}
			</ul>
		{/if}
		{#if changesTypes}
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

	<section class="mt-12 max-w-4xl" aria-labelledby="roles-title">
		<h2 id="roles-title" class="mb-1 text-base font-semibold">Roles</h2>
		<p class="mb-4 text-sm text-muted-foreground">
			What each person in the project may do. Owners can always do everything; other roles can be changed, and you can make your own.
		</p>
		{#if roles.isError}
			<Problem error={roles.error} />
		{:else if roles.data}
			<ul class="mb-6 overflow-hidden rounded-xl border border-border bg-card" data-testid="roles">
				{#each roles.data as item (item.key)}
					<li class="rule-dotted last:border-b-0">
						<a class="flex items-center gap-4 px-4 py-3 hover:bg-accent" href={resolve('/projects/[project]/settings/roles/[role]', { project, role: item.key })}>
							<span class="min-w-0 flex-1">
								<span class="flex items-center gap-2">
									<strong class="font-medium">{item.name}</strong>
									{#if item.built_in}<Badge variant="secondary">Built in</Badge>{/if}
								</span>
								{#if item.description}<span class="block truncate text-xs text-muted-foreground">{item.description}</span>{/if}
							</span>
							<span class="text-sm whitespace-nowrap text-muted-foreground">{item.members} {item.members === 1 ? 'person' : 'people'}</span>
							<span class="text-sm text-muted-foreground">{managesRoles && !item.fixed ? 'Edit' : 'View'}</span>
						</a>
					</li>
				{/each}
			</ul>
		{/if}
		{#if managesRoles}
			<form onsubmit={addRole} aria-label="New role">
				<h3 class="mb-4 text-sm font-semibold">Make a role</h3>
				<FormRow label="Name" hint="For example Copy desk, Translator or Guest writer." for="role-name">
					<Input id="role-name" class="max-w-md" bind:value={roleName} required maxlength={100} />
				</FormRow>
				<FormRow label="Short name" hint="Lowercase letters, digits and hyphens; used by the API. It can't change later." for="role-key">
					<Input
						id="role-key"
						class="max-w-md font-mono"
						value={effectiveKey}
						oninput={(event) => {
							roleKeyTouched = true;
							roleKey = event.currentTarget.value;
						}}
						required
						maxlength={63}
					/>
				</FormRow>
				{#if roleError}<p role="alert" class="mb-4 text-sm text-destructive">{roleError}</p>{/if}
				<div class="flex justify-end">
					<Button type="submit" disabled={roleBusy || roleName.trim() === '' || effectiveKey === ''}>Create and choose what it may do</Button>
				</div>
			</form>
		{/if}
	</section>

	{#if managesTokens}
		<section class="mt-12 max-w-4xl" aria-labelledby="access-title">
			<h2 id="access-title" class="mb-1 text-base font-semibold">Access</h2>
			<p class="mb-4 text-sm text-muted-foreground">Keys that let scripts and sites use this project's content.</p>
			<ul class="overflow-hidden rounded-xl border border-border bg-card" data-testid="access-links">
				{#if managesTokens}
					<li class="rule-dotted last:border-b-0">
						<a class="flex items-center gap-4 px-4 py-3 hover:bg-accent" href={resolve('/projects/[project]/settings/tokens', { project })} data-testid="tokens-link">
							<span class="min-w-0 flex-1">
								<strong class="font-medium">API tokens</strong>
								<span class="block text-xs text-muted-foreground">Let a script or your site's build use this project's content, each with its own powers and end date.</span>
							</span>
							<span class="text-sm text-muted-foreground">Open</span>
						</a>
					</li>
				{/if}
			</ul>
		</section>
	{/if}

	<section class="mt-12 max-w-4xl scroll-mt-20" aria-label="Website connections" id="website-connections">
		<h2 class="mb-1 text-base font-semibold">Website connections</h2>
		<p class="mb-4 text-sm text-muted-foreground">
			What happens to your website after each release. A site rebuild tells your site a release is out (a webhook, sent once, with a key it can use to
			drop repeats); a CDN clear asks your CDN to drop the copies the release changed; a site check reads the live site to confirm readers see it.
		</p>
		{#if !managesDestinations}
			<p class="rounded-lg border border-border bg-card px-4 py-3 text-sm text-muted-foreground" data-testid="destinations-hidden">
				Your role doesn't manage website connections, so they stay hidden: their addresses may carry secrets.
			</p>
		{:else}
			{#if destinations.data}
				<ul class="mb-8 overflow-hidden rounded-xl border border-border bg-card" class:hidden={destinations.data.length === 0} data-testid="destinations">
					{#each destinations.data as destination (destination.id)}
						<li class="rule-dotted flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3 last:border-b-0">
							<span class="min-w-0 flex-1">
								<span class="flex items-center gap-2">
									<strong class="font-medium">{destination.name}</strong>
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
				{#if destinations.data.length === 0}
					<div class="mb-8">
						<EmptyState icon={GlobeIcon} title="No website connections yet" testid="connections-empty">
							Connect your website so each release rebuilds it, clears its CDN, or is checked on the live site. Until then, releases are published in
							Colophon and your site reads them whenever it asks.
							{#snippet action()}
								<Button variant="outline" onclick={() => document.getElementById('destination-name')?.focus()}>Add a website connection</Button>
							{/snippet}
						</EmptyState>
					</div>
				{/if}
			{/if}
			<form onsubmit={add} aria-label="New destination">
				<h3 class="mb-4 text-sm font-semibold">Add a website connection</h3>
				<FormRow label="Kind" hint={CONNECTION_KIND[kind].detail} for="destination-kind">
					<select id="destination-kind" class="{select} max-w-md" bind:value={kind}>
						<option value="webhook">{CONNECTION_KIND.webhook.name} (webhook)</option>
						<option value="cdn">{CONNECTION_KIND.cdn.name} (purge)</option>
						<option value="probe">{CONNECTION_KIND.probe.name} (probe)</option>
					</select>
				</FormRow>
				{#if kind === 'probe'}
					<FormRow label="Location (optional; only workers there run it)" hint="For example eu-west." for="destination-location">
						<Input id="destination-location" class="max-w-md" bind:value={location} maxlength={63} />
					</FormRow>
				{/if}
				<FormRow label="Connection name" for="destination-name">
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
				<div class="flex justify-end"><Button type="submit" disabled={busy}>Add website connection</Button></div>
			</form>
		{/if}
	</section>
{/if}
