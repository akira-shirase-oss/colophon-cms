<!--
  One content type in the builder (D-054, D-055), beside the list of content types and components:
  its fields, what can change, and, when the server refuses a change, the entries in the way.
  People whose role changes content types edit; everyone else reads.
-->
<script lang="ts">
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { getContentType, may } from '#lib/api.ts';
	import BuilderFrame from '#lib/BuilderFrame.svelte';
	import { projectQuery } from '#lib/queries.ts';
	import Problem from '#lib/shell/Problem.svelte';
	import TypeBuilder from '#lib/TypeBuilder.svelte';

	const project = $derived(page.params.project ?? '');
	const slug = $derived(page.params.type ?? '');
	const mine = createQuery(() => projectQuery(project));
	const editable = $derived(may(mine.data, 'schema.change', slug));
	const type = createQuery(() => ({ queryKey: ['type', project, slug], queryFn: () => getContentType(project, slug) }));
</script>

<svelte:head><title>{type.data?.definition.name ?? 'Content type'} · Content types · Colophon Studio</title></svelte:head>

<BuilderFrame {project} current={{ kind: 'type', slug }}>
	{#if type.isError}
		<Problem error={type.error} what="this content type" retry={() => type.refetch()} />
	{:else if type.data}
		{#key `${type.data.slug}:${type.data.version}`}
			<TypeBuilder {project} type={type.data} {editable} />
		{/key}
	{:else}
		<div class="grid gap-3" aria-busy="true">
			<Skeleton class="h-10 w-80" />
			<Skeleton class="h-16 w-full" />
			<Skeleton class="h-16 w-full" />
		</div>
	{/if}
</BuilderFrame>
