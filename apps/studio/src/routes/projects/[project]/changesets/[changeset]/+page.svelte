<!--
  One publish request (a changeset, D-044): the exact versions it holds and what each changes for
  readers, where it stands, and its one next step (lib/requestState.ts): prepared → Ask for review
  (who should look, P2-12); waiting → the reviewer approves or asks for changes, everyone else
  reads who it waits for; approved → Publish now, or Schedule…; scheduled → when, with Publish now
  and Cancel schedule; published → the release. One primary at a time, never three. Once it is
  approved, the review is one line, and approving your own request is a note, not a warning.
  Every action names the hash on screen, so nobody approves a version they didn't see: if the
  request changed meanwhile, the page shows the current one and says so. Where the environment
  names review stages (D-104), each one and where it stands is listed, every review names the
  stage it decides, and publishing waits for all of them. Internal names and hashes are in the
  Technical details.
-->
<script lang="ts">
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import MessageSquareIcon from '@lucide/svelte/icons/message-square';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Label } from '@colophon/ui/ui/label';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { Textarea } from '@colophon/ui/ui/textarea';
	import {
		ApiError,
		cancelSchedule,
		type Changeset,
		type ChangesetStage,
		changesetAction,
		createChangeset,
		getChangeset,
		allReleaseChanges,
		may,
		mayOn,
		newKey,
		publishChangeset,
		updateTask
	} from '#lib/api.ts';
	import { goto } from '$app/navigation';
	import { explainLinks, type Named } from '#lib/references.ts';
	import { plainError } from '#lib/errors.ts';
	import AskForReview from '#lib/AskForReview.svelte';
	import ChangesetPreview from '#lib/ChangesetPreview.svelte';
	import ItemDiff from '#lib/ItemDiff.svelte';
	import SchedulePanel, { readsShort, schedulesQuery } from '#lib/SchedulePanel.svelte';
	import { ago, meQuery, membersQuery, projectQuery, releasesQuery, tasksQuery, typesQuery } from '#lib/queries.ts';
	import { ACTION, actionsOf, names, nextStep, type Viewer } from '#lib/requestState.ts';
	import { originRelease, originText, releaseOrigin } from '#lib/releaseOrigin.ts';
	import ChangesetBadge from '#lib/shell/ChangesetBadge.svelte';
	import StatusBadge from '#lib/shell/StatusBadge.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';
	import { capital, PUBLISH_REQUEST, releaseName, REVIEW_STAGES, STAGE_STATE, VALUE_TAKEN_ON_PUBLISH } from '#lib/words.ts';

	const project = $derived(page.params.project ?? '');
	const id = $derived(page.params.changeset ?? '');
	const queryClient = useQueryClient();
	const set = createQuery(() => ({ queryKey: ['changeset', project, id], queryFn: () => getChangeset(project, id) }));
	// Undo, rollback or neither: undo first, since an undo also carries rollback_generation (D-184).
	const origin = $derived(set.data === undefined ? null : releaseOrigin(set.data));
	const originTarget = $derived(originRelease(origin));
	// Items this role can't read, or can't read the title of: it may not approve them (D-185).
	const blind = $derived(set.data !== undefined && (set.data.hidden_items > 0 || set.data.items.some((item) => item.title_hidden)));
	// The role comes from the project itself: me().projects lists at most 1,000 projects. Reviewing
	// and publishing need the permission in every collection the changeset touches (D-058).
	const mine = createQuery(() => projectQuery(project));
	const collections = $derived([...new Set((set.data?.items ?? []).map((item) => item.collection))]);
	const reviewer = $derived(collections.every((collection) => may(mine.data, 'content.review', collection)));
	const publisher = $derived(collections.every((collection) => may(mine.data, 'content.publish', collection)));
	// Sending for review and updating need edit rights on every entry in it: for a role whose edit
	// rights are only for its own entries, every entry must be the caller's (each item says whose).
	const who = createQuery(meQuery);
	const writer = $derived(
		mine.data !== undefined &&
			who.data != null &&
			(set.data?.items ?? []).every((item) => mayOn(mine.data, 'content.update', item.collection, item.created_by === who.data?.id))
	);
	const own = $derived(set.data?.submitted_by != null && set.data.submitted_by === who.data?.id);
	// Approving your own request is the project's choice (D-044); without it a reviewer's own request
	// waits for someone else.
	const decides = $derived(reviewer && set.data?.submitted_by != null && (!own || mine.data?.self_review === true));
	// Its schedules, read with the Schedule card's own key: one answer serves both.
	const schedules = createQuery(() => schedulesQuery(project, id));
	const pending = $derived(schedules.data?.find((item) => item.state === 'scheduled'));
	const cancels = $derived(publisher || (pending !== undefined && who.data != null && pending.scheduled_by === who.data.id));
	const viewer = $derived<Viewer>({ writer, decides, publisher, cancels });
	const step = $derived(set.data === undefined ? null : nextStep(set.data.status.state, pending !== undefined, viewer));
	const stepActions = $derived(step === null ? { primary: null, secondary: [] } : actionsOf(step, viewer));
	// Who it waits for: the review tasks sent with it (AskForReview), on its first entry.
	const first = $derived(set.data?.items[0]?.entry_id);
	const tasks = createQuery(() => ({ ...tasksQuery(project, { entry: first ?? '', status: 'open' }), enabled: first !== undefined }));
	const members = createQuery(() => membersQuery(project));
	const asked = $derived((tasks.data?.data ?? []).filter((task) => task.changeset_id === id && task.assignee_id !== null));
	const nameOf = (identity: string) =>
		identity === who.data?.id ? 'you' : (members.data?.find((member) => member.identity_id === identity)?.display_name ?? 'a member');
	const waitingFor = $derived(names([...new Set(asked.map((task) => task.assignee_id ?? ''))].map(nameOf)));
	// Publishing names the release it follows: the latest one this page has seen.
	const releases = createQuery(() => releasesQuery(project));
	const types = createQuery(() => typesQuery(project));
	const typeName = (collection: string) => types.data?.find((type) => type.slug === collection)?.definition.name ?? collection;
	// Once published, "what readers had" is the release's own record (its receipt), not what is
	// live now, which is this request's own versions.
	const generation = $derived(set.data?.status.state === 'published' ? set.data.status.generation : null);
	const releaseChanges = createQuery(() => ({
		queryKey: ['receipt', project, generation ?? 0, 'changes', 'all'],
		queryFn: () => allReleaseChanges(project, generation ?? 0),
		enabled: generation !== null
	}));
	const before = (entry: string, fallback: string | null) =>
		releaseChanges.data === undefined ? fallback : (releaseChanges.data.find((change) => change.entry_id === entry)?.previous_revision_id ?? null);
	// One key per publish attempt: pressing Publish now again after a lost answer is safe.
	let publishKey = newKey();
	// Which items show their comparison; the first few open by default.
	let open = $state<Record<string, boolean>>({});
	let scheduling = $state(false);
	let reviewing = $state(false);

	let comment = $state('');
	// The stage the next review decides (D-104): the first one still waiting, so several
	// approvals can be given in a row without picking each time.
	let deciding = $state('');
	const nextUndecided = (stages: ChangesetStage[]) => stages.find((stage) => stage.state !== 'approved')?.key ?? stages[0]?.key ?? '';
	/** The stage's name for people, or its key marked as gone once the workflow dropped it. */
	const stageName = (shown: Changeset, key: string) => {
		const stage = shown.stages.find((item) => item.key === key);
		return stage === undefined ? `${key} (removed)` : stage.name;
	};
	let error = $state('');
	/** Linked entries a refused publish named, which a new request could include. */
	let missing = $state<Named[]>([]);
	let notice = $state('');
	let busy = $state(false);
	const STALE = `This ${PUBLISH_REQUEST} changed since you loaded it. Here is the current one: check it, then try again.`;

	async function publish(shown: Changeset) {
		const base = releases.data?.generation;
		if (base === undefined) return;
		busy = true;
		error = '';
		notice = '';
		missing = [];
		try {
			await publishChangeset(project, id, shown.hash, base, publishKey);
			publishKey = newKey();
		} catch (failure) {
			const links = failure instanceof ApiError ? await explainLinks(project, id, failure.reason, types.data) : null;
			if (links !== null) {
				error = links.text;
				missing = links.missing.filter((entry) => !entry.title.startsWith('an entry'));
				publishKey = newKey();
				return;
			}
			if (failure instanceof ApiError && failure.reason === 'generation_moved') {
				notice = 'Another release was published first. Check what is live now, then publish again.';
				publishKey = newKey();
			} else if (failure instanceof ApiError && failure.reason === 'stale_changeset') {
				notice = STALE;
			} else if (failure instanceof ApiError && failure.reason === 'value_taken') {
				error = VALUE_TAKEN_ON_PUBLISH;
			} else {
				error = plainError(failure, 'publish this');
			}
		} finally {
			busy = false;
			await Promise.all([
				queryClient.invalidateQueries({ queryKey: ['changeset', project, id] }),
				queryClient.invalidateQueries({ queryKey: ['releases', project] }),
				queryClient.invalidateQueries({ queryKey: ['changesets', project] }),
				queryClient.invalidateQueries({ queryKey: ['entries', project] }),
				queryClient.invalidateQueries({ queryKey: ['schedules', project] })
			]);
		}
	}

	async function cancel() {
		if (pending === undefined) return;
		busy = true;
		error = '';
		try {
			await cancelSchedule(project, pending.id);
		} catch (failure) {
			error = plainError(failure, 'cancel this schedule');
		} finally {
			busy = false;
			await Promise.all([
				queryClient.invalidateQueries({ queryKey: ['schedules', project] }),
				queryClient.invalidateQueries({ queryKey: ['changeset', project, id] })
			]);
		}
	}

	/** A new request with this one's entries and the linked ones it was missing; it opens next. */
	async function includeLinked(shown: Changeset) {
		busy = true;
		try {
			const publishing = shown.items.filter((item) => item.kind === 'publish').map((item) => item.entry_id);
			const leaving = shown.items.filter((item) => item.kind === 'unpublish').map((item) => item.entry_id);
			const made = await createChangeset(project, `${shown.title}, with what it links to`, [...publishing, ...missing.map((entry) => entry.id)], leaving, newKey());
			missing = [];
			error = '';
			await queryClient.invalidateQueries({ queryKey: ['changesets', project] });
			await goto(resolve('/projects/[project]/changesets/[changeset]', { project, changeset: made.id }));
		} catch (failure) {
			error = plainError(failure, 'make this request');
		} finally {
			busy = false;
		}
	}

	async function act(shown: Changeset, action: 'refresh' | 'reviews', decision?: 'approve' | 'request_changes') {
		busy = true;
		error = '';
		notice = '';
		try {
			const body = decision === undefined ? undefined : { decision, comment, ...(deciding !== '' ? { stage: deciding } : {}) };
			const next = await changesetAction(project, id, shown.hash, action, body);
			queryClient.setQueryData(['changeset', project, id], next);
			if (decision !== undefined) {
				comment = '';
				reviewing = false;
				deciding = nextUndecided(next.stages);
				await closeOwnReviewTasks();
			}
		} catch (failure) {
			if (failure instanceof ApiError && failure.reason === 'stale_changeset') {
				notice = STALE;
			} else {
				error = plainError(failure, action === 'reviews' ? 'review this' : 'update it');
			}
			await queryClient.invalidateQueries({ queryKey: ['changeset', project, id] });
		} finally {
			busy = false;
			void queryClient.invalidateQueries({ queryKey: ['changesets', project] });
		}
	}

	/** A reviewer who decided has done what their review task asked: it is marked done. If that
	 *  fails the task stays open, where the Tasks page shows it; the review itself counted. */
	async function closeOwnReviewTasks() {
		for (const task of asked.filter((one) => one.assignee_id === who.data?.id)) {
			await updateTask(project, task.id, task.etag, { status: 'done' }, newKey()).catch(() => undefined);
		}
		await queryClient.invalidateQueries({ queryKey: ['tasks', project] });
	}

	// Keep the pick on a stage that exists; if the list or the request changed under it, move to
	// the first one still waiting (or the first, when all are decided).
	$effect(() => {
		const stages = set.data?.stages ?? [];
		if (stages.length > 0 && !stages.some((stage) => stage.key === deciding)) deciding = nextUndecided(stages);
	});

	const STAGE_BADGE = { pending: 'pending', approved: 'live', changes_requested: 'attention', approval_expired: 'attention' } as const;
	const select =
		'h-9 rounded-control border border-input bg-popover px-2 text-sm outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40 disabled:opacity-50';

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
	/** The approval that counts now: who gave it and when. */
	const approval = (shown: Changeset) => shown.reviews.find((review) => review.current && review.decision === 'approve');
	const at = (instant: string) => new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit' }).format(new Date(instant));
