<!--
  One shared component (D-073) in the builder, beside the list of content types and components:
  its fields, the content types that use it, and, after a save, the new version each of them got.
  Changing one takes a role that may change every content type, since a change reaches them all;
  everyone else reads.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { type Component, getComponent, may } from '#lib/api.ts';
	import BuilderFrame from '#lib/BuilderFrame.svelte';
	import ComponentBuilder from '#lib/ComponentBuilder.svelte';
	import { projectQuery, typesQuery } from '#lib/queries.ts';
	import Problem from '#lib/shell/Problem.svelte';

	const project = $derived(page.params.project ?? '');
	const slug = $derived(page.params.component ?? '');
	const mine = createQuery(() => projectQuery(project));
	// Everywhere: a change reaches every content type that uses it.
	const editable = $derived(may(mine.data, 'schema.change'));
	const loaded = createQuery(() => ({ queryKey: ['component', project, slug], queryFn: () => getComponent(project, slug) }));
	const types = createQuery(() => typesQuery(project));
	const nameOf = (type: string) => types.data?.find((item) => item.slug === type)?.definition.name ?? type;
	// The last save's answer, kept here because the builder starts afresh on the new version.
	let saved = $state<Component | null>(null);
</script>

<svelte:head><title>{loaded.data?.component.definition.name ?? 'Component'} · Content types · Colophon Studio</title></svelte:head>

<BuilderFrame {project} current={{ kind: 'component', slug }}>
	{#if saved && saved.slug === slug}
		<div role="status" class="mb-6 rounded-xl border border-border bg-card px-4 py-3 text-sm" data-testid="component-saved">
			<p class="font-medium">Saved {saved.definition.name}, version {saved.version}.</p>
			{#if saved.content_types?.length}
				<p class="mt-1 text-muted-foreground">
					Also changed:
					{#each saved.content_types as type, index (type.slug)}{index > 0 ? ', ' : ''}<a
							class="font-medium text-foreground underline"
							href={resolve('/projects/[project]/settings/types/[type]', { project, type: type.slug })}>{nameOf(type.slug)}</a
						>, now version {type.version}{/each}.
				</p>
			{:else}
				<p class="mt-1 text-muted-foreground">No content type needed a new version.</p>
			{/if}
		</div>
	{/if}

	{#if loaded.isError}
		<Problem error={loaded.error} what="this component" retry={() => loaded.refetch()} />
	{:else if loaded.data}
		{#key `${loaded.data.component.slug}:${loaded.data.component.version}`}
			<ComponentBuilder {project} component={loaded.data.component} etag={loaded.data.etag} {editable} onsaved={(result) => (saved = result)} />
		{/key}
	{:else}
		<div class="grid gap-3" aria-busy="true">
			<Skeleton class="h-10 w-80" />
			<Skeleton class="h-16 w-full" />
		</div>
	{/if}
</BuilderFrame>
