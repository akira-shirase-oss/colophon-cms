<!--
  Publishing one entry from the editor (D-059, rule 2). One button makes a publish request for
  this entry's saved version and sends it for review; when the project lets people approve their
  own requests (D-044) and the role may approve and publish, the same click approves it, recorded
  as a self-review, and publishes it on the release this page has seen (D-045). Every step is the
  ordinary API call with its ordinary checks. If one is refused, it stops and says where things
  stand, with a link to the request; it never retries past a rule (another release first, a
  changed entry).
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { useQueryClient } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import {
		ApiError,
		changesetAction,
		createChangeset,
		type EntryState,
		listReleases,
		may,
		newKey,
		type Project,
		publishChangeset
	} from './api.ts';
	import { PUBLISH_REQUEST, releaseName } from './words.ts';

	let {
		project,
		collection,
		entry,
		title,
		mine,
		standing,
		saved,
		generation,
		seenGeneration
	}: {
		project: string;
		collection: string;
		entry: string;
		/** The saved title, which names the request. */
		title: string;
		mine: Project | undefined;
		standing: EntryState;
		/** Whether the editor's content is saved: only a saved version can be published. */
		saved: boolean;
		/** The release this entry is live in, if it is. */
		generation: number | null;
		/** The newest release this page has seen: publishing goes on top of it, never past one unseen. */
		seenGeneration: number | undefined;
	} = $props();

	const queryClient = useQueryClient();
	const oneClick = $derived(
		mine?.self_review === true && may(mine, 'content.publish', collection) && may(mine, 'content.review', collection)
	);
	const asks = $derived(may(mine, 'content.update', collection));

	type Outcome =
		| { kind: 'published'; generation: number }
		| { kind: 'requested'; request: string }
		| { kind: 'stopped'; message: string; request: string | null };
	let step = $state('');
	let outcome = $state<Outcome | null>(null);
	// One key for the request and one for publishing it: pressing again after a lost answer is safe.
	let requestKey = newKey();
	let publishKey = newKey();

	async function refresh() {
		await Promise.all([
			queryClient.invalidateQueries({ queryKey: ['entries', project] }),
			queryClient.invalidateQueries({ queryKey: ['changesets', project] }),
			queryClient.invalidateQueries({ queryKey: ['releases', project] })
		]);
	}

	async function go() {
		outcome = null;
		let request: string | null = null;
		try {
			step = `Making a ${PUBLISH_REQUEST}…`;
			let made = await createChangeset(project, title || 'Untitled', [entry], [], requestKey);
			request = made.id;
			if (made.status.state === 'open') {
				step = 'Sending it for review…';
				made = await changesetAction(project, made.id, made.hash, 'submit');
			}
			if (!oneClick) {
				outcome = { kind: 'requested', request: made.id };
				return;
			}
			if (made.status.state === 'awaiting_review') {
				step = 'Approving it…';
				made = await changesetAction(project, made.id, made.hash, 'reviews', {
					decision: 'approve',
					comment: 'Approved and published from the editor in one step.'
				});
			}
			step = 'Publishing…';
			const base = seenGeneration ?? (await listReleases(project)).generation;
			const published = await publishChangeset(project, made.id, made.hash, base, publishKey);
			outcome = { kind: 'published', generation: published.generation };
		} catch (failure) {
			const reason = failure instanceof ApiError ? failure.reason : null;
			const message =
				reason === 'generation_moved'
					? 'Another release went live while you were publishing. Your request is approved and waiting: look at what is live now, then publish it from the request.'
					: reason === 'stale_changeset'
						? 'The entry changed while it was being published. Open the request to update it to the latest version.'
						: failure instanceof ApiError
							? failure.message
							: String(failure);
			outcome = { kind: 'stopped', message, request };
		} finally {
			step = '';
			requestKey = newKey();
			publishKey = newKey();
			await refresh();
		}
	}
</script>

<div class="space-y-2" data-testid="publish-entry">
	{#if mine === undefined}
		<p class="text-xs text-muted-foreground">Checking what your role may do…</p>
	{:else if standing === 'live' && saved && outcome === null}
		<p class="text-sm" data-testid="publish-state">
			Readers see this version{#if generation !== null}, in
				<a class="underline underline-offset-2" href={resolve('/projects/[project]/releases/[generation]', { project, generation: String(generation) })}
					>{releaseName(generation)}</a
				>{/if}.
		</p>
	{:else if outcome?.kind === 'published' || outcome?.kind === 'requested'}
		<!-- Said below; pressing again would only make a second request. -->
	{:else if oneClick || asks}
		<Button class="w-full" onclick={go} disabled={!saved || step !== ''} data-testid="publish-button">
			{step !== '' ? step : oneClick ? (standing === 'changed' ? 'Publish changes' : 'Publish') : 'Ask for review'}
		</Button>
		<p class="text-xs text-muted-foreground">
			{#if !saved}
				Saving first…
			{:else if oneClick}
				Goes live for readers at once, as the next release. Your approval is recorded.
			{:else}
				Someone who can publish approves it and puts it live.
			{/if}
		</p>
	{:else}
		<p class="text-xs text-muted-foreground">Your role can't publish or ask for review here.</p>
	{/if}

	{#if outcome?.kind === 'published'}
		<p role="status" class="rounded-lg bg-live-soft px-3 py-2 text-sm text-live-ink" data-testid="publish-outcome">
			Live as <a class="font-medium underline underline-offset-2" href={resolve('/projects/[project]/releases/[generation]', { project, generation: String(outcome.generation) })}
				>{releaseName(outcome.generation)}</a
			>. Its receipt shows what reached your website.
		</p>
	{:else if outcome?.kind === 'requested'}
		<p role="status" class="rounded-lg bg-pending-soft px-3 py-2 text-sm text-pending-ink" data-testid="publish-outcome">
			Sent for review.
			<a class="font-medium underline underline-offset-2" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: outcome.request })}
				>See the {PUBLISH_REQUEST}</a
			>
		</p>
	{:else if outcome?.kind === 'stopped'}
		<p role="alert" class="rounded-lg bg-attention-soft px-3 py-2 text-sm text-attention-ink" data-testid="publish-outcome">
			Not published: {outcome.message}
			{#if outcome.request}
				<a class="font-medium underline underline-offset-2" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: outcome.request })}
					>Open the {PUBLISH_REQUEST}</a
				>
			{/if}
		</p>
	{/if}
</div>
