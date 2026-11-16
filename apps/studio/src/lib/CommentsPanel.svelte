<!--
	Comments on one entry's versions (E1, D-102). A comment binds the immutable version its author
	looked at, so it stays true as drafts move on; it is kept verbatim — never edited, never
	deleted — only answered (resolved) or reopened. A deleted entry's comments stay readable while
	nothing new can be added; the composer closes with a note instead of failing later.
-->
<script lang="ts">
	import CheckCheckIcon from '@lucide/svelte/icons/check-check';
	import MessageSquareIcon from '@lucide/svelte/icons/message-square';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import * as Tabs from '@colophon/ui/ui/tabs';
	import { Textarea } from '@colophon/ui/ui/textarea';
	import { ApiError, createComment, type EntryComment, newKey, setCommentResolution } from './api.ts';
	import { plainError } from './errors.ts';
	import { ago, commentsQuery, meQuery, membersQuery } from './queries.ts';
	import EmptyState from './shell/EmptyState.svelte';
	import Problem from './shell/Problem.svelte';

	let {
		project,
		collection,
		entry,
		revision = null,
		canWrite = false,
		live = true,
		oncount
	}: {
		project: string;
		collection: string;
		entry: string;
		/** The version a new comment binds — the one on screen. `null` closes the composer. */
		revision?: string | null;
		/** `content.update` on this entry: writing and answering comments ask for it. */
		canWrite?: boolean;
		/** `false` when the entry was deleted: its comments stay readable, nothing new is added. */
		live?: boolean;
		/** Called with (open, total) as the list changes; for a count beside the panel's tab. */
		oncount?: (open: number, total: number) => void;
	} = $props();

	const queryClient = useQueryClient();
	const who = createQuery(meQuery);
	const people = createQuery(() => membersQuery(project));
	/** Who wrote it: a member's name, "you", or an honest note for someone who left. */
	const person = (id: string) => {
		if (id === who.data?.id) return 'you';
		const member = people.data?.find((member) => member.identity_id === id);
		return member?.display_name ?? 'someone who left the project';
	};

	let showing = $state<'all' | 'open' | 'resolved'>('all');
	const comments = createQuery(() => commentsQuery(project, collection, entry, showing === 'all' ? undefined : showing));

	let body = $state('');
	let busy = $state(false);
	let note = $state('');
	// Set when a write answers not-found: the entry is gone — comments stay, nothing new can be added.
	let gone = $state(false);
	const closed = $derived(gone || !live || revision === null);
	// A comment just written shows at once, before its page comes back; deduped when it does.
	let pending = $state<EntryComment[]>([]);
	const shown = $derived.by(() => {
		const listed = comments.data?.data ?? [];
		const there = new Set(listed.map((comment) => comment.id));
		return [...pending.filter((comment) => !there.has(comment.id)), ...listed];
	});
	$effect(() => oncount?.(shown.filter((comment) => comment.resolved_at === null).length, shown.length));

	async function refresh() {
		pending = [];
		await queryClient.invalidateQueries({ queryKey: ['comments', project, collection, entry] });
	}

	async function post(event: SubmitEvent) {
		event.preventDefault();
		if (revision === null || body.trim() === '' || busy) return;
		busy = true;
		note = '';
		try {
			const comment = await createComment(project, collection, entry, { revision, body: body.trim() }, newKey());
			body = '';
			pending = [comment, ...pending];
			await refresh();
		} catch (failure) {
			if (failure instanceof ApiError && failure.failure === 'not-found') {
				gone = true;
			} else {
				note = plainError(failure, 'write this comment');
			}
		} finally {
			busy = false;
		}
	}

	async function answer(comment: EntryComment, resolved: boolean) {
		if (!canWrite || busy) return;
		busy = true;
		note = '';
		try {
			await setCommentResolution(project, collection, entry, comment.id, comment.etag, resolved, newKey());
			await refresh();
		} catch (failure) {
			// `comment_moved`/`already_resolved`/`already_open` mean the list is stale: refetch, then
			// say so in the comment's own words.
			await refresh();
			note = plainError(failure, resolved ? 'answer this comment' : 'reopen this comment');
		} finally {
			busy = false;
		}
	}
