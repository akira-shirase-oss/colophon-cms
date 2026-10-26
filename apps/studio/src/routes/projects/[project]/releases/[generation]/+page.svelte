<!--
  A release's receipt (D-051): its one label, each destination's part in delivering it, and every
  look a probe took for it, oldest first. "Verified" always says how many locations looked: a probe
  proves what it saw from where it ran, nothing more. Technical detail stays in the timeline.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { getReceipt } from '#lib/api.ts';
	import { deliveryText, labelText } from '#lib/receipts.ts';

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
</script>

<svelte:head><title>Release {generation} · Colophon Studio</title></svelte:head>

<p><a href={resolve('/projects/[project]/releases', { project })}>← Releases</a></p>
{#if receipt.isPending}
	<p class="muted">Loading…</p>
{:else if receipt.isError}
	<p role="alert">{receipt.error.message}</p>
{:else}
	{@const shown = receipt.data}
	<h1>Release {shown.generation}: {shown.title}</h1>
	<p class="label" data-testid="receipt-label" data-label={shown.label}>{labelText(shown)}</p>
	<p class="muted">
		Published by {shown.published_by_name ?? shown.published_by}, {when(shown.published_at)}. {shown.entries}
		{shown.entries === 1 ? 'entry' : 'entries'} changed{#if shown.unpublished > 0}, {shown.unpublished} taken down{/if}.
	</p>
	{#if shown.label === 'published' && shown.deliveries.length === 0}
		<p class="muted">Nothing checks the website yet. Add a probe on the Releases page to verify what readers see.</p>
	{/if}

	<h2>Destinations</h2>
	<ul class="list" data-testid="deliveries">
		{#each shown.deliveries as delivery (delivery.destination_id)}
			<li data-state={delivery.state}>
				<strong>{delivery.name}</strong>
				<small class="muted">{delivery.kind}{delivery.location ? ` at ${delivery.location}` : ''}</small>:
				{deliveryText(delivery)}{#if delivery.attempts && delivery.attempts > 1}, after {delivery.attempts} attempts{/if}
			</li>
		{:else}
			<li class="muted">None.</li>
		{/each}
	</ul>

	<h2>What the probes saw</h2>
	<ol class="list" data-testid="observations">
		{#each shown.observations as seen, index (index)}
			<li>
				<time datetime={seen.observed_at}>{when(seen.observed_at)}</time>, from <strong>{seen.location}</strong>
				({named(seen.destination_id)}):
				{#if seen.observed_generation !== null}release {seen.observed_generation}{:else}no release named{/if}{#if seen.http_status !== null},
					HTTP {seen.http_status}{/if}{#if seen.error && seen.error !== `HTTP ${seen.http_status}`}
					<span class="muted">({seen.error})</span>{/if}
			</li>
		{:else}
			<li class="muted">No probe has looked for this release.</li>
		{/each}
	</ol>
{/if}

<style>
	.label {
		display: inline-block;
		font: var(--md-sys-typescale-label-large);
		padding: 0.25rem 0.75rem;
		border-radius: var(--md-sys-shape-corner-small, 8px);
		background: var(--md-sys-color-secondary-container);
		color: var(--md-sys-color-on-secondary-container);
	}
	.label[data-label='needs_attention'] {
		background: var(--md-sys-color-error-container);
		color: var(--md-sys-color-on-error-container);
	}
	.list {
		display: grid;
		gap: 0.5rem;
		padding-left: 1.25rem;
	}
	.muted {
		color: var(--md-sys-color-on-surface-variant);
	}
</style>
