<!--
  The collections themselves, as a page: collections and single types each in their own
  group, with the fields each declares. From here a type's entries or its fields are one
  click — the same page Strapi calls the Content-Type Builder.
-->
<script lang="ts">
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import type { ContentType } from '#lib/api.ts';
	import { projectQuery, typesQuery } from '#lib/queries.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';

	const project = $derived(page.params.project ?? '');
	const types = createQuery(() => typesQuery(project));
	const mine = createQuery(() => projectQuery(project));
	const mayChange = $derived(mine.data?.role === 'owner' || mine.data?.role === 'developer');

	const collections = $derived((types.data ?? []).filter((type) => type.definition.kind !== 'single'));
	const singles = $derived((types.data ?? []).filter((type) => type.definition.kind === 'single'));
</script>

{#snippet typeRow(type: ContentType)}
	<li>
		<div class="group flex items-center gap-3 rounded-lg px-2 py-2 hover:bg-muted">
			<a class="min-w-0 flex-1" href={resolve('/projects/[project]/types/[type]', { project, type: type.slug })}>
				<span class="block text-sm font-medium">{type.definition.name}</span>
				<span class="text-xs text-muted-foreground">
					{type.slug} · {type.definition.fields.length} {type.definition.fields.length === 1 ? 'field' : 'fields'} · v{type.version}
				</span>
			</a>
			{#if mayChange}
				<Button variant="outline" size="sm" href={resolve('/projects/[project]/types/[type]', { project, type: type.slug })}>Change fields</Button>
			{/if}
			<ArrowRightIcon class="size-4 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" aria-hidden="true" />
		</div>
	</li>
{/snippet}

{#if types.isPending}
	<div class="space-y-2"><Skeleton class="h-14 w-full" /><Skeleton class="h-14 w-full" /><Skeleton class="h-14 w-full" /></div>
{:else if types.isError}
	<Problem error={types.error} />
{:else}
	<PageHeader title="Types">
		{#snippet meta()}
			<span>What an entry can be — its fields, in order.</span>
		{/snippet}
		{#snippet actions()}
			{#if mayChange}
				<Button href={resolve('/projects/[project]/types/new', { project })}><PlusIcon data-icon="inline-start" />New type</Button>
			{/if}
		{/snippet}
	</PageHeader>

	<section class="mt-2" aria-labelledby="collections-heading">
		<div class="mb-3 flex items-baseline justify-between">
			<h2 id="collections-heading" class="text-base font-semibold">Collections</h2>
			<span class="text-xs text-muted-foreground">Many entries of a kind</span>
		</div>
		{#if collections.length === 0}
			<p class="rounded-lg border border-dashed border-border px-3 py-4 text-sm text-muted-foreground">No collections yet — a collection is a kind of entry (articles, authors, products).</p>
		{:else}
			<ul class="space-y-1" data-testid="types-collections">
				{#each collections as type (type.slug)}
					{@render typeRow(type)}
				{/each}
			</ul>
		{/if}
	</section>

	<section class="mt-8" aria-labelledby="singles-heading">
		<div class="mb-3 flex items-baseline justify-between">
			<h2 id="singles-heading" class="text-base font-semibold">Single types</h2>
			<span class="text-xs text-muted-foreground">Exactly one entry — a homepage, a settings page</span>
		</div>
		{#if singles.length === 0}
			<p class="rounded-lg border border-dashed border-border px-3 py-4 text-sm text-muted-foreground">No single types — mark "This collection holds one entry" on a type to make one.</p>
		{:else}
			<ul class="space-y-1" data-testid="types-singles">
				{#each singles as type (type.slug)}
					{@render typeRow(type)}
				{/each}
			</ul>
		{/if}
	</section>
{/if}
