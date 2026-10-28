<!--
  The content-type builder's front page (its own item in the Studio's navigation): every content
  type and shared component, with how many fields each has, beside the same list in the left
  panel. Pick one to change it, or make a new one from the panel.
-->
<script lang="ts">
	import ComponentIcon from '@lucide/svelte/icons/component';
	import FilesIcon from '@lucide/svelte/icons/files';
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import ShapesIcon from '@lucide/svelte/icons/shapes';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import BuilderFrame from '#lib/BuilderFrame.svelte';
	import { usedBy } from '#lib/fieldDefs.ts';
	import { componentsQuery, typesQuery } from '#lib/queries.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import Problem from '#lib/shell/Problem.svelte';

	const project = $derived(page.params.project ?? '');
	const types = createQuery(() => typesQuery(project));
	const components = createQuery(() => componentsQuery(project));
	const fields = (count: number) => `${count} field${count === 1 ? '' : 's'}`;
</script>

<svelte:head><title>Content types · Colophon Studio</title></svelte:head>

{#snippet card(href: string, name: string, line: string, Icon: typeof FilesIcon, testid: string)}
	<li>
		<a
			{href}
			class="flex h-full items-start gap-3 rounded-xl border border-border bg-card p-4 shadow-hairline outline-none hover:border-ring/60 hover:bg-accent focus-visible:border-ring focus-visible:ring-4 focus-visible:ring-ring/25"
			data-testid={testid}
		>
			<span class="flex size-9 shrink-0 items-center justify-center rounded-lg border border-border bg-background" aria-hidden="true"><Icon class="size-4" /></span>
			<span class="grid min-w-0 gap-0.5">
				<span class="truncate font-medium">{name}</span>
				<span class="text-xs text-muted-foreground">{line}</span>
			</span>
		</a>
	</li>
{/snippet}

<BuilderFrame {project}>
	<header class="mb-6 space-y-1.5">
		<h1 class="text-2xl leading-tight font-semibold tracking-tight sm:text-[1.75rem]">Content types</h1>
		<p class="max-w-2xl text-sm text-muted-foreground">
			The kinds of content this project holds, and the fields each has. Pick one to change it. Changes that could break saved entries are checked
			against them first.
		</p>
	</header>
	{#if types.isError}
		<Problem error={types.error} what="content types" retry={() => types.refetch()} />
	{:else if types.isPending}
		<Skeleton class="h-32 w-full rounded-xl" />
	{:else if types.data.length === 0}
		<EmptyState icon={ShapesIcon} title="No content types yet" testid="types-empty">Make the first one with New content type, on the left.</EmptyState>
	{:else}
		<ul class="grid gap-3 sm:grid-cols-2" data-testid="content-types">
			{#each types.data as type (type.slug)}
				{@render card(
					resolve('/projects/[project]/settings/types/[type]', { project, type: type.slug }),
					type.definition.name,
					`${type.definition.single === true ? 'Just one entry' : 'Many entries'} · ${fields(type.definition.fields.length)}`,
					type.definition.single === true ? FileTextIcon : FilesIcon,
					`type-card-${type.slug}`
				)}
			{/each}
		</ul>
		{#if (components.data ?? []).length > 0}
			<h2 class="mt-8 mb-3 text-base font-semibold">Components</h2>
			<ul class="grid gap-3 sm:grid-cols-2" data-testid="components">
				{#each components.data ?? [] as shared (shared.slug)}
					{@const users = usedBy(types.data ?? [], shared.slug)}
					{@render card(
						resolve('/projects/[project]/settings/components/[component]', { project, component: shared.slug }),
						shared.definition.name,
						`${fields(shared.definition.fields.length)} · ${users.length === 0 ? 'Not used yet' : `Used by ${users.map((user) => user.name).join(', ')}`}`,
						ComponentIcon,
						`component-card-${shared.slug}`
					)}
				{/each}
			</ul>
		{/if}
	{/if}
</BuilderFrame>
