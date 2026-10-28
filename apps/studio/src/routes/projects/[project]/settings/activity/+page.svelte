<!--
  Activity (step R.4's audit log, D-059's words): who changed people's access, and when. Each
  change is a sentence, newest first, by day; filters narrow it to one kind of change or one
  person; "Show older" follows the server's cursor. The log can't be edited or deleted by anyone.
  Only roles that may see it (owners, by default) get its contents. One of Settings' pages (D-081).
-->
<script lang="ts">
	import HistoryIcon from '@lucide/svelte/icons/history';
	import { page } from '$app/state';
	import { createInfiniteQuery, createQuery, keepPreviousData } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { type AuditEvent, listAudit, listMembers, may, type Page } from '#lib/api.ts';
	import { KINDS, type Names, sentence } from '#lib/activity.ts';
	import { ago, projectQuery, rolesQuery, typesQuery } from '#lib/queries.ts';
	import SettingsFrame from '#lib/SettingsFrame.svelte';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';

	/** Events per page: "Show older" fetches the next. */
	const PAGE = 25;

	const project = $derived(page.params.project ?? '');
	const mine = createQuery(() => projectQuery(project));
	const reads = $derived(may(mine.data, 'audit.read'));
	const roles = createQuery(() => ({ ...rolesQuery(project), enabled: reads }));
	const types = createQuery(() => ({ ...typesQuery(project), enabled: reads }));
	const members = createQuery(() => ({ queryKey: ['members', project], queryFn: () => listMembers(project), enabled: reads }));

	let action = $state('');
	let actor = $state('');
	const events = createInfiniteQuery(() => ({
		queryKey: ['audit', project, action, actor],
		queryFn: ({ pageParam }: { pageParam: string | null }) => listAudit(project, { cursor: pageParam, action, actor, limit: PAGE }),
		initialPageParam: null as string | null,
		getNextPageParam: (last: Page<AuditEvent>) => last.next_cursor,
		placeholderData: keepPreviousData,
		enabled: reads
	}));
	const loaded = $derived((events.data?.pages ?? []).flatMap((one) => one.data));

	// Names for the sentences: people from the members list and from every actor loaded (someone who
	// left is often named by their own earlier events); roles as they are now, else as last recorded;
	// invitations by the label they were made with.
	const people = $derived.by(() => {
		const map = new Map<string, string>();
		for (const event of loaded) if (event.actor.kind === 'person' && event.actor.name) map.set(event.actor.id, event.actor.name);
		for (const member of members.data ?? []) if (member.display_name) map.set(member.identity_id, member.display_name);
		return map;
	});
	const recordedRoles = $derived.by(() => {
		const map = new Map<string, string>();
		for (const event of loaded) {
			const name = event.detail['name'] ?? (event.detail['after'] as Record<string, unknown> | undefined)?.['name'];
			if (event.action.startsWith('role.') && typeof name === 'string') map.set(event.subject, name);
		}
		return map;
	});
	const labels = $derived(
		new Map(loaded.filter((event) => event.action === 'invitation.created').map((event) => [event.subject, String(event.detail['label'] ?? '')]))
	);
	const names: Names = $derived({
		person: (id) => people.get(id),
		role: (key) => roles.data?.find((role) => role.key === key)?.name ?? recordedRoles.get(key) ?? key,
		invitation: (id) => labels.get(id),
		types: types.data ?? []
	});

	/** The events grouped by day, newest first: "Today", "Yesterday", or the date. */
	const days = $derived.by(() => {
		const groups: { day: string; events: AuditEvent[] }[] = [];
		const today = new Date().toDateString();
		const yesterday = new Date(Date.now() - 86_400_000).toDateString();
		for (const event of loaded) {
			const date = new Date(event.at);
			const key = date.toDateString();
			const day =
				key === today ? 'Today' : key === yesterday ? 'Yesterday' : date.toLocaleDateString(undefined, { weekday: 'long', day: 'numeric', month: 'long', year: 'numeric' });
			const last = groups.at(-1);
			if (last?.day === day) last.events.push(event);
			else groups.push({ day, events: [event] });
		}
		return groups;
	});
	const select =
		'h-9 w-full rounded-md border border-input bg-card px-3 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40 sm:w-60';
</script>

<svelte:head><title>Activity · Colophon Studio</title></svelte:head>

