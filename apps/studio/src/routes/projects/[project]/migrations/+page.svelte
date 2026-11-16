<!--
  A project's migration plans (D-100, D-101): the content-type changes stored entries couldn't
  take as they were, and how each plan is doing. A plan is made from the type's page when a
  change is refused with entries in the way; this page lists what exists. Any member may look;
  only a role that may change the schema can make or move one, and it finds the affordance on
  the refusal itself.
-->
<script lang="ts">
	import RouteIcon from '@lucide/svelte/icons/route';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import * as Table from '@colophon/ui/ui/table';
	import { ago, migrationsQuery, projectQuery, typesQuery } from '#lib/queries.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import MigrationBadge from '#lib/shell/MigrationBadge.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';

	const project = $derived(page.params.project ?? '');
	const plans = createQuery(() => migrationsQuery(project));
	const mine = createQuery(() => projectQuery(project));
	const types = createQuery(() => typesQuery(project));
	const typeName = (collection: string) => types.data?.find((type) => type.slug === collection)?.definition.name ?? collection;
</script>

<svelte:head><title>Migrations · Colophon Studio</title></svelte:head>

<PageHeader title="Migrations">
	{#snippet description()}
		When a change to a content type would leave entries invalid, a plan carries it out: it names every entry in the way, waits for approval, and a
		worker applies it. Plans are made from a content type's page, on the refusal that names the entries.
	{/snippet}
</PageHeader>

{#if plans.isPending || mine.isPending}
	<Skeleton class="h-48 w-full rounded-xl" />
{:else if plans.isError}
	<Problem error={plans.error} what="migrations" />
{:else if plans.data.length === 0}
	<EmptyState icon={RouteIcon} title="No migrations yet" testid="migrations-empty">
		A plan appears here when a content-type change can't apply while entries stay as they are. Until then every type change applies directly.
	</EmptyState>
{:else}
	<div class="overflow-hidden rounded-xl border border-border bg-card">
		<Table.Root>
			<Table.Header>
				<Table.Row class="hover:bg-transparent">
					<Table.Head class="pl-4">Content type</Table.Head>
					<Table.Head>Status</Table.Head>
					<Table.Head class="hidden sm:table-cell">Entries</Table.Head>
					<Table.Head class="hidden md:table-cell">Version</Table.Head>
					<Table.Head class="pr-4 text-right">Updated</Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body data-testid="migrations" class="stagger">
				{#each plans.data as plan, index (plan.id)}
					<Table.Row class="relative" style="--i: {index}">
						<Table.Cell class="max-w-0 pl-4 sm:max-w-md">
							<a class="block truncate font-medium after:absolute after:inset-0" href={resolve('/projects/[project]/migrations/[migration]', { project, migration: plan.id })}
								>{typeName(plan.collection)}</a
							>
						</Table.Cell>
						<Table.Cell><MigrationBadge state={plan.state} /></Table.Cell>
						<Table.Cell class="hidden text-sm text-muted-foreground sm:table-cell">
							{plan.items.done}/{plan.items.expected} done{#if plan.items.blocked > 0}, {plan.items.blocked} in the way{/if}
						</Table.Cell>
						<Table.Cell class="hidden text-sm text-muted-foreground md:table-cell">v{plan.base_version} → {plan.target_version === null ? 'next' : `v${plan.target_version}`}</Table.Cell>
						<Table.Cell class="pr-4 text-right text-sm text-muted-foreground">{ago(plan.updated_at)}</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	</div>
{/if}

<TechnicalDetails class="mt-8">
	<p>Each plan binds a collection, the version it was computed on and the definition it moves to; its hash is what every action names.</p>
	<p class="font-mono break-all">/api/v1/projects/{project}/environments/production/migrations</p>
</TechnicalDetails>
