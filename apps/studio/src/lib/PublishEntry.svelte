<!--
  The editor's one next step and its More menu (D-184, after D-059 rule 2 and D-081). One primary
  per state: Publish, Publish again (taken down), Publish changes, or Ask for review when the role
  can't approve; Review the request while the entry waits in one; nothing when readers already
  have this version. `⋯ More` holds Copy public link, Duplicate, Technical details and, last and
  apart, Unpublish, which asks first ("Take “…” off your site?", Keep it live / Unpublish).

  Publishing makes a publish request for the saved version and sends it for review; when the
  project lets people approve their own requests (D-044) and the role may approve and publish, the
  same click approves it, recorded as a self-review, and publishes it on the release this page has
  seen (D-045). Every step is the ordinary API call; a refusal stops it and says so here, with a
  link to the request. What went well is a toast — the status card above already says where the
  entry stands, so it isn't said twice. Below xl its parts join the editor's bottom bar
  (`contents`): the buttons beside the status, a refusal on a row of its own.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { goto } from '$app/navigation';
	import { tick } from 'svelte';
	import CheckIcon from '@lucide/svelte/icons/check';
	import CopyIcon from '@lucide/svelte/icons/copy';
	import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
	import EyeOffIcon from '@lucide/svelte/icons/eye-off';
	import LinkIcon from '@lucide/svelte/icons/link';
	import ScrollTextIcon from '@lucide/svelte/icons/scroll-text';
	import { useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import * as AlertDialog from '@colophon/ui/ui/alert-dialog';
	import { Button, buttonVariants } from '@colophon/ui/ui/button';
	import * as DropdownMenu from '@colophon/ui/ui/dropdown-menu';
	import { type EntryState, mayOn, type Project } from './api.ts';
	import { type InReview, newKeys, type Outcome, publishesInOneClick, publishInOneClick } from './publishing.ts';
	import { PUBLISH_REQUEST, releaseName } from './words.ts';

	let {
		project,
		collection,
		entry,
		title,
		revision,
		mine,
		standing,
		review = null,
		reviewKnown = true,
		takenDown = false,
		saved,
		seenGeneration,
		own = true,
		linkedLive = [],
		published = false,
		duplicates = false,
		oncopy,
		onduplicate,
		ontechnical,
		onunpublished
	}: {
		project: string;
		collection: string;
		entry: string;
		/** The saved title, which names the request. */
		title: string;
		/** The saved version on screen. What happened to a version is said until a newer one is saved. */
		revision: string;
		mine: Project | undefined;
		standing: EntryState;
		/** The publish request it waits in, if one is waiting for review. */
		review?: InReview | null;
		/** Whether `review` is known yet: false while the reads that find the request still run. */
		reviewKnown?: boolean;
		/** A release took it down: readers no longer get it. */
		takenDown?: boolean;
		/** Whether the editor's content is saved: only a saved version can be published. */
		saved: boolean;
		/** The newest release this page has seen: publishing goes on top of it, never past one unseen. */
		seenGeneration: number | undefined;
		/** Whether the caller created this entry, for permissions limited to one's own entries. */
		own?: boolean;
		/** Published entries that link to this one, named: taking it down will be refused (D-065). */
		linkedLive?: string[];
		/** Whether readers have a version now, so there is a public link to copy. */
		published?: boolean;
		/** Whether the role may make a copy here. */
		duplicates?: boolean;
		oncopy: () => void;
		onduplicate: () => void;
		ontechnical: () => void;
		/** Taken down in one click: the editor offers Undo for a moment. */
		onunpublished?: (generation: number) => void;
	} = $props();

	const queryClient = useQueryClient();
	const oneClick = $derived(publishesInOneClick(mine, [collection]));
	const asks = $derived(mayOn(mine, 'content.update', collection, own));
	const acts = $derived(mine !== undefined && (oneClick || asks));

	let step = $state('');
	let result = $state<{ outcome: Outcome; revision: string } | null>(null);
	// What pressing did is about the version that was on screen then. Once a newer version is
	// saved it no longer applies, so a change made after publishing can be published too.
	const outcome = $derived(result !== null && result.revision === revision ? result.outcome : null);
	// Pressing again after a lost answer is safe: the same keys until the attempt ends.
	let keys = newKeys();
	const named = $derived(title || 'Untitled');

	/** Until the editor knows whether a request waits, no primary is offered and nothing is said about
	 *  what is left to publish: "Nothing new to publish", or a Publish that would make a second
	 *  request, could both be false. What pressing just did is known here, so it isn't held. */
	const held = $derived(!reviewKnown && outcome === null);
	/** The state the buttons answer, with what pressing just did folded in until the reads catch up. */
	const now = $derived.by((): 'publish' | 'review' | 'waiting' | 'nothing' => {
		if (outcome?.kind === 'requested' || (review !== null && outcome?.kind !== 'published' && outcome?.kind !== 'unpublished')) return oneClick && (review !== null || outcome?.kind === 'requested') ? 'review' : 'waiting';
		if (outcome?.kind === 'published') return 'nothing';
		if (outcome?.kind === 'unpublished') return 'publish';
		return standing === 'live' && saved ? 'nothing' : 'publish';
	});
	const requestId = $derived(outcome?.kind === 'requested' ? outcome.request : (review?.id ?? null));
	const gone = $derived(outcome?.kind === 'unpublished' || (takenDown && standing === 'draft'));
	const label = $derived(
		!oneClick ? 'Ask for review' : gone ? 'Publish again' : standing === 'changed' ? 'Publish changes' : 'Publish'
	);
	/** Readers have a version that Unpublish would take down, and no request to do so waits. */
	const takesDown = $derived(
		acts && !held && (standing !== 'draft' || outcome?.kind === 'published') && outcome?.kind !== 'unpublished' && !(review?.unpublish ?? false) && !(outcome?.kind === 'requested' && outcome.unpublish)
	);
	const nextRelease = $derived(seenGeneration === undefined ? 'the next release' : releaseName(seenGeneration + 1));

	let confirming = $state(false);
	let keep = $state<HTMLButtonElement | null>(null);

	/** Publishes the saved version, or with `unpublish`, takes the live one down: the same path,
	 *  a request whose one item unpublishes (D-049). Publishing it again puts it back. */
	async function go(unpublish = false) {
		// One attempt at a time, decided before anything awaits: a double click is one request.
		if (going) return;
		going = true;
		try {
			await attempt(unpublish);
		} finally {
			going = false;
		}
	}
	let going = false;

	async function attempt(unpublish: boolean) {
		result = null;
		const acting = revision;
		const done = await publishInOneClick({
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
		result = { outcome: done, revision: acting };
		step = '';
		keys = newKeys();
		// Said before the reads refresh, while the page still knows what readers had (Undo needs it).
		const receipt = (generation: number) => resolve('/projects/[project]/releases/[generation]', { project, generation: String(generation) });
		const request = (id: string) => resolve('/projects/[project]/changesets/[changeset]', { project, changeset: id });
		switch (done.kind) {
			case 'published':
				toast.success(`Published in ${releaseName(done.generation)}`, {
					description: 'Readers get this version now.',
					action: { label: 'See the receipt', onClick: () => void goto(receipt(done.generation)) }
				});
				break;
			case 'unpublished':
				onunpublished?.(done.generation);
				break;
			case 'requested':
				toast.success(done.unpublish ? 'Asked to take it off your site' : 'Sent for review', {
					description: 'Someone who can publish approves it.',
					action: { label: `See the ${PUBLISH_REQUEST}`, onClick: () => void goto(request(done.request)) }
				});
				break;
		}
		await Promise.all([
			queryClient.invalidateQueries({ queryKey: ['entries', project] }),
			queryClient.invalidateQueries({ queryKey: ['changesets', project] }),
			queryClient.invalidateQueries({ queryKey: ['releases', project] })
		]);
	}

	/** From the More menu: the confirmation opens once the menu has given focus back. */
	async function askFirst() {
		await tick();
		confirming = true;
	}
</script>

<div class="space-y-2 max-xl:contents" data-testid="publish-entry">
	{#if mine === undefined}
		<p class="text-sm text-muted-foreground xl:py-2">Checking what your role may do…</p>
	{:else if !(oneClick || asks)}
		<p class="text-sm text-muted-foreground xl:py-2">Your role can’t publish or ask for review here.</p>
	{/if}

	<!-- Side by side in the phone's bar and the desktop card: the one primary, then More. -->
	<div class="flex shrink-0 items-center gap-2 max-xl:ml-auto xl:w-full">
		{#if acts}
			{#if held}
				<p class="flex h-10 flex-1 items-center text-sm text-muted-foreground max-xl:hidden" aria-busy="true" data-testid="checking-review">Checking whether it waits for review…</p>
			{:else if now === 'publish'}
				<Button class="flex-1" onclick={() => go()} disabled={!saved || step !== ''} data-testid="publish-button">
					{step !== '' ? step : label}
				</Button>
			{:else if now === 'review' && requestId !== null}
				<Button class="flex-1" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: requestId })} data-testid="review-button"
					>{review?.runAt ? 'See the schedule' : 'Review the request'}</Button
				>
			{:else if now === 'waiting'}
				<p class="flex h-10 flex-1 items-center text-sm text-muted-foreground max-xl:hidden" data-testid="waiting-for-review">{review?.runAt ? 'Scheduled' : 'Waiting for review'}</p>
			{:else}
				<p class="flex h-10 flex-1 items-center gap-1.5 text-sm text-muted-foreground max-xl:hidden" data-testid="nothing-to-publish">
					<CheckIcon class="size-4 text-live" aria-hidden="true" />Nothing new to publish
				</p>
			{/if}
		{/if}
		<DropdownMenu.Root>
			<DropdownMenu.Trigger class={buttonVariants({ variant: 'secondary', size: 'icon' })} aria-label="More actions" title="More actions" data-testid="more-menu">
				<EllipsisIcon />
			</DropdownMenu.Trigger>
			<!-- Inside the page's main landmark, so a screen reader's landmarks still hold it. -->
			<DropdownMenu.Content align="end" class="w-56" portalProps={{ to: 'main' }}>
				{#if published || outcome?.kind === 'published'}
					<DropdownMenu.Item onSelect={oncopy} data-testid="copy-public-link"><LinkIcon />Copy public link</DropdownMenu.Item>
				{/if}
				{#if duplicates}
					<DropdownMenu.Item onSelect={onduplicate} data-testid="duplicate"><CopyIcon />Duplicate</DropdownMenu.Item>
				{/if}
				<DropdownMenu.Item onSelect={ontechnical} data-testid="technical-menu"><ScrollTextIcon />Technical details</DropdownMenu.Item>
				{#if takesDown}
					<DropdownMenu.Separator />
					<DropdownMenu.Item variant="destructive" onSelect={askFirst} disabled={step !== ''} data-testid="unpublish-button">
						<EyeOffIcon />{oneClick ? 'Unpublish…' : 'Ask to unpublish…'}
					</DropdownMenu.Item>
				{/if}
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	</div>

	{#if acts && !held && outcome?.kind !== 'stopped'}
		<!-- What pressing does, in one line; in the phone's bar the save status already says why it waits. -->
		<p class="hidden text-sm text-muted-foreground xl:block" data-testid="publish-hint">
			{#if now === 'publish' && !saved}
				Saving first…
			{:else if now === 'publish' && oneClick}
				Goes live at once, as {nextRelease}.
			{:else if now === 'publish'}
				Someone who can publish approves it, then it goes live.
			{:else if now === 'review' && review?.runAt}
				It publishes by itself then; change or cancel it there.
			{:else if now === 'review'}
				Approve it there to publish it.
			{:else if now === 'waiting'}
				Someone who can publish approves it.
			{/if}
		</p>
	{/if}

	{#if outcome?.kind === 'stopped'}
		<p role="alert" class="rounded-lg bg-attention-soft px-3 py-2 text-sm text-attention-ink max-xl:basis-full" data-testid="publish-outcome">
			Not published: {outcome.message}
			{#if outcome.request}
				<a class="font-medium underline underline-offset-2" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: outcome.request })}
					>Open the {PUBLISH_REQUEST}</a
				>
			{/if}
		</p>
	{/if}
</div>

<AlertDialog.Root bind:open={confirming}>
	<AlertDialog.Content
		data-testid="unpublish-dialog"
		onOpenAutoFocus={(event) => {
			// Keeping it live is the default: the focus starts there, not on the red button.
			event.preventDefault();
			keep?.focus();
		}}
	>
		<AlertDialog.Header>
			<AlertDialog.Title>{oneClick ? `Take “${named}” off your site?` : `Ask to take “${named}” off your site?`}</AlertDialog.Title>
			<AlertDialog.Description>
				{#if oneClick}
					Readers stop seeing it as soon as {nextRelease} goes out. You can undo it for a few seconds afterwards, or publish it again later.
				{:else}
					Someone who can publish approves it first; until then readers still see it.
				{/if}
			</AlertDialog.Description>
		</AlertDialog.Header>
		{#if linkedLive.length > 0}
			<p class="rounded-lg bg-attention-soft px-3 py-2 text-sm text-attention-ink" data-testid="unpublish-warning">
				{linkedLive.join(', ')}
				{linkedLive.length === 1 ? 'links' : 'link'} to it, and readers have {linkedLive.length === 1 ? 'it' : 'them'}. Colophon won’t take this down until
				{linkedLive.length === 1 ? 'that changes' : 'they change'}: change or take down {linkedLive.length === 1 ? 'that entry' : 'those entries'} first, or in the same
				{PUBLISH_REQUEST}.
			</p>
		{/if}
		<AlertDialog.Footer>
			<AlertDialog.Cancel bind:ref={keep} data-testid="keep-it-live">Keep it live</AlertDialog.Cancel>
			<AlertDialog.Action
				variant="destructive"
				onclick={() => {
					// The action doesn't close the dialog by itself; close it first, so its overlay
					// never sits over the toast that follows, and a second click finds nothing.
					confirming = false;
					void go(true);
				}}
				data-testid="unpublish-confirm"
			>
				{linkedLive.length > 0 ? 'Try anyway' : oneClick ? 'Unpublish' : 'Ask to unpublish'}
			</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
