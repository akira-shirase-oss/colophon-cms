<!--
  One changeset (D-044): the exact revisions it pins, where it stands, and the actions that move
  it on. Every action names the hash on screen, so nobody approves a version they didn't see: if
  the changeset changed in the meantime, the page shows the current one and says so.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui';
	import { ApiError, changesetAction, getChangeset, me, type Changeset } from '#lib/api.ts';
	import { STATE_LABEL } from '#lib/changesets.ts';

	const project = $derived(page.params.project ?? '');
	const id = $derived(page.params.changeset ?? '');
	const queryClient = useQueryClient();
	const set = createQuery(() => ({ queryKey: ['changeset', project, id], queryFn: () => getChangeset(project, id) }));
	const who = createQuery(() => ({ queryKey: ['me'], queryFn: me }));
	const role = $derived(who.data?.projects.find((mine) => mine.id === project)?.role);
	const reviewer = $derived(role === 'owner' || role === 'publisher');

	let comment = $state('');
	let error = $state('');
	let notice = $state('');
	let busy = $state(false);

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
				notice = 'This changeset changed since you loaded it. Here is the current version: check it, then try again.';
			} else {
				error = failure instanceof ApiError ? failure.message : String(failure);
			}
			await queryClient.invalidateQueries({ queryKey: ['changeset', project, id] });
		} finally {
			busy = false;
			void queryClient.invalidateQueries({ queryKey: ['changesets', project] });
		}
	}

	const when = (at: string) => new Date(at).toLocaleString();
</script>

<svelte:head><title>{set.data?.title ?? 'Changeset'} · Colophon Studio</title></svelte:head>

<p><a href={resolve('/projects/[project]/changesets', { project })}>← Changesets</a></p>
{#if set.isPending}
	<p class="muted">Loading…</p>
{:else if set.isError}
	<p role="alert">{set.error.message}</p>
{:else}
	{@const shown = set.data}
	<h1>{shown.title}</h1>
	<p class="status" data-testid="changeset-status" data-state={shown.status.state}>
		<strong>{STATE_LABEL[shown.status.state]}</strong>
		{#if shown.status.state === 'approved'}
			by {shown.status.by}{#if shown.status.self_review}<span class="flag" data-testid="self-review"> (reviewed their own changeset)</span>{/if}
		{:else if shown.status.state === 'changes_requested'}
			by {shown.status.by}
		{/if}
	</p>
	<p class="muted small">Version <code title={shown.hash}>{shown.hash.slice(7, 19)}</code></p>
	{#if notice}<p role="status" class="notice" data-testid="changeset-notice">{notice}</p>{/if}
	{#if error}<p role="alert" class="error">{error}</p>{/if}

	<h2>Entries</h2>
	<ul class="items" data-testid="changeset-items">
		{#each shown.items as item (item.entry_id)}
			<li>
				<a href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: item.collection, entry: item.entry_id })}>{item.title}</a>
				<small class="muted">{item.collection}</small>
				{#if item.edited}<span class="flag" data-testid="edited">edited since pinned</span>{/if}
			</li>
		{/each}
	</ul>

	<div class="actions">
		{#if shown.status.state === 'stale'}
			<Button onclick={() => act(shown, 'refresh')} disabled={busy}>Refresh to the current versions</Button>
		{:else if shown.status.state === 'open'}
			<Button onclick={() => act(shown, 'submit')} disabled={busy}>Submit for review</Button>
		{/if}
	</div>

	{#if reviewer && shown.submitted_by !== null && shown.status.state !== 'stale'}
		<section class="review" aria-label="Review">
			<h2>Review</h2>
			<label class="field">
				Comment (optional)
				<textarea bind:value={comment} rows="3" maxlength="2000"></textarea>
			</label>
			<div class="actions">
				<Button onclick={() => act(shown, 'reviews', 'approve')} disabled={busy}>Approve this version</Button>
				<Button variant="outlined" onclick={() => act(shown, 'reviews', 'request_changes')} disabled={busy}>Request changes</Button>
			</div>
		</section>
	{/if}

	<h2>History</h2>
	<ul class="items" data-testid="changeset-reviews">
		{#each shown.reviews as review (review.id)}
			<li class:old={!review.current}>
				<strong>{review.decision === 'approve' ? 'Approved' : 'Changes requested'}</strong>
				by {review.reviewer_name ?? review.reviewer_id}, {when(review.created_at)}
				{#if !review.current}<small class="muted"> on an earlier version</small>{/if}
				{#if review.comment}<blockquote>{review.comment}</blockquote>{/if}
			</li>
		{:else}
			<li class="muted">No reviews yet.</li>
		{/each}
	</ul>
{/if}

<style>
	.muted {
		color: var(--md-sys-color-on-surface-variant);
	}
	.small {
		font: var(--md-sys-typescale-body-small);
	}
	.status {
		font: var(--md-sys-typescale-title-medium);
	}
	.flag {
		color: var(--md-sys-color-tertiary);
		font: var(--md-sys-typescale-label-large);
	}
	.notice {
		padding: 0.5rem 0.75rem;
		border-radius: var(--md-sys-shape-corner-small, 8px);
		background: var(--md-sys-color-tertiary-container);
		color: var(--md-sys-color-on-tertiary-container);
	}
	.error {
		color: var(--md-sys-color-error);
	}
	.items {
		display: grid;
		gap: 0.5rem;
		padding-left: 1.25rem;
		max-width: 44rem;
	}
	.old {
		opacity: 0.75;
	}
	.actions {
		display: flex;
		gap: 0.75rem;
		margin: 1rem 0;
	}
	.review {
		max-width: 40rem;
	}
	.field {
		display: grid;
		gap: 0.25rem;
		font: var(--md-sys-typescale-body-small);
		color: var(--md-sys-color-on-surface-variant);
	}
	textarea {
		font: var(--md-sys-typescale-body-large);
		color: var(--md-sys-color-on-surface);
		background: transparent;
		border: 1px solid var(--md-sys-color-outline);
		border-radius: var(--md-sys-shape-corner-extra-small, 4px);
		padding: 0.5rem 0.75rem;
	}
	blockquote {
		margin: 0.25rem 0 0;
		padding-left: 0.75rem;
		border-left: 3px solid var(--md-sys-color-outline-variant);
	}
</style>
