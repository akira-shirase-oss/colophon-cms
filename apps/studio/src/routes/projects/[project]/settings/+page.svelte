<!--
  Settings → General (D-081): what the project is called, its URL name, and a link to where its
  content types are built (Content types, in the main menu, D-079). The project id and the public
  API address are for developers, so they wait in Technical details. The other settings are pages
  of their own beside this one (SettingsFrame); the old address of website connections
  (`…/settings#website-connections`) opens their page.
-->
<script lang="ts">
	import CopyIcon from '@lucide/svelte/icons/copy';
	import ShapesIcon from '@lucide/svelte/icons/shapes';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Button } from '@colophon/ui/ui/button';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { deliveryUrl, may } from '#lib/api.ts';
	import { componentsQuery, projectQuery, typesQuery } from '#lib/queries.ts';
	import SettingsFrame from '#lib/SettingsFrame.svelte';
	import FormRow from '#lib/shell/FormRow.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';

	const project = $derived(page.params.project ?? '');
	// The role comes from the project itself: me().projects lists at most 1,000 projects.
	const mine = createQuery(() => projectQuery(project));
	const changesTypes = $derived(may(mine.data, 'schema.change'));
	const types = createQuery(() => typesQuery(project));
	const components = createQuery(() => componentsQuery(project));

	// Links made before Settings had pages of its own pointed at a section of this one.
	onMount(() => {
		if (window.location.hash === '#website-connections') {
			void goto(resolve('/projects/[project]/settings/connections', { project }), { replaceState: true });
		}
	});

	async function copy(text: string) {
		try {
			await navigator.clipboard.writeText(text);
			toast.success('Copied');
		} catch {
			toast.error("Couldn't copy; select it and copy it yourself.");
		}
	}

	const plural = (count: number, one: string) => `${count} ${one}${count === 1 ? '' : 's'}`;
	const typesLine = $derived(
		types.data === undefined
			? 'Their fields, and shared components'
			: [plural(types.data.length, 'content type'), ...(components.data === undefined ? [] : [plural(components.data.length, 'component')])].join(' · ')
	);
	const publicUrl = $derived(`${window.location.origin}${deliveryUrl(project, 'articles')}`);
</script>

<svelte:head><title>Settings · Colophon Studio</title></svelte:head>

<SettingsFrame {project} current="general">
	<PageHeader title="General">
		{#snippet description()}How this project is named, and where the kinds of content it holds are built.{/snippet}
	</PageHeader>

	{#if mine.isError}
		<Problem error={mine.error} what="this project's settings" retry={() => mine.refetch()} />
	{:else if mine.isPending}
		<Skeleton class="mb-4 h-6 w-40" />
		<Skeleton class="h-40 w-full max-w-3xl rounded-xl" />
	{:else if mine.data}
		<section class="max-w-3xl" aria-label="Project">
			<FormRow label="Name">
				<p class="text-sm" data-user-content>{mine.data.name}</p>
			</FormRow>
			<FormRow label="URL name" hint="Used in addresses and the command line.">
				<p class="text-sm">{mine.data.slug}</p>
			</FormRow>
		</section>

		<section class="mt-8 max-w-3xl" aria-labelledby="types-title">
			<h2 id="types-title" class="mb-1 text-base font-semibold">Content types and components</h2>
			<p class="mb-4 text-sm text-muted-foreground">The kinds of content this project holds and their fields are built in Content types, in the main menu.</p>
			<a
				class="flex items-center gap-4 rounded-xl border border-border bg-card px-4 py-3 outline-none hover:bg-accent focus-visible:ring-4 focus-visible:ring-ring/25"
				href={resolve('/projects/[project]/settings/types', { project })}
				data-testid="open-builder"
			>
				<ShapesIcon class="size-5 shrink-0 text-muted-foreground" aria-hidden="true" />
				<span class="min-w-0 flex-1">
					<strong class="font-medium">Content types</strong>
					<span class="block text-xs text-muted-foreground" data-testid="types-count">{typesLine}</span>
				</span>
				<span class="text-sm text-muted-foreground">{changesTypes ? 'Open' : 'View'}</span>
			</a>
		</section>

		<TechnicalDetails class="mt-8 max-w-3xl">
			<div class="grid gap-0.5">
				<span class="font-medium text-foreground">Project id</span>
				<span class="flex min-w-0 items-center gap-2">
					<code class="truncate font-mono" data-testid="project-id">{mine.data.id}</code>
					<Button variant="ghost" size="icon-xs" aria-label="Copy the project id" onclick={() => copy(mine.data?.id ?? '')}><CopyIcon /></Button>
				</span>
			</div>
			<div class="grid gap-0.5 pt-1">
				<span class="font-medium text-foreground">Public API</span>
				<span>Published content, no sign-in needed. Drafts never appear here.</span>
				<span class="flex min-w-0 items-center gap-2">
					<code class="truncate font-mono" data-testid="public-api">{publicUrl}</code>
					<Button variant="ghost" size="icon-xs" aria-label="Copy the public API address" onclick={() => copy(publicUrl)}><CopyIcon /></Button>
				</span>
			</div>
		</TechnicalDetails>
	{/if}
</SettingsFrame>
