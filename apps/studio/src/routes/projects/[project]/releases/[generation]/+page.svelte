<!--
  A release's receipt (D-051). First what went live: one row per entry it changed for readers —
  new, a newer version, or taken down — each linking to its entry, with its changes a click away
  (the audit's P1-6). Then whether it reached the website: its one label, each destination's part
  in delivering it, and every look a probe took for it, oldest first. "Verified" always says how
  many locations looked: a probe proves what it saw from where it ran, nothing more. Rolling back
  to it asks first, showing what the rollback would change (RollbackDialog). Technical detail
  stays in the drawer.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import { createInfiniteQuery, createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import { type Delivery, getReceipt, listReleaseChanges, may } from '#lib/api.ts';
	import { originRelease, originText, releaseOrigin } from '#lib/releaseOrigin.ts';
	import ItemDiff from '#lib/ItemDiff.svelte';
	import RollbackDialog from '#lib/RollbackDialog.svelte';
	import StatusBadge from '#lib/shell/StatusBadge.svelte';
	import { markReceiptSeen } from '#lib/firstRun.ts';
	import { deliveryText, failureText } from '#lib/receipts.ts';
	import { ago, projectQuery, releasesQuery, typesQuery } from '#lib/queries.ts';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import ReleaseLabel from '#lib/shell/ReleaseLabel.svelte';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';
	import { CONNECTION_KIND, count, PUBLISH_REQUEST, releaseName, WEBSITE_CONNECTIONS } from '#lib/words.ts';

	const project = $derived(page.params.project ?? '');
	const generation = $derived(Number(page.params.generation ?? '0'));
	// A pending receipt settles on its own: jobs and probes commit as `releases` hints, and the
	// live stream refreshes this receipt (D-108) — nothing to poll for.
	const receipt = createQuery(() => ({
		queryKey: ['receipt', project, generation],
		queryFn: () => getReceipt(project, generation)
	}));

	// What it changed for readers, 100 at a time (D-185): only the entries this role reads, so the
	// count of the rest comes from the release's own totals once the last page is in.
	type ChangesPage = Awaited<ReturnType<typeof listReleaseChanges>>;
	const changes = createInfiniteQuery(() => ({
		queryKey: ['receipt', project, generation, 'changes'],
		queryFn: ({ pageParam }: { pageParam: string | null }) => listReleaseChanges(project, generation, pageParam),
		initialPageParam: null as string | null,
		getNextPageParam: (last: ChangesPage) => last.nextCursor
	}));
	const rows = $derived((changes.data?.pages ?? []).flatMap((one) => one.changes));
	const when = (at: string) => new Intl.DateTimeFormat(undefined, { dateStyle: 'medium', timeStyle: 'short' }).format(new Date(at));
	const mine = createQuery(() => projectQuery(project));
	const releases = createQuery(() => releasesQuery(project));
	const fixes = $derived(may(mine.data, 'destinations.manage'));
	// Restoring is a changeset like any other: it goes through review, never around it (D-097).
	const latest = $derived(releases.data?.generation ?? 0);
	const restores = $derived(may(mine.data, 'content.update') && generation < latest);
	const at = (n: number) => resolve('/projects/[project]/releases/[generation]', { project, generation: String(n) });
	// Nothing is made by one click: the button opens the preview, which makes it on confirmation.
	let rolling = $state(false);
	const types = createQuery(() => typesQuery(project));
	const typeName = (collection: string) => types.data?.find((type) => type.slug === collection)?.definition.name ?? collection;
	// Which rows show their changes: none until asked, so the list reads as a list.
	let open = $state<Record<string, boolean>>({});
	const listed = $derived(receipt.data === undefined ? 0 : receipt.data.entries + receipt.data.unpublished);
	// Entries this role can't open: known once every page is in.
	const unseen = $derived(changes.hasNextPage || changes.data === undefined ? 0 : Math.max(0, listed - rows.length));
	const origin = $derived(receipt.data === undefined ? null : releaseOrigin(receipt.data));
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
			{#if origin !== null}
				{@const target = originRelease(origin)}
				<span data-testid="release-origin">· {#if target !== null}<a class="underline underline-offset-2" href={at(target)}>{originText(origin)}</a>{/if}</span>
			{/if}
		{/snippet}
		{#snippet actions()}
			{#if restores}
				<Button variant="outline" size="sm" onclick={() => (rolling = true)} data-testid="rollback">Roll back to this release…</Button>
			{/if}
			<a class="text-sm text-muted-foreground underline underline-offset-2 hover:text-foreground" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: shown.changeset_id })}
				>The {PUBLISH_REQUEST}</a
			>
		{/snippet}
	</PageHeader>

	{#if restores}<RollbackDialog {project} {generation} bind:open={rolling} />{/if}

	{#if latest > 1}
		<div class="mb-8 flex flex-wrap items-center gap-3 rounded-xl border border-border bg-card px-4 py-3" data-testid="release-slider">
			<Button variant="ghost" size="sm" href={generation > 1 ? at(generation - 1) : undefined} disabled={generation <= 1} aria-label="Older release">← Older</Button>
			<input
				type="range"
				min="1"
				max={latest}
				value={generation}
				aria-label="Release"
				class="min-w-0 flex-1 accent-primary max-sm:order-last max-sm:basis-full"
				onchange={(event) => goto(at(Number(event.currentTarget.value)))}
				data-testid="slider"
			/>
			<span class="shrink-0 text-sm text-muted-foreground">Release <strong class="font-medium text-foreground">{generation}</strong> of {latest}</span>
			<Button variant="ghost" size="sm" href={generation < latest ? at(generation + 1) : undefined} disabled={generation >= latest} aria-label="Newer release">Newer →</Button>
		</div>
	{/if}

	<section class="mb-10" aria-labelledby="went-live-heading" data-testid="went-live">
		<h2 id="went-live-heading" class="mb-3 flex items-baseline gap-2 text-base font-semibold">
			What went live
			<span class="text-sm font-normal text-muted-foreground tabular" data-testid="receipt-counts"
				>{#if shown.unpublished > 0}{[...(shown.entries > 0 ? [`${shown.entries} went live`] : []), `${shown.unpublished} taken down`].join(', ')}{:else}{count(listed, 'entry', 'entries')}{/if}</span
			>
		</h2>
		{#if changes.isPending}
			<div class="grid gap-2" aria-busy="true" data-testid="receipt-rows-loading">
				<Skeleton class="h-14 w-full rounded-xl" />
				<Skeleton class="h-14 w-full rounded-xl" />
			</div>
		{:else if changes.isError}
			<Problem error={changes.error} what="what this release changed" retry={() => changes.refetch()} />
		{:else if rows.length === 0 && unseen === 0}
			<p class="text-sm text-muted-foreground">This release changed nothing readers get.</p>
		{:else}
			{#if rows.length > 0}
				<ul class="overflow-hidden rounded-xl border border-border bg-card" data-testid="receipt-rows">
					{#each rows as change (change.entry_id)}
						{@const expanded = open[change.entry_id] ?? false}
						{@const badge = change.kind === 'unpublish' ? 'taken_down' : change.previous_revision_id === null ? 'new' : 'updated'}
						<li class="rule-dotted last:border-b-0" data-testid="receipt-row" data-entry={change.entry_id} data-change={badge}>
							<div class="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-3">
								<span class="min-w-0 flex-1">
									{#if change.title_hidden}
										<span class="block truncate font-medium text-muted-foreground italic" data-testid="title-hidden">Title hidden for your role</span>
									{:else if change.entry_deleted}
										<span class="block truncate font-medium" title={change.title} data-user-content>{change.title}</span>
									{:else}
										<a class="block truncate font-medium hover:underline" href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: change.collection, entry: change.entry_id })} title={change.title} data-user-content
											>{change.title}</a
										>
									{/if}
									<span class="text-xs text-muted-foreground">{typeName(change.collection)}{change.entry_deleted ? ' · deleted since' : ''}</span>
								</span>
								<StatusBadge state={badge} testid="receipt-change" />
								{#if !change.entry_deleted}
									<Button variant="ghost" size="xs" aria-expanded={expanded} aria-controls="change-{change.entry_id}" onclick={() => (open[change.entry_id] = !expanded)} data-testid="see-changes">
										See changes<ChevronDownIcon class="transition-transform {expanded ? 'rotate-180' : ''}" />
									</Button>
								{/if}
							</div>
							{#if expanded}
								<div id="change-{change.entry_id}" class="unfold border-t border-dotted border-border bg-background/60 px-4 py-4">
									<ItemDiff {project} item={{ kind: change.kind, entry_id: change.entry_id, collection: change.collection, revision_id: change.revision_id, published_revision_id: change.previous_revision_id }} done />
								</div>
							{/if}
						</li>
					{/each}
				</ul>
			{/if}
			{#if changes.hasNextPage || changes.isFetchingNextPage || changes.isFetchNextPageError}
				<div class="mt-3 flex flex-wrap items-center gap-3">
					<Button variant="outline" size="sm" onclick={() => changes.fetchNextPage()} disabled={changes.isFetchingNextPage} data-testid="receipt-more">
						{changes.isFetchingNextPage ? 'Loading more…' : `Show more (${rows.length} of ${listed})`}
					</Button>
					{#if changes.isFetchNextPageError}
						<p role="alert" class="text-sm text-destructive">Couldn't load more. Try again.</p>
					{/if}
				</div>
			{/if}
			{#if unseen > 0}
				<p class="mt-2 text-xs text-muted-foreground" data-testid="receipt-unseen">
					And {count(unseen, 'entry', 'entries')} your role can't open.
				</p>
			{/if}
		{/if}
	</section>

	<section aria-labelledby="reach-heading" data-testid="reach">
		<h2 id="reach-heading" class="mb-3 text-base font-semibold">Did it reach your website?</h2>
		<div class="mb-8 flex flex-wrap items-center gap-3 rounded-xl border border-border bg-card px-4 py-4">
			<div class="text-[0.9375rem]"><ReleaseLabel release={shown} testid="receipt-label" /></div>
			{#if shown.label === 'published' && shown.deliveries.length === 0}
				<p class="text-sm text-muted-foreground" data-testid="receipt-explainer">
					Your website can read it now. Nothing checks your website yet; add a site check in
					<a class="underline underline-offset-2" href={resolve('/projects/[project]/settings/connections', { project })}>Settings → Website connections</a> to confirm
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
				<h3 id="attention-heading" class="font-semibold text-attention-ink">What needs attention</h3>
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
						<Button variant="outline" href={resolve('/projects/[project]/settings/connections', { project })} data-testid="fix-connections">Open {WEBSITE_CONNECTIONS}</Button>
					{:else}
						<p class="text-sm text-attention-ink">Ask someone who manages {WEBSITE_CONNECTIONS} (an owner or developer) to check them.</p>
					{/if}
				</div>
			</section>
		{/if}

		<div class="grid gap-10 lg:grid-cols-2">
			<section aria-labelledby="destinations-heading">
				<h3 id="destinations-heading" class="mb-3 text-sm font-medium">Website connections</h3>
				<ul class="space-y-2" data-testid="deliveries">
					{#each shown.deliveries as delivery (delivery.destination_id)}
						<li class="flex items-start gap-3 rounded-lg border border-border bg-card px-3 py-2.5 text-sm" data-state={delivery.state}>
							<span class="mt-1.5 size-2 shrink-0 rounded-full {DOT[delivery.state]}" aria-hidden="true"></span>
							<span class="min-w-0 flex-1">
								<strong class="font-medium">{delivery.name}</strong>
								<span class="text-xs text-muted-foreground">{CONNECTION_KIND[delivery.kind].name}{delivery.location ? ` at ${delivery.location}` : ''}</span>:
								{deliveryText(delivery)}{#if delivery.attempts && delivery.attempts > 1}, after {delivery.attempts} attempts{/if}
							</span>
						</li>
					{:else}
						<li class="text-sm text-muted-foreground">None were set up when this went live. Connect your website in Settings so each release rebuilds it, clears the old copies its cache keeps, or is checked.</li>
					{/each}
				</ul>
			</section>

			<section aria-labelledby="observations-heading">
				<h3 id="observations-heading" class="mb-3 text-sm font-medium">What the site checks saw</h3>
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
	</section>
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
