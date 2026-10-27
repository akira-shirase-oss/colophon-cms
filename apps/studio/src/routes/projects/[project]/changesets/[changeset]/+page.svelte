<!--
  One publish request (a changeset, D-044): the exact versions it holds and what each changes for
  readers, where it stands, and the actions that move it on. Every action names the hash on
  screen, so nobody approves a version they didn't see: if the request changed meanwhile, the page
  shows the current one and says so. Internal names and hashes are in the Technical details.
-->
<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import MessageSquareIcon from '@lucide/svelte/icons/message-square';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Label } from '@colophon/ui/ui/label';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { Textarea } from '@colophon/ui/ui/textarea';
	import { ApiError, type Changeset, changesetAction, getChangeset, may, newKey, publishChangeset } from '#lib/api.ts';
	import ItemDiff from '#lib/ItemDiff.svelte';
	import { ago, projectQuery, releasesQuery, typesQuery } from '#lib/queries.ts';
	import ChangesetBadge from '#lib/shell/ChangesetBadge.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';
	import { PUBLISH_REQUEST } from '#lib/words.ts';

	const project = $derived(page.params.project ?? '');
	const id = $derived(page.params.changeset ?? '');
	const queryClient = useQueryClient();
	const set = createQuery(() => ({ queryKey: ['changeset', project, id], queryFn: () => getChangeset(project, id) }));
	// The role comes from the project itself: me().projects lists at most 1,000 projects. Reviewing
	// and publishing need the permission in every collection the changeset touches (D-058).
	const mine = createQuery(() => projectQuery(project));
	const collections = $derived([...new Set((set.data?.items ?? []).map((item) => item.collection))]);
	const reviewer = $derived(collections.every((collection) => may(mine.data, 'content.review', collection)));
	const publisher = $derived(collections.every((collection) => may(mine.data, 'content.publish', collection)));
	// Publishing names the release it follows: the latest one this page has seen.
	const releases = createQuery(() => releasesQuery(project));
	const types = createQuery(() => typesQuery(project));
	const typeName = (collection: string) => types.data?.find((type) => type.slug === collection)?.definition.name ?? collection;
	// One key per publish attempt: pressing Publish again after a lost answer is safe.
	let publishKey = newKey();
	// Which items show their comparison; the first few open by default.
	let open = $state<Record<string, boolean>>({});

	let comment = $state('');
	let error = $state('');
	let notice = $state('');
	let busy = $state(false);
	const STALE = `This ${PUBLISH_REQUEST} changed since you loaded it. Here is the current one: check it, then try again.`;

	async function publish(shown: Changeset) {
		const base = releases.data?.generation;
		if (base === undefined) return;
		busy = true;
		error = '';
		notice = '';
		try {
			await publishChangeset(project, id, shown.hash, base, publishKey);
			publishKey = newKey();
		} catch (failure) {
			if (failure instanceof ApiError && failure.reason === 'generation_moved') {
				notice = 'Another release was published first. Check what is live now, then publish again.';
				publishKey = newKey();
			} else if (failure instanceof ApiError && failure.reason === 'stale_changeset') {
				notice = STALE;
			} else {
				error = failure instanceof ApiError ? failure.message : String(failure);
			}
		} finally {
			busy = false;
			await Promise.all([
				queryClient.invalidateQueries({ queryKey: ['changeset', project, id] }),
				queryClient.invalidateQueries({ queryKey: ['releases', project] }),
				queryClient.invalidateQueries({ queryKey: ['changesets', project] }),
				queryClient.invalidateQueries({ queryKey: ['entries', project] })
			]);
		}
	}

	async function act(shown: Changeset, action: 'submit' | 'refresh' | 'reviews', decision?: 'approve' | 'request_changes') {
		busy = true;
		error = '';
		notice = '';
		try {
			const body = decision === undefined ? undefined : { decision, comment };
			const next = await changesetAction(project, id, shown.hash, action, body);
			queryClient.setQueryData(['changeset', project, id], next);
			if (decision !== undefined) comment = '';
		} catch (failure) {
			if (failure instanceof ApiError && failure.reason === 'stale_changeset') {
				notice = STALE;
			} else {
				error = failure instanceof ApiError ? failure.message : String(failure);
			}
			await queryClient.invalidateQueries({ queryKey: ['changeset', project, id] });
		} finally {
			busy = false;
			void queryClient.invalidateQueries({ queryKey: ['changesets', project] });
		}
	}

	const STEPS = ['Prepared', 'Sent for review', 'Approved', 'Live'] as const;
	function reached(shown: Changeset): number {
		switch (shown.status.state) {
			case 'published':
				return 4;
			case 'approved':
				return 3;
			case 'awaiting_review':
			case 'changes_requested':
			case 'approval_expired':
				return 2;
			default:
				return 1;
		}
	}
