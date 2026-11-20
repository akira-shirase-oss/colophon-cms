<!--
  An entry's history (D-184): one row per sitting — saves one person made within five minutes of
  each other — with who, when and what it changed ("Title edited · Body: +2 paragraphs"), so rows
  read apart. The version readers have carries the live rule on its left and its release. A
  sitting opens to its single saves. Compare and Restore show on the row being pointed at or
  focused (always on a touch screen): Compare as a link, Restore as a secondary button.
-->
<script lang="ts">
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import { createQueries, createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { type Content, type Definition, getRevision, type Revision, type Summary } from './api.ts';
	import { compare } from './diff.ts';
	import DiffView from './DiffView.svelte';
	import { changeSummary, clock, initials, type Sitting, sittings } from './history.ts';
	import { meQuery, membersQuery } from './queries.ts';
	import StatusBadge from './shell/StatusBadge.svelte';
	import { releaseName } from './words.ts';

	let {
		project,
		collection,
		entry,
		definition,
		revisions,
		published,
		draft,
		restores,
		ready,
		restoring,
		onrestore
	}: {
		project: string;
		collection: string;
		entry: string;
		definition: Definition;
		/** Newest first, as the API lists them (the newest 100). */
		revisions: Revision[];
		published: Summary['published'];
		/** What the editor would save now, for comparing a version with it. */
		draft: () => Content;
		/** Whether the role may change any field this would restore. */
		restores: boolean;
		/** Whether the draft is saved, so a restore builds on it. */
		ready: boolean;
		/** The version being restored, if one is. */
		restoring: string | null;
		onrestore: (revision: Revision) => void;
	} = $props();

	const who = createQuery(meQuery);
	const people = createQuery(() => membersQuery(project));
	/** "You", a member's name, or an honest note for someone who isn't one now. */
	const person = (id: string) =>
		id === who.data?.id ? 'You' : (people.data?.find((member) => member.identity_id === id)?.display_name ?? 'Someone no longer in the project');
	const avatar = (id: string) => initials(id === who.data?.id ? (who.data?.display_name ?? 'You') : person(id));

	const PAGE = 10;
	let shown = $state(PAGE);
	const all = $derived(sittings(revisions, [published?.revision_id]));
	const visible = $derived(all.slice(0, shown));
	// Each shown sitting's last version and the one before it, read once (versions never change),
	// to say what the sitting changed.
	const needed = $derived([...new Set(all.slice(0, shown + 1).map((sitting) => sitting.last.id))]);
	const contents = createQueries(() => ({
		queries: needed.map((id) => ({
			queryKey: ['revision', project, collection, entry, id],
			queryFn: () => getRevision(project, collection, entry, id),
			staleTime: Infinity
		}))
	}));
	const contentOf = (id: string) => contents[needed.indexOf(id)]?.data?.content;
	function summary(index: number): string | null {
		const sitting = all[index];
		if (sitting === undefined) return null;
		const after = contentOf(sitting.last.id);
		const previous = all[index + 1];
		const before = previous === undefined ? null : contentOf(previous.last.id);
		if (after === undefined || before === undefined) return null;
		return changeSummary(definition, before, after);
	}

	let open = $state<string | null>(null);
	let comparing = $state<string | null>(null);
	const compared = createQuery(() => ({
		queryKey: ['revision', project, collection, entry, comparing],
		queryFn: () => getRevision(project, collection, entry, comparing ?? ''),
		enabled: comparing !== null,
		staleTime: Infinity
	}));

	const isLive = (sitting: Sitting) => published !== null && sitting.revisions.some((revision) => revision.id === published.revision_id);
	const span = (sitting: Sitting) => {
		const end = clock(sitting.last.created_at);
		const start = clock(sitting.first.created_at);
		return start === end ? end : `${start}–${end.split(', ').at(-1)}`;
	};
</script>

{#snippet actions(revision: Revision, current: boolean)}
	{#if !current}
		<span class="actions flex shrink-0 items-center gap-1">
			<Button
				variant="link"
				size="xs"
				class="h-7 px-1.5"
				aria-expanded={comparing === revision.id}
				aria-label="{comparing === revision.id ? 'Hide the comparison with' : 'Compare with the draft:'} the version saved {clock(revision.created_at)}"
				onclick={() => (comparing = comparing === revision.id ? null : revision.id)}
				data-testid="history-compare">{comparing === revision.id ? 'Hide' : 'Compare'}</Button
			>
			{#if restores}
				<Button
					variant="secondary"
					size="xs"
					disabled={restoring !== null || !ready}
					title={ready ? undefined : 'Wait until your changes are saved'}
					aria-label="Restore the version saved {clock(revision.created_at)}"
					onclick={() => onrestore(revision)}
					data-testid="history-restore">{restoring === revision.id ? 'Restoring…' : 'Restore'}</Button
				>
			{/if}
		</span>
	{/if}
{/snippet}

{#snippet comparison(revision: Revision)}
	{#if comparing === revision.id}
		<div class="unfold mt-2 rounded-lg border border-border bg-card p-4" data-testid="history-comparison">
			{#if compared.data}
				<DiffView changes={compare(definition, compared.data.content, draft())} before="this version" after="the draft" />
			{:else if compared.isError}
				<p class="text-sm text-destructive">{compared.error.message}</p>
			{:else}
				<p class="text-sm text-muted-foreground">Loading…</p>
			{/if}
		</div>
	{/if}
{/snippet}

<ol data-testid="history" class="grid gap-1">
	{#each visible as sitting, index (sitting.last.id)}
		{@const live = isLive(sitting)}
		{@const current = index === 0}
		{@const said = summary(index)}
		<li
			class="row relative rounded-lg py-3 pr-2 pl-4 transition-colors hover:bg-muted/60 focus-within:bg-muted/60"
			data-testid="history-row"
			data-saves={sitting.revisions.length}
			data-live={live || undefined}
		>
			<!-- What readers have: the live rule on the left, as on the status card. -->
			<span class="absolute inset-y-2 left-0 w-0.5 rounded-full {live ? 'bg-live' : 'bg-transparent'}" aria-hidden="true"></span>
			<div class="flex flex-wrap items-start gap-x-3 gap-y-2">
				<span class="mt-0.5 flex size-7 shrink-0 items-center justify-center rounded-full bg-muted text-xs font-medium text-muted-foreground" aria-hidden="true"
					>{avatar(sitting.author)}</span
				>
				<div class="min-w-0 flex-1">
					<p class="flex flex-wrap items-center gap-x-2 gap-y-1 text-sm">
						<span class="font-medium" data-user-content>{person(sitting.author)}</span>
						<span class="text-muted-foreground tabular" title={new Date(sitting.last.created_at).toLocaleString()}>{span(sitting)}</span>
						{#if current}<span class="text-muted-foreground">· Current draft</span>{/if}
						{#if live && published}<StatusBadge state="live" text="Live in {releaseName(published.generation)}" testid="history-live" class="tabular" />{/if}
					</p>
					<p class="mt-0.5 text-sm text-pretty text-muted-foreground" data-testid="history-summary">
						{#if said === null}<span class="inline-block h-4 w-40 animate-pulse rounded bg-muted align-middle motion-reduce:animate-none" aria-label="Working out what changed"></span>{:else}{said}{/if}
					</p>
					{#if sitting.revisions.length > 1}
						<button
							type="button"
							class="mt-1 inline-flex items-center gap-1 rounded-md text-sm text-muted-foreground outline-none hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/40"
							aria-expanded={open === sitting.last.id}
							onclick={() => (open = open === sitting.last.id ? null : sitting.last.id)}
							data-testid="history-saves"
						>
							<ChevronRightIcon class="size-3.5 transition-transform {open === sitting.last.id ? 'rotate-90' : ''}" aria-hidden="true" />{sitting.revisions.length} saves
						</button>
					{/if}
				</div>
				{@render actions(sitting.last, current)}
			</div>
			{@render comparison(sitting.last)}
			{#if open === sitting.last.id}
				<ol class="mt-2 ml-10 grid gap-0.5 border-l border-border pl-3" data-testid="history-sitting">
					{#each sitting.revisions as revision, at (revision.id)}
						<li class="row rounded-md py-1.5 pr-1 pl-2 hover:bg-muted/60 focus-within:bg-muted/60">
							<div class="flex flex-wrap items-center gap-x-3 gap-y-1">
								<span class="min-w-0 flex-1 text-sm tabular text-muted-foreground" title={new Date(revision.created_at).toLocaleString()}
									>Saved {clock(revision.created_at)}{#if published?.revision_id === revision.id}<span class="text-foreground"> · live</span>{/if}</span
								>
								{@render actions(revision, current && at === 0)}
							</div>
							{@render comparison(revision)}
						</li>
					{/each}
				</ol>
			{/if}
		</li>
	{/each}
</ol>
{#if all.length > shown}
	<Button variant="secondary" size="sm" class="mt-3" onclick={() => (shown += PAGE)} data-testid="history-more">Show older</Button>
{:else if revisions.length >= 100}
	<p class="mt-3 text-sm text-muted-foreground">The newest 100 saves are shown.</p>
{/if}

<style>
	/* Pointer screens show a row's actions on hover or focus, or while it is compared; touch
	   screens always. */
	@media (hover: hover) {
		.actions {
			opacity: 0;
		}
		.row:hover > div > .actions,
		.row:focus-within > div > .actions,
		.actions:has(:global([aria-expanded='true'])) {
			opacity: 1;
		}
	}
</style>