<SettingsFrame {project} current="activity">
	<PageHeader title="Activity">
		{#snippet description()}
			Who changed roles, members, invitations and API tokens, and when. Nobody can edit or delete this record, not even an owner.
		{/snippet}
	</PageHeader>

	{#if mine.isError}
		<Problem error={mine.error} what="this project" retry={() => mine.refetch()} />
	{:else if mine.isPending}
		<Skeleton class="h-40 w-full max-w-3xl rounded-xl" />
	{:else if !reads}
		<p class="max-w-3xl rounded-lg border border-border bg-card px-4 py-3 text-sm text-muted-foreground" data-testid="activity-hidden">
			Your role can't see the activity log. Owners can.
		</p>
	{:else}
		<div class="mb-6 flex max-w-3xl flex-col gap-3 sm:flex-row sm:items-end" role="search" aria-label="Narrow the activity">
			<label class="grid gap-1.5 text-sm">
				<span class="font-medium">Kind of change</span>
				<select class={select} bind:value={action} data-testid="activity-kind">
					<option value="">Every kind</option>
					{#each KINDS as kind (kind.action)}<option value={kind.action}>{kind.label}</option>{/each}
				</select>
			</label>
			<label class="grid gap-1.5 text-sm">
				<span class="font-medium">Person</span>
				<select class={select} bind:value={actor} data-testid="activity-person">
					<option value="">Anyone</option>
					{#each members.data ?? [] as member (member.identity_id)}<option value={member.identity_id}>{member.display_name ?? 'Someone without a name'}</option>{/each}
				</select>
			</label>
			{#if action !== '' || actor !== ''}
				<Button
					variant="ghost"
					size="sm"
					onclick={() => {
						action = '';
						actor = '';
					}}>Show everything</Button
				>
			{/if}
		</div>

		{#if events.isPending}
			<Skeleton class="h-48 w-full max-w-3xl rounded-xl" />
		{:else if events.isError}
			<div class="max-w-3xl"><Problem error={events.error} what="the activity log" retry={() => events.refetch()} /></div>
		{:else if loaded.length === 0}
			<div class="max-w-3xl">
				<EmptyState icon={HistoryIcon} title={action !== '' || actor !== '' ? 'Nothing matches' : 'No changes yet'} testid="activity-empty">
					{action !== '' || actor !== ''
						? 'No change of this kind, or by this person, is recorded. Show everything to see the rest.'
						: 'When someone changes a role, invites or removes someone, or makes or revokes an API token, it appears here.'}
				</EmptyState>
			</div>
		{:else}
			<div class="max-w-3xl space-y-6" class:opacity-60={events.isPlaceholderData} data-testid="activity">
				{#each days as group (group.day)}
					<section aria-label={group.day}>
						<h2 class="mb-2 text-sm font-semibold text-muted-foreground">{group.day}</h2>
						<ol class="overflow-hidden rounded-xl border border-border bg-card">
							{#each group.events as event (event.id)}
								<li class="rule-dotted flex flex-wrap items-baseline gap-x-4 gap-y-1 px-4 py-3 text-sm last:border-b-0" data-testid="activity-event" data-action={event.action}>
									<span class="min-w-0 flex-1">{sentence(event, names)}</span>
									<time class="text-xs text-muted-foreground" datetime={event.at} title={new Date(event.at).toLocaleString()}>{ago(event.at)}</time>
								</li>
							{/each}
						</ol>
					</section>
				{/each}
			</div>
			{#if events.hasNextPage || events.isFetchingNextPage || events.isFetchNextPageError}
				<div class="mt-4 flex flex-wrap items-center gap-3">
					<Button variant="outline" size="sm" onclick={() => events.fetchNextPage()} disabled={events.isFetchingNextPage} data-testid="activity-older">
						{events.isFetchingNextPage ? 'Loading older changes…' : 'Show older'}
					</Button>
					{#if events.isFetchNextPageError}<p role="alert" class="text-sm text-destructive">Couldn't load older changes. Try again.</p>{/if}
				</div>
			{/if}
		{/if}
		<TechnicalDetails class="mt-10 max-w-3xl">
			<p>This is the project's audit log: <span class="font-mono">GET /api/v1/projects/{project}/audit</span>, which needs <span class="font-mono">audit.read</span>.</p>
			<p>The database refuses every update and delete of it, from the application and from the tables' owner.</p>
		</TechnicalDetails>
	{/if}
</SettingsFrame>
