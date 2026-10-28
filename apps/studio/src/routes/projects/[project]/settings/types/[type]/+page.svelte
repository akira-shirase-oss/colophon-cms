<!--
  One content type in the builder (D-054, D-055): its fields, what can change, and, when the
  server refuses a change, the entries in the way. Owners and developers edit; everyone else
  reads.
-->
<script lang="ts">
	import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { getContentType } from '#lib/api.ts';
	import { projectQuery } from '#lib/queries.ts';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import TypeBuilder from '#lib/TypeBuilder.svelte';

	const project = $derived(page.params.project ?? '');
	const slug = $derived(page.params.type ?? '');
	const mine = createQuery(() => projectQuery(project));
	const editable = $derived(mine.data?.role === 'owner' || mine.data?.role === 'developer');
	const type = createQuery(() => ({ queryKey: ['type', project, slug], queryFn: () => getContentType(project, slug) }));
</script>

<svelte:head><title>{type.data?.definition.name ?? 'Content type'} · Colophon Studio</title></svelte:head>

<PageHeader title={type.data?.definition.name ?? 'Content type'}>
	{#snippet eyebrow()}
		<Button variant="ghost" size="xs" href={resolve('/projects/[project]/settings', { project })}>
			<ArrowLeftIcon data-icon="inline-start" />Settings
		</Button>
	{/snippet}
	{#snippet meta()}
		{#if type.data}
			<span>Version {type.data.version}</span>
			<span>In addresses as <code class="font-mono">{type.data.slug}</code></span>
			{#if !editable}<span>Only owners and developers can change it.</span>{/if}
		{/if}
	{/snippet}
</PageHeader>

{#if type.isError}
	<Problem error={type.error} />
{:else if type.data}
	{#key type.data.version}
		<TypeBuilder {project} type={type.data} {editable} />
	{/key}
{:else}
	<div class="grid max-w-4xl gap-3" aria-busy="true">
		<Skeleton class="h-10 w-80" />
		<Skeleton class="h-24 w-full" />
		<Skeleton class="h-24 w-full" />
	</div>
{/if}
