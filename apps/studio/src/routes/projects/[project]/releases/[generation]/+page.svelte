<!--
  A release's receipt (D-051): its one label, each destination's part in delivering it, and every
  look a probe took for it, oldest first. "Verified" always says how many locations looked: a probe
  proves what it saw from where it ran, nothing more. Technical detail stays in the timeline.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { type Delivery, getReceipt, may } from '#lib/api.ts';
	import { markReceiptSeen } from '#lib/firstRun.ts';
	import { deliveryText, failureText } from '#lib/receipts.ts';
	import { ago, projectQuery } from '#lib/queries.ts';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import ReleaseLabel from '#lib/shell/ReleaseLabel.svelte';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';
	import { CONNECTION_KIND, PUBLISH_REQUEST, WEBSITE_CONNECTIONS } from '#lib/words.ts';

	const project = $derived(page.params.project ?? '');
	const generation = $derived(Number(page.params.generation ?? '0'));
	// A pending receipt settles on its own: look again every few seconds until it does.
	const receipt = createQuery(() => ({
		queryKey: ['receipt', project, generation],
		queryFn: () => getReceipt(project, generation),
		refetchInterval: (query) => (query.state.data?.label === 'pending' ? 2_000 : false)
	}));

	const when = (at: string) => new Date(at).toLocaleString();
	const mine = createQuery(() => projectQuery(project));
	const fixes = $derived(may(mine.data, 'destinations.manage'));
	const failed = $derived((receipt.data?.deliveries ?? []).filter((delivery) => delivery.state === 'failed'));
	const named = (id: string) => receipt.data?.deliveries.find((delivery) => delivery.destination_id === id)?.name ?? 'a site check';
	const deliveryTextShort = (delivery: Delivery) => (delivery.last_error ? failureText(delivery.last_error) : 'it failed');
	const DOT: Record<Delivery['state'], string> = { done: 'bg-live', covered: 'bg-live', pending: 'bg-pending', failed: 'bg-attention' };
	// The first-run checklist's last step is looking at a receipt (step 3.7).
	onMount(() => markReceiptSeen(project));
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
				>The {PUBLISH_REQUEST}</a
			>
		{/snippet}
	</PageHeader>

	<div class="mb-8 flex flex-wrap items-center gap-3 rounded-xl border border-border bg-card px-4 py-4">
		<div class="text-[0.9375rem]"><ReleaseLabel release={shown} testid="receipt-label" /></div>
		{#if shown.label === 'published' && shown.deliveries.length === 0}
			<p class="text-sm text-muted-foreground" data-testid="receipt-explainer">
				Published in Colophon: your website can read it now. Nothing checks your website yet; add a site check in
				<a class="underline underline-offset-2" href={resolve('/projects/[project]/settings', { project }) + '#website-connections'}>Settings → Website connections</a> to confirm
				what readers see.
			</p>
		{:else if shown.label === 'verified'}
			<p class="text-sm text-muted-foreground">A site check read this release (or a later one) on the live site.</p>
		{:else if shown.label === 'pending'}
			<p class="text-sm text-muted-foreground">On its way: the label settles by itself.</p>
		{/if}
	</div>

	{#if shown.label === 'needs_attention'}
		<!-- Needs attention always says what failed and the safe next step (spec §05's labels). A
		     retry button for one delivery comes with the API's retry (B5). -->
		<section class="mb-8 rounded-xl border border-attention/40 bg-attention-soft/60 p-5" aria-labelledby="attention-heading" data-testid="attention">
			<h2 id="attention-heading" class="font-semibold text-attention-ink">What needs attention</h2>
			<p class="mt-1 text-sm text-attention-ink">
				{#if failed.length > 0}
					{failed.map((delivery) => delivery.name).join(', ')} gave up on this release:
					{failed.map((delivery) => deliveryTextShort(delivery)).join('; ')}.
				{:else}
					A website connection gave up on this release.
				{/if}
				It is published in Colophon, and anything that reads Colophon directly has it, but your website may not show it yet.
			</p>
			<ol class="mt-3 list-decimal space-y-1 pl-5 text-sm text-attention-ink">
				<li>Check the connection's address, and that your website is up.</li>
				<li>Publish again: the next release carries this one's changes, and the label of this receipt follows when it is delivered.</li>
			</ol>
			<div class="mt-4">
				{#if fixes}
					<Button variant="outline" href={resolve('/projects/[project]/settings', { project }) + '#website-connections'} data-testid="fix-connections">Open {WEBSITE_CONNECTIONS}</Button>
				{:else}
					<p class="text-sm text-attention-ink">Ask someone who manages {WEBSITE_CONNECTIONS} (an owner or developer) to check them.</p>
				{/if}
			</div>
		</section>
	{/if}

	<div class="grid gap-10 lg:grid-cols-2">
		<section aria-labelledby="destinations-heading">
			<h2 id="destinations-heading" class="mb-3 text-base font-semibold">Website connections</h2>
			<ul class="space-y-2" data-testid="deliveries">
				{#each shown.deliveries as delivery (delivery.destination_id)}
					<li class="flex items-start gap-3 rounded-lg border border-border bg-card px-3 py-2.5 text-sm" data-state={delivery.state}>
						<span class="mt-1.5 size-2 shrink-0 rounded-full {DOT[delivery.state]}" class:breathe={delivery.state === 'pending'} aria-hidden="true"></span>
						<span class="min-w-0 flex-1">
							<strong class="font-medium">{delivery.name}</strong>
							<span class="text-xs text-muted-foreground">{CONNECTION_KIND[delivery.kind].name}{delivery.location ? ` at ${delivery.location}` : ''}</span>:
							{deliveryText(delivery)}{#if delivery.attempts && delivery.attempts > 1}, after {delivery.attempts} attempts{/if}
						</span>
					</li>
				{:else}
					<li class="text-sm text-muted-foreground">None were set up when this went live. Connect your website in Settings so each release rebuilds it, clears its CDN, or is checked.</li>
				{/each}
			</ul>
		</section>

		<section aria-labelledby="observations-heading">
			<h2 id="observations-heading" class="mb-3 text-base font-semibold">What the site checks saw</h2>
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
					<li class="text-sm text-muted-foreground">No site check has looked for this release.</li>
				{/each}
			</ol>
		</section>
	</div>
	<TechnicalDetails class="mt-10">
		<p>Generation <span class="font-mono">{shown.generation}</span> (epoch {shown.epoch}) from changeset <span class="font-mono break-all">{shown.changeset_id}</span>, hash <span class="font-mono break-all">{shown.changeset_hash}</span></p>
		{#each shown.deliveries as delivery (delivery.destination_id)}
			<p>Destination <span class="font-mono break-all">{delivery.destination_id}</span> ({delivery.kind}): job {delivery.status ?? 'none'}{delivery.attempts ? `, ${delivery.attempts} attempts` : ''}</p>
		{/each}
		{#each shown.observations as seen, index (index)}
			{#if seen.body_hash}<p>Probe at {seen.location}: body <span class="font-mono break-all">{seen.body_hash}</span></p>{/if}
		{/each}
	</TechnicalDetails>
{/if}