</script>

<svelte:head><title>{set.data?.title ?? 'Publish request'} · Colophon Studio</title></svelte:head>

{#if set.isPending}
	<Skeleton class="mb-8 h-10 w-1/2" />
	<Skeleton class="h-64 w-full rounded-xl" />
{:else if set.isError}
	<Problem error={set.error} />
{:else}
	{@const shown = set.data}
	{@const approved = approval(shown)}
	<PageHeader title={shown.title}>
		{#snippet meta()}
			<span data-testid="changeset-status" data-state={shown.status.state} data-step={step?.kind} class="inline-flex flex-wrap items-center gap-x-2 gap-y-1">
				{#if step?.kind === 'scheduled' && pending}
					<StatusBadge state="scheduled" testid="scheduled-badge" />
					<span class="text-foreground" data-testid="publishes-at">Publishes {readsShort(pending.run_at, pending.time_zone)}</span>
				{:else}
					<ChangesetBadge state={shown.status.state} />
				{/if}
				{#if origin !== null && originTarget !== null}
					<!-- Undo before rollback: an undo of release N also carries rollback_generation N-1 (D-184). -->
					<a class="underline underline-offset-2" href={resolve('/projects/[project]/releases/[generation]', { project, generation: String(originTarget) })} data-testid="restores-link" data-origin={origin.kind}
						>{originText(origin)}</a
					>
				{/if}
				{#if step?.kind === 'waiting' || (step?.kind === 'decide' && shown.status.state === 'awaiting_review')}
					<span data-testid="waiting-for">{waitingFor === '' ? 'Waiting for someone who can approve it' : `Waiting for ${waitingFor}`}</span>
				{:else if shown.status.state === 'changes_requested'}
					<span>by {shown.status.by}</span>
				{:else if shown.status.state === 'published'}
					<span>as <a class="underline underline-offset-2" href={resolve('/projects/[project]/releases/[generation]', { project, generation: String(shown.status.generation) })} data-testid="release-link">{releaseName(shown.status.generation ?? 0)}</a></span>
				{/if}
			</span>
		{/snippet}
		{#snippet actions()}
			{#if step?.kind === 'cannot-send' || step?.kind === 'cannot-update'}
				<p class="max-w-xs text-sm text-muted-foreground" data-testid="cannot-send">
					{step.kind === 'cannot-send' ? 'Someone who can edit these entries sends it for review.' : 'Someone who can edit these entries updates it.'}
				</p>
			{:else if step?.kind === 'ask-review'}
				<AskForReview
					{project}
					changeset={shown}
					disabled={busy}
					onsent={(next, warning) => {
						queryClient.setQueryData(['changeset', project, id], next);
						notice = warning;
						void queryClient.invalidateQueries({ queryKey: ['changesets', project] });
					}}
					onerror={(failure) => {
						if (failure instanceof ApiError && failure.reason === 'stale_changeset') notice = STALE;
						else error = plainError(failure, 'ask for review here');
						void queryClient.invalidateQueries({ queryKey: ['changeset', project, id] });
					}}
				/>
			{:else if step?.kind === 'update'}
				<Button onclick={() => act(shown, 'refresh')} disabled={busy}>{ACTION.update}</Button>
			{:else if step?.kind === 'publish'}
				<Button variant="outline" onclick={() => (scheduling = true)} disabled={busy || scheduling} data-testid="schedule-open">{ACTION.schedule}</Button>
				<Button onclick={() => publish(shown)} disabled={busy || releases.data === undefined}>{ACTION.publishNow}</Button>
			{:else if step?.kind === 'scheduled'}
				{#each stepActions.secondary as name (name)}
					{#if name === ACTION.publishNow}
						<Button variant="outline" onclick={() => publish(shown)} disabled={busy || releases.data === undefined}>{name}</Button>
					{:else}
						<Button variant="outline" onclick={cancel} disabled={busy} data-testid="cancel-schedule">{name}</Button>
					{/if}
				{/each}
			{:else if step?.kind === 'approved'}
				<p class="max-w-xs text-sm text-muted-foreground" data-testid="cannot-publish">Approved. Someone who can publish it puts it live.</p>
			{:else if step?.kind === 'changes-asked'}
				<p class="max-w-xs text-sm text-muted-foreground" data-testid="changes-asked">Edit the entries, then update this request; it goes back for review.</p>
			{/if}
		{/snippet}
	</PageHeader>

	<ol class="mb-8 grid grid-cols-4 gap-2" aria-label="Progress">
		{#each STEPS as stepName, index (stepName)}
			<li class="grid gap-1.5" aria-current={index === reached(shown) - 1 ? 'step' : undefined}>
				<span class="h-1 rounded-full {index < reached(shown) ? 'bg-brand' : 'bg-border'}"></span>
				<span class="text-xs {index < reached(shown) ? 'text-foreground' : 'text-muted-foreground'}">{stepName}</span>
			</li>
		{/each}
	</ol>

	<!-- Exactly these versions on the editor's own site, in a new tab (gap/preview). -->
	<ChangesetPreview {project} changeset={shown.id} published={shown.status.state === 'published'} manages={may(mine.data, 'destinations.manage')} />

	{#if notice}<p role="status" class="mb-4 rounded-lg bg-pending-soft px-4 py-3 text-sm text-pending-ink" data-testid="changeset-notice">{notice}</p>{/if}
	{#if error}
		<div role="alert" class="mb-4 rounded-lg bg-destructive/10 px-4 py-3 text-sm text-destructive" data-testid="changeset-error">
			<p>{error}</p>
			{#if missing.length > 0}
				<Button class="mt-3" variant="outline" size="sm" disabled={busy} onclick={() => includeLinked(shown)} data-testid="include-linked">
					Make a {PUBLISH_REQUEST} that includes {missing.length === 1 ? missing[0]?.title : 'them'}
				</Button>
			{/if}
		</div>
	{/if}

	<div class="grid grid-cols-[minmax(0,1fr)] gap-10 lg:grid-cols-[minmax(0,1fr)_20rem]">
		<section aria-labelledby="items-heading">
			<h2 id="items-heading" class="mb-3 text-base font-semibold">Entries <span class="font-normal text-muted-foreground tabular">{shown.items.length + shown.hidden_items}</span></h2>
			{#if shown.hidden_items > 0}
				<p class="mb-3 text-sm text-muted-foreground" data-testid="hidden-items">
					{shown.hidden_items === 1 ? 'One entry' : `${shown.hidden_items} entries`} in it {shown.hidden_items === 1 ? "isn't" : "aren't"} shown: your role can't open {shown.hidden_items === 1 ? 'it' : 'them'}.
				</p>
			{/if}
			<ul class="space-y-3" data-testid="changeset-items">
				{#each shown.items as item, index (item.entry_id)}
					{@const expanded = open[item.entry_id] ?? index < 3}
					{@const had = generation === null ? item.published_revision_id : before(item.entry_id, item.published_revision_id)}
					<li class="overflow-hidden rounded-xl border border-border bg-card">
						<div class="flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-3">
							<span class="min-w-0 flex-1">
								{#if item.title_hidden}
									<span class="block truncate font-medium text-muted-foreground italic" data-testid="title-hidden">Title hidden for your role</span>
								{:else}
									<a class="block truncate font-medium hover:underline" href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: item.collection, entry: item.entry_id })} title={item.title} data-user-content
										>{item.title}</a
									>
								{/if}
								<span class="text-xs text-muted-foreground">{typeName(item.collection)}</span>
							</span>
							{#if item.kind === 'unpublish'}
								<StatusBadge state="taken_down" testid="unpublish" />
							{:else if had === null}
								<StatusBadge state="new" testid="item-new" />
							{:else}
								<StatusBadge state="updated" testid="item-updated" />
							{/if}
							{#if item.edited && shown.status.state !== 'published'}
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
								{item.kind === 'unpublish' ? 'What goes' : had === null ? 'Preview' : 'Changes'}<ChevronDownIcon class="transition-transform {expanded ? 'rotate-180' : ''}" />
							</Button>
						</div>
						{#if expanded}
							<div id="diff-{item.entry_id}" class="unfold border-t border-dotted border-border bg-background/60 px-4 py-4">
								{#if generation !== null && releaseChanges.data === undefined && !releaseChanges.isError}
									<p class="text-sm text-muted-foreground">Loading the comparison…</p>
								{:else}
									<ItemDiff {project} item={{ ...item, published_revision_id: had }} done={shown.status.state === 'published'} />
								{/if}
							</div>
						{/if}
					</li>
				{/each}
			</ul>
		</section>

		<!-- Beside the entries on wide screens: Review, then Schedule, then History. Below lg its
		     parts join the page's column, and what asks for action (deciding, scheduling) moves to
		     the top, so a reviewer on a phone finds Approve without scrolling past every comparison
		     (D-059, rule 7). -->
		<div class="contents lg:block lg:space-y-8">
			{#if step?.kind === 'decide' || (reviewing && reviewer && shown.status.state === 'approved')}
				<section class="-order-1 rounded-xl border border-border bg-card p-4 lg:order-none" aria-label="Review" data-testid="review-panel">
					<h2 class="text-base font-semibold">Review</h2>
					<p class="mt-1 text-sm text-muted-foreground">
						You approve exactly the {shown.items.length === 1 ? 'version' : 'versions'} on this page<span class="lg:hidden">: the changes are below</span>. If
						{shown.items.length === 1 ? 'it changes' : 'they change'}, your approval no longer counts.
					</p>
					{#if shown.stages.length > 0}
						<div class="mt-4 grid gap-1.5">
							<Label for="review-stage">Deciding</Label>
							<select id="review-stage" class={select} bind:value={deciding} disabled={busy} data-testid="review-stage">
								{#each shown.stages as stage (stage.key)}
									<option value={stage.key} data-user-content
										>{stage.name} — {STAGE_STATE[stage.state].toLowerCase()}{stage.different_reviewer && own ? ', someone other than you' : ''}</option
									>
								{/each}
							</select>
						</div>
					{/if}
					<div class="mt-4 grid gap-1.5">
						<Label for="review-comment">Comment (optional)</Label>
						<Textarea id="review-comment" bind:value={comment} rows={3} maxlength={2000} />
					</div>
					<div class="mt-3 grid gap-2">
						{#if step?.kind === 'decide' && blind}
							<!-- An approval covers every version; the server refuses one made unseen (D-185). -->
							<p class="text-sm text-muted-foreground" data-testid="cannot-approve-hidden">Your role can't read everything this request publishes, so you can ask for changes but not approve it.</p>
						{:else if step?.kind === 'decide'}
							<Button onclick={() => act(shown, 'reviews', 'approve')} disabled={busy}>{ACTION.approve}</Button>
						{/if}
						<Button variant="outline" onclick={() => act(shown, 'reviews', 'request_changes')} disabled={busy}>{ACTION.requestChanges}</Button>
						{#if reviewing}<Button variant="ghost" onclick={() => (reviewing = false)} disabled={busy}>Keep the approval</Button>{/if}
					</div>
				</section>
			{:else if shown.status.state === 'approved' && approved}
				<!-- Approved: the review is one line now; the decision is history, the next step is above. -->
				<section class="rounded-xl border border-border bg-card px-4 py-3 text-sm" aria-label="Review" data-testid="review-done">
					<p class="flex flex-wrap items-center gap-x-1.5 gap-y-1">
						<CircleCheckIcon class="size-4 shrink-0 text-live-ink" aria-hidden="true" />
						<span>Approved by {approved.reviewer_id === who.data?.id ? 'you' : (approved.reviewer_name ?? 'a reviewer')}</span>
						<span class="text-muted-foreground" title={new Date(approved.created_at).toLocaleString()}>· {at(approved.created_at)}</span>
						{#if reviewer}
							<span class="text-muted-foreground">·</span>
							<Button variant="link" size="xs" class="h-auto p-0" onclick={() => (reviewing = true)} data-testid="ask-changes-instead">Ask for changes instead</Button>
						{/if}
					</p>
					{#if shown.status.self_review}
						<p class="mt-1.5 text-muted-foreground" data-testid="self-review">
							{approved.reviewer_id === who.data?.id ? 'You approved your own request' : `${approved.reviewer_name ?? 'The sender'} approved their own request`}: this project allows it.
						</p>
					{/if}
				</section>
			{/if}
			{#if shown.stages.length > 0}
				<!-- Each stage and where it stands (D-104): all of them approve — in any order —
				     before this can publish; changes asked for at one stage block it. -->
				<section aria-labelledby="stages-heading" data-testid="review-stages">
					<h2 id="stages-heading" class="mb-3 text-base font-semibold">{capital(REVIEW_STAGES)}</h2>
					<ol class="space-y-2.5">
						{#each shown.stages as stage, index (stage.key)}
							<li class="flex flex-wrap items-center gap-x-2.5 gap-y-1" data-testid="stage" data-state={stage.state}>
								<span class="text-xs text-muted-foreground tabular">{index + 1}.</span>
								<span class="text-sm font-medium" data-user-content>{stage.name}</span>
								<Badge variant={STAGE_BADGE[stage.state]}>{STAGE_STATE[stage.state]}</Badge>
								{#if stage.different_reviewer}
									<span class="text-xs text-muted-foreground">needs {own ? 'someone other than you' : 'someone other than whoever sent it'}</span>
								{/if}
								{#if stage.by_name !== null || stage.by !== null}
									<span class="text-xs text-muted-foreground" data-user-content>
										by {stage.by_name ?? stage.by}{#if stage.at !== null}
											· <span title={new Date(stage.at).toLocaleString()}>{ago(stage.at)}</span>{/if}
									</span>
								{/if}
							</li>
						{/each}
					</ol>
				</section>
			{/if}
			<div class={scheduling ? '-order-1 lg:order-none' : ''}>
				<SchedulePanel {project} changeset={shown} canSchedule={publisher} bind:form={scheduling} />
			</div>

			<section aria-labelledby="history-heading">
				<h2 id="history-heading" class="mb-3 text-base font-semibold">History</h2>
				<ol class="space-y-3 border-l border-dotted border-border pl-5" data-testid="changeset-reviews">
					{#each shown.reviews as review (review.id)}
						<li class="relative text-sm" class:opacity-70={!review.current}>
							<span class="absolute top-1.5 -left-[1.6rem] size-2.5 rounded-full border-2 border-background {review.decision === 'approve' ? 'bg-live' : 'bg-attention'}" aria-hidden="true"></span>
							<p>
								<strong class="font-medium">{review.decision === 'approve' ? 'Approved' : 'Changes requested'}</strong>
								{#if review.stage !== null}for <span data-user-content>{stageName(shown, review.stage)}</span> {/if}by {review.reviewer_name ?? review.reviewer_id}{#if review.decision === 'approve' && review.reviewer_id === shown.submitted_by}<span
										class="text-muted-foreground"
										data-testid="self-review-note">, who sent it</span
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
							<p>Sent for review{#if asked.length > 0}<span class="text-muted-foreground">, to {waitingFor}</span>{/if}</p>
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
