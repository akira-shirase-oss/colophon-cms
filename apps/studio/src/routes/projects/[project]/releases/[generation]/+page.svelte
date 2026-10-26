<!--
  A release's receipt (D-051): its one label, each destination's part in delivering it, and every
  look a probe took for it, oldest first. "Verified" always says how many locations looked: a probe
  proves what it saw from where it ran, nothing more. Technical detail stays in the timeline.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { type Delivery, getReceipt } from '#lib/api.ts';
	import { deliveryText } from '#lib/receipts.ts';
	import { ago } from '#lib/queries.ts';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import ReleaseLabel from '#lib/shell/ReleaseLabel.svelte';

	const project = $derived(page.params.project ?? '');
	const generation = $derived(Number(page.params.generation ?? '0'));
	// A pending receipt settles on its own: look again every few seconds until it does.
	const receipt = createQuery(() => ({
		queryKey: ['receipt', project, generation],
		queryFn: () => getReceipt(project, generation),
		refetchInterval: (query) => (query.state.data?.label === 'pending' ? 2_000 : false)
	}));

	const when = (at: string) => new Date(at).toLocaleString();
	const named = (id: string) => receipt.data?.deliveries.find((delivery) => delivery.destination_id === id)?.name ?? 'a probe';
	const DOT: Record<Delivery['state'], string> = { done: 'bg-live', covered: 'bg-live', pending: 'bg-pending', failed: 'bg-attention' };
	const KIND: Record<Delivery['kind'], string> = { webhook: 'Webhook', cdn: 'CDN', probe: 'Probe' };
</script>

<svelte:head><title>Release {generation} · Colophon Studio</title></svelte:head>

{#if receipt.isPending}
	<Skeleton class="mb-8 h-10 w-1/2" />
	<Skeleton class="h-48 w-full rounded-xl" />
{:else if receipt.isError}
	<Problem error={receipt.error} />
{:else}
	{@const shown = receipt.data}
	<PageHeader title="Release {shown.generation}: {shown.title}">
		{#snippet eyebrow()}Receipt{/snippet}
		{#snippet meta()}
			<span>Published by {shown.published_by_name ?? shown.published_by}, {when(shown.published_at)}</span>
			<span aria-hidden="true">·</span>
			<span>{shown.entries} {shown.entries === 1 ? 'entry' : 'entries'} changed{#if shown.unpublished > 0}, {shown.unpublished} taken down{/if}</span>
		{/snippet}
		{#snippet actions()}
			<a class="text-sm text-muted-foreground underline underline-offset-2 hover:text-foreground" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: shown.changeset_id })}
				>The changeset</a
			>
		{/snippet}
	</PageHeader>

	<div class="mb-8 flex flex-wrap items-center gap-3 rounded-xl border border-border bg-card px-4 py-4">
		<div class="text-[0.9375rem]"><ReleaseLabel release={shown} testid="receipt-label" /></div>
		{#if shown.label === 'published' && shown.deliveries.length === 0}
			<p class="text-sm text-muted-foreground">Nothing checks the website yet. Add a probe in Settings to verify what readers see.</p>
		{:else if shown.label === 'verified'}
			<p class="text-sm text-muted-foreground">A probe read this release (or a later one) on the live site.</p>
		{:else if shown.label === 'pending'}
			<p class="text-sm text-muted-foreground">On its way: the label settles by itself.</p>
		{/if}
	</div>

	<div class="grid gap-10 lg:grid-cols-2">
		<section aria-labelledby="destinations-heading">
			<h2 id="destinations-heading" class="mb-3 text-base font-semibold">Destinations</h2>
			<ul class="space-y-2" data-testid="deliveries">
				{#each shown.deliveries as delivery (delivery.destination_id)}
					<li class="flex items-start gap-3 rounded-lg border border-border bg-card px-3 py-2.5 text-sm" data-state={delivery.state}>
						<span class="mt-1.5 size-2 shrink-0 rounded-full {DOT[delivery.state]}" aria-hidden="true"></span>
						<span class="min-w-0 flex-1">
							<strong class="font-medium">{delivery.name}</strong>
							<span class="text-xs text-muted-foreground">{KIND[delivery.kind]}{delivery.location ? ` at ${delivery.location}` : ''}</span>:
							{deliveryText(delivery)}{#if delivery.attempts && delivery.attempts > 1}, after {delivery.attempts} attempts{/if}
						</span>
					</li>
				{:else}
					<li class="text-sm text-muted-foreground">None.</li>
				{/each}
			</ul>
		</section>

		<section aria-labelledby="observations-heading">
			<h2 id="observations-heading" class="mb-3 text-base font-semibold">What the probes saw</h2>
			<ol class="space-y-3 border-l border-dotted border-border pl-5" data-testid="observations">
				{#each shown.observations as seen, index (index)}
					<li class="relative text-sm">
						<span
							class="absolute top-1.5 -left-[1.6rem] size-2.5 rounded-full border-2 border-background {seen.observed_generation !== null && seen.observed_generation >= shown.generation ? 'bg-live' : 'bg-attention'}"
							aria-hidden="true"
						></span>
						<p>
							From <strong class="font-medium">{seen.location}</strong> ({named(seen.destination_id)}):
							{#if seen.observed_generation !== null}release {seen.observed_generation}{:else}no release named{/if}{#if seen.http_status !== null},
								HTTP {seen.http_status}{/if}{#if seen.error && seen.error !== `HTTP ${seen.http_status}`}
								<span class="text-muted-foreground">({seen.error})</span>{/if}
						</p>
						<p class="text-xs text-muted-foreground"><time datetime={seen.observed_at} title={when(seen.observed_at)}>{ago(seen.observed_at)}</time></p>
					</li>
				{:else}
					<li class="text-sm text-muted-foreground">No probe has looked for this release.</li>
				{/each}
			</ol>
		</section>
	</div>
{/if}