</script>

<svelte:head><title>{set.data?.title ?? 'Publish request'} · Colophon Studio</title></svelte:head>

{#if set.isPending}
	<Skeleton class="mb-8 h-10 w-1/2" />
	<Skeleton class="h-64 w-full rounded-xl" />
{:else if set.isError}
	<Problem error={set.error} />
{:else}
	{@const shown = set.data}
	<PageHeader title={shown.title}>
		{#snippet meta()}
			<span data-testid="changeset-status" data-state={shown.status.state} class="inline-flex flex-wrap items-center gap-2">
				<ChangesetBadge state={shown.status.state} />
				{#if shown.status.state === 'approved'}
					<span>by {shown.status.by}</span>
					{#if shown.status.self_review}<Badge variant="attention" data-testid="self-review">approved their own request</Badge>{/if}
				{:else if shown.status.state === 'changes_requested'}
					<span>by {shown.status.by}</span>
				{:else if shown.status.state === 'published'}
					<span>as <a class="underline underline-offset-2" href={resolve('/projects/[project]/releases', { project })} data-testid="release-link">release {shown.status.generation}</a></span>
				{/if}
			</span>
		{/snippet}
		{#snippet actions()}
			{#if shown.status.state === 'stale'}
				<Button onclick={() => act(shown, 'refresh')} disabled={busy}>Update to the latest versions</Button>
			{:else if shown.status.state === 'open'}
				<Button onclick={() => act(shown, 'submit')} disabled={busy}>Ask for review</Button>
			{:else if shown.status.state === 'approved' && publisher}
				<Button onclick={() => publish(shown)} disabled={busy || releases.data === undefined}>Publish</Button>
			{/if}
		{/snippet}
	</PageHeader>

	<ol class="mb-8 grid grid-cols-4 gap-2" aria-label="Progress">
		{#each STEPS as step, index (step)}
			<li class="grid gap-1.5">
				<span class="h-1 rounded-full {index < reached(shown) ? 'bg-foreground' : 'bg-border'}"></span>
				<span class="text-xs {index < reached(shown) ? 'text-foreground' : 'text-muted-foreground'}">{step}</span>
			</li>
		{/each}
	</ol>

	{#if notice}<p role="status" class="mb-4 rounded-lg bg-pending-soft px-4 py-3 text-sm text-pending-ink" data-testid="changeset-notice">{notice}</p>{/if}
	{#if error}<p role="alert" class="mb-4 rounded-lg bg-destructive/10 px-4 py-3 text-sm text-destructive">{error}</p>{/if}

	<div class="grid gap-10 lg:grid-cols-[minmax(0,1fr)_20rem]">
		<section aria-labelledby="items-heading">
			<h2 id="items-heading" class="mb-3 text-base font-semibold">Entries <span class="font-normal text-muted-foreground tabular">{shown.items.length}</span></h2>
			<ul class="space-y-3" data-testid="changeset-items">
				{#each shown.items as item, index (item.entry_id)}
					{@const expanded = open[item.entry_id] ?? index < 3}
					<li class="overflow-hidden rounded-xl border border-border bg-card">
						<div class="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-3">
							<span class="min-w-0 flex-1">
								<a class="block truncate font-medium hover:underline" href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: item.collection, entry: item.entry_id })}
									>{item.title}</a
								>
								<span class="text-xs text-muted-foreground">{typeName(item.collection)}</span>
							</span>
							{#if item.kind === 'unpublish'}
								<Badge variant="attention" data-testid="unpublish">unpublish</Badge>
							{:else if item.published_revision_id === null}
								<Badge variant="live">new</Badge>
							{:else}
								<Badge variant="pending">update</Badge>
							{/if}
							{#if item.edited}
								<Badge variant="attention" data-testid="edited"
									>{#if item.kind === 'publish'}edited since it was added{:else if item.current_revision_id === null}no longer published{:else}a newer version was published{/if}</Badge
								>
							{/if}
							<Button
								variant="ghost"
								size="xs"
								aria-expanded={expanded}
								aria-controls="diff-{item.entry_id}"
								onclick={() => (open[item.entry_id] = !expanded)}
							>
								Changes<ChevronDownIcon class="transition-transform {expanded ? 'rotate-180' : ''}" />
							</Button>
						</div>
						{#if expanded}
							<div id="diff-{item.entry_id}" class="unfold border-t border-dotted border-border bg-background/60 px-4 py-4">
								<ItemDiff {project} {item} />
							</div>
						{/if}
					</li>
				{/each}
			</ul>
		</section>

		<div class="space-y-8">
			{#if reviewer && shown.submitted_by !== null && shown.status.state !== 'stale' && shown.status.state !== 'published'}
				<section class="rounded-xl border border-border bg-card p-4 shadow-xs" aria-label="Review">
					<h2 class="text-base font-semibold">Review</h2>
					<p class="mt-1 text-sm text-muted-foreground">You approve exactly what is shown here. If it changes, your approval no longer counts.</p>
					<div class="mt-4 grid gap-1.5">
						<Label for="review-comment">Comment (optional)</Label>
						<Textarea id="review-comment" bind:value={comment} rows={3} maxlength={2000} />
					</div>
					<div class="mt-3 grid gap-2">
						<Button onclick={() => act(shown, 'reviews', 'approve')} disabled={busy}>Approve this version</Button>
						<Button variant="outline" onclick={() => act(shown, 'reviews', 'request_changes')} disabled={busy}>Request changes</Button>
					</div>
				</section>
			{/if}

			<section aria-labelledby="history-heading">
				<h2 id="history-heading" class="mb-3 text-base font-semibold">History</h2>
				<ol class="space-y-3 border-l border-dotted border-border pl-5" data-testid="changeset-reviews">
					{#each shown.reviews as review (review.id)}
						<li class="relative text-sm" class:opacity-70={!review.current}>
							<span class="absolute top-1.5 -left-[1.6rem] size-2.5 rounded-full border-2 border-background {review.decision === 'approve' ? 'bg-live' : 'bg-attention'}" aria-hidden="true"></span>
							<p>
								<strong class="font-medium">{review.decision === 'approve' ? 'Approved' : 'Changes requested'}</strong>
								by {review.reviewer_name ?? review.reviewer_id}{#if review.decision === 'approve' && review.reviewer_id === shown.submitted_by}<span
										class="text-muted-foreground"
										data-testid="self-review-note">, who sent it: their own request</span
									>{/if}
							</p>
							<p class="text-xs text-muted-foreground" title={new Date(review.created_at).toLocaleString()}>
								{ago(review.created_at)}{#if !review.current}<span> · on an earlier version</span>{/if}
							</p>
							{#if review.comment}
								<blockquote class="mt-2 flex gap-2 rounded-lg bg-muted px-3 py-2 text-sm">
									<MessageSquareIcon class="mt-0.5 size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />{review.comment}
								</blockquote>
							{/if}
						</li>
					{/each}
					{#if shown.submitted_at}
						<li class="relative text-sm">
							<span class="absolute top-1.5 -left-[1.6rem] size-2.5 rounded-full border-2 border-background bg-pending" aria-hidden="true"></span>
							<p>Sent for review</p>
							<p class="text-xs text-muted-foreground">{ago(shown.submitted_at)}</p>
						</li>
					{/if}
					<li class="relative text-sm">
						<span class="absolute top-1.5 -left-[1.6rem] size-2.5 rounded-full border-2 border-background bg-draft" aria-hidden="true"></span>
						<p>Prepared</p>
						<p class="text-xs text-muted-foreground">{ago(shown.created_at)}</p>
					</li>
				</ol>
			</section>
		</div>
	</div>

	<TechnicalDetails class="mt-10">
		<p>Changeset <span class="font-mono break-all">{shown.id}</span>, hash <span class="font-mono break-all">{shown.hash}</span></p>
		{#each shown.items as item (item.entry_id)}
			<p>
				{item.kind === 'unpublish' ? 'Unpublishes' : 'Pins'} revision <span class="font-mono break-all">{item.revision_id}</span> of
				<span class="font-mono">{item.collection}</span> entry <span class="font-mono break-all">{item.entry_id}</span> ({item.content_hash.slice(0, 19)}…)
			</p>
		{/each}
	</TechnicalDetails>
{/if}