</script>

<section aria-label="Comments" data-testid="comments-panel">
	{#if closed}
		<p class="mb-4 rounded-xl border border-dashed border-border px-4 py-4 text-sm text-muted-foreground" data-testid="comments-closed">
			{#if gone || !live}
				This entry was deleted. What was said stays here — nothing new can be added to a deleted entry.
			{:else}
				Open a version of this entry to write a comment.
			{/if}
		</p>
	{:else if canWrite}
		<form class="mb-4" onsubmit={post} aria-label="Write a comment" data-testid="comment-composer">
			<Textarea
				bind:value={body}
				rows={3}
				maxlength={8000}
				placeholder="Say what needs changing or checking…"
				aria-label="Write a comment"
				data-testid="comment-input"
			/>
			<div class="mt-2 flex items-center justify-between gap-3">
				<p class="text-xs text-muted-foreground">It attaches to the version on screen and can’t be edited.</p>
				<Button type="submit" size="sm" disabled={busy || body.trim() === ''} data-testid="comment-post">Comment</Button>
			</div>
		</form>
	{:else}
		<p class="mb-4 text-sm text-muted-foreground" data-testid="comments-readonly">Your role can’t write comments here.</p>
	{/if}

	<Tabs.Root bind:value={showing as string}>
		<Tabs.List aria-label="Show comments">
			<Tabs.Trigger value="all" data-testid="comments-all">All</Tabs.Trigger>
			<Tabs.Trigger value="open" data-testid="comments-open">Open</Tabs.Trigger>
			<Tabs.Trigger value="resolved" data-testid="comments-resolved">Answered</Tabs.Trigger>
		</Tabs.List>
	</Tabs.Root>

	{#if note !== ''}
		<p role="alert" class="mt-3 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm" data-testid="comments-note">{note}</p>
	{/if}

	<div class="mt-4">
		{#if comments.isPending}
			<Skeleton class="h-40 w-full rounded-xl" />
		{:else if comments.isError}
			<Problem error={comments.error} what="comments" retry={() => comments.refetch()} />
		{:else if shown.length === 0}
			<EmptyState icon={MessageSquareIcon} title="Nothing said yet" testid="comments-empty">
				Say what needs changing or checking — it attaches to the version on screen and stays with it.
			</EmptyState>
		{:else}
			<ol class="space-y-3" data-testid="comments">
				{#each shown as comment (comment.id)}
					<li class="rounded-xl border border-border bg-card p-4" data-testid="comment" data-resolved={comment.resolved_at !== null}>
						<div class="flex items-start justify-between gap-3">
							<p class="text-sm">
								<span class="font-medium" data-user-content>{person(comment.author_id)}</span>
								<span class="text-muted-foreground"> · {ago(comment.created_at)}</span>
							</p>
							{#if canWrite}
								{#if comment.resolved_at === null}
									<Button variant="outline" size="xs" disabled={busy} onclick={() => answer(comment, true)} data-testid="comment-resolve">
										<CheckCheckIcon data-icon="inline-start" />Resolve
									</Button>
								{:else}
									<Button variant="ghost" size="xs" disabled={busy} onclick={() => answer(comment, false)} data-testid="comment-reopen">
										<RotateCcwIcon data-icon="inline-start" />Reopen
									</Button>
								{/if}
							{/if}
						</div>
						<p class="mt-1.5 whitespace-pre-wrap text-sm" data-user-content>{comment.body}</p>
						<p class="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
							{#if comment.field !== null}<span>on <span class="font-mono" data-user-content>{comment.field}</span></span>{/if}
							{#if revision !== null && comment.revision_id !== revision}<span>on an earlier version</span>{/if}
							{#if comment.resolved_at !== null}
								<Badge variant="live">Answered</Badge>
								{#if comment.resolved_by !== null}<span>by <span data-user-content>{person(comment.resolved_by)}</span> {ago(comment.resolved_at)}</span>{/if}
							{/if}
						</p>
					</li>
				{/each}
			</ol>
		{/if}
	</div>
</section>
