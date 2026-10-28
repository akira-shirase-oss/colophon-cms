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
	import * as AlertDialog from '@colophon/ui/ui/alert-dialog';
	import { Button } from '@colophon/ui/ui/button';
	import { type EntryState, mayOn, type Project } from './api.ts';
	import { newKeys, type Outcome, publishesInOneClick, publishInOneClick } from './publishing.ts';
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
		seenGeneration,
		own = true,
		linkedLive = []
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
		/** Whether the caller created this entry, for permissions limited to one's own entries. */
		own?: boolean;
		/** Published entries that link to this one, named: taking it down will be refused (D-065). */
		linkedLive?: string[];
	} = $props();

	// Before an unpublish the server will refuse, say why and let the person decide (stale data can
	// be wrong, so trying stays possible).
	let warning = $state(false);

	const queryClient = useQueryClient();
	const oneClick = $derived(publishesInOneClick(mine, [collection]));
	const asks = $derived(mayOn(mine, 'content.update', collection, own));

	let step = $state('');
	let outcome = $state<Outcome | null>(null);
	// Pressing again after a lost answer is safe: the same keys until the attempt ends.
	let keys = newKeys();

	/** Publishes the saved version, or with `unpublish`, takes the live one down: the same path,
	 *  a request whose one item unpublishes (D-049). Publishing it again puts it back. */
	async function go(unpublish = false) {
		outcome = null;
		const named = title || 'Untitled';
		outcome = await publishInOneClick({
			project,
			title: unpublish ? `Unpublish: ${named}` : named,
			entries: unpublish ? [] : [entry],
			unpublish: unpublish ? [entry] : [],
			oneClick,
			seenGeneration,
			keys,
			step: (words) => (step = words),
			doing: oneClick ? (unpublish ? 'unpublish this' : 'publish this') : 'ask for review here'
		});
		step = '';
		keys = newKeys();
		await Promise.all([
			queryClient.invalidateQueries({ queryKey: ['entries', project] }),
			queryClient.invalidateQueries({ queryKey: ['changesets', project] }),
			queryClient.invalidateQueries({ queryKey: ['releases', project] })
		]);
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
	{:else if !(oneClick || asks)}
		<p class="text-xs text-muted-foreground">Your role can't publish or ask for review here.</p>
	{/if}

	{#if mine !== undefined && (oneClick || asks)}
		<!-- Side by side in the phone's bar, stacked in the desktop card. -->
		<div class="flex gap-2 xl:grid">
			{#if !(standing === 'live' && saved && outcome === null) && outcome?.kind !== 'published' && outcome?.kind !== 'requested'}
				<Button class="flex-1" onclick={() => go()} disabled={!saved || step !== ''} data-testid="publish-button">
					{step !== '' ? step : oneClick ? (standing === 'changed' ? 'Publish changes' : 'Publish') : 'Ask for review'}
				</Button>
			{/if}
			{#if (standing !== 'draft' || outcome?.kind === 'published') && outcome?.kind !== 'unpublished' && outcome?.kind !== 'requested'}
				<!-- Taking it down is the same path in reverse, and undone by publishing again (D-059, rule 6). -->
				<Button
					class="flex-1"
					variant="outline"
					onclick={() => (linkedLive.length > 0 ? (warning = true) : go(true))}
					disabled={step !== ''}
					data-testid="unpublish-button"
				>
					{oneClick ? 'Unpublish' : 'Ask to unpublish'}
				</Button>
			{/if}
		</div>
		{#if !(standing === 'live' && saved && outcome === null) && outcome === null}
			<p class="text-xs text-muted-foreground {saved ? 'hidden xl:block' : ''}">
				{#if !saved}
					Saving first…
				{:else if oneClick}
					Goes live for readers at once, as the next release. Your approval is recorded.
				{:else}
					Someone who can publish approves it and puts it live.
				{/if}
			</p>
		{/if}
	{/if}

	{#if linkedLive.length > 0 && standing !== 'draft' && outcome?.kind !== 'unpublished'}
		<p class="hidden text-xs text-muted-foreground xl:block" data-testid="unpublish-warning">
			Readers have {linkedLive.length === 1 ? 'an entry that links' : 'entries that link'} here, so it can't be taken down until {linkedLive.length === 1 ? 'that changes' : 'they change'}.
		</p>
	{/if}

	{#if outcome?.kind === 'unpublished'}
		<p role="status" class="rounded-lg bg-pending-soft px-3 py-2 text-sm text-pending-ink" data-testid="publish-outcome">
			Taken down in <a class="font-medium underline underline-offset-2" href={resolve('/projects/[project]/releases/[generation]', { project, generation: String(outcome.generation) })}
				>{releaseName(outcome.generation)}</a
			>. Readers no longer get it; publish it again to put it back.
		</p>
	{:else if outcome?.kind === 'published'}
		<p role="status" class="rounded-lg bg-live-soft px-3 py-2 text-sm text-live-ink" data-testid="publish-outcome">
			Live as <a class="font-medium underline underline-offset-2" href={resolve('/projects/[project]/releases/[generation]', { project, generation: String(outcome.generation) })}
				>{releaseName(outcome.generation)}</a
			>. Its receipt shows what reached your website.
		</p>
	{:else if outcome?.kind === 'requested'}
		<p role="status" class="rounded-lg bg-pending-soft px-3 py-2 text-sm text-pending-ink" data-testid="publish-outcome">
			{outcome.unpublish ? 'Asked to take it down.' : 'Sent for review.'}
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

<AlertDialog.Root bind:open={warning}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Readers still have links to this</AlertDialog.Title>
			<AlertDialog.Description>
				{linkedLive.join(', ')}
				{linkedLive.length === 1 ? 'links' : 'link'} to it, and readers have {linkedLive.length === 1 ? 'it' : 'them'}. Colophon won't take this down
				until {linkedLive.length === 1 ? 'that changes' : 'they change'}: change or take down {linkedLive.length === 1 ? 'that entry' : 'those entries'} first, or in
				the same publish request.
			</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Keep it</AlertDialog.Cancel>
			<AlertDialog.Action variant="outline" onclick={() => go(true)}>Try anyway</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
