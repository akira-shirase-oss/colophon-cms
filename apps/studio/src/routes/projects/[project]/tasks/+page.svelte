<!--
	Tasks on a project's entries (E1, D-102): what has to be done before it's ready, who it's
	waiting on, and where it stands. A task may name the exact version or publish request it's
	about, and its comments are the entry's — the same list the entry's editor shows. Waiting on
	nobody is sent as `assignee: null`, never left out; every change binds the task's etag, and a
	`task_moved` answer refetches before it complains.
-->
<script lang="ts">
	import ListTodoIcon from '@lucide/svelte/icons/list-todo';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQueries, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Input } from '@colophon/ui/ui/input';
	import * as NativeSelect from '@colophon/ui/ui/native-select';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import * as Table from '@colophon/ui/ui/table';
	import * as Tabs from '@colophon/ui/ui/tabs';
	import {
		ApiError,
		type ChangesetSummary,
		createTask,
		type EditorialTask,
		getEntry,
		getTask,
		listChangesets,
		listContentTypes,
		listEntries,
		listRevisions,
		may,
		mayOn,
		newKey,
		type Revision,
		type Summary,
		type TaskUpdate,
		updateTask
	} from '#lib/api.ts';
	import CommentsPanel from '#lib/CommentsPanel.svelte';
	import { plainError } from '#lib/errors.ts';
	import { ago, meQuery, membersQuery, projectQuery, tasksQuery } from '#lib/queries.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import FormRow from '#lib/shell/FormRow.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';
	import { PUBLISH_REQUESTS, shownTitle } from '#lib/words.ts';

	const project = $derived(page.params.project ?? '');
	const queryClient = useQueryClient();
	const who = createQuery(meQuery);
	const people = createQuery(() => membersQuery(project));
	const mine = createQuery(() => projectQuery(project));
	/** Who someone is: their name in the project, "you", or honest words for someone who left. */
	const person = (id: string | null) => {
		if (id === null) return 'Nobody';
		if (id === who.data?.id) return 'You';
		const member = people.data?.find((option) => option.identity_id === id);
		return member?.display_name ?? 'Someone who left';
	};

	// Making or changing a task needs `content.update` on its entry; the server checks each one.
	const makes = $derived(mine.data === undefined || may(mine.data, 'content.update'));

	let status = $state<'all' | 'open' | 'done' | 'cancelled'>('all');
	let waiting = $state<'any' | 'me' | 'none'>('any');
	const filter = $derived({
		status: status === 'all' ? undefined : status,
		assignee: waiting === 'any' ? undefined : waiting === 'me' ? 'me' : 'none'
	});
	const tasks = createQuery(() => tasksQuery(project, filter));

	// What each task is on: the collections it names, read once each for their entries' titles.
	const collections = $derived([...new Set((tasks.data?.data ?? []).map((task) => task.collection))]);
	const titleReads = createQueries(() => ({
		queries: collections.map((collection) => ({
			queryKey: ['task-entries', project, collection],
			queryFn: () => listEntries(project, collection).catch(() => [] as Summary[])
		}))
	}));
	const titles = $derived.by(() => {
		const map = new Map<string, string>();
		titleReads.forEach((read, index) => {
			const collection = collections[index];
			read.data?.forEach((summary) => map.set(`${collection}/${summary.id}`, shownTitle(summary.revision.title)));
		});
		return map;
	});
	/** The entry's title, "a deleted entry" once its collection's read says it's gone, else "…". */
	const onWhat = (task: EditorialTask) => {
		const known = titles.get(`${task.collection}/${task.entry_id}`);
		if (known !== undefined) return known;
		return titleReads[collections.indexOf(task.collection)]?.isPending === false ? 'A deleted entry' : '…';
	};

	let error = $state('');
	let busy = $state(false);
	async function act(work: () => Promise<void>, doing = 'make this change'): Promise<boolean> {
		busy = true;
		error = '';
		try {
			await work();
			return true;
		} catch (failure) {
			error = plainError(failure, doing);
			return false;
		} finally {
			busy = false;
		}
	}
	const refresh = () => queryClient.invalidateQueries({ queryKey: ['tasks', project] });

	// ---- New task --------------------------------------------------------------
	let creating = $state(false);
	// Every entry the role may update, by type, to pick the task's entry from (the server still
	// checks `content.update` on it).
	const choices = createQuery(() => ({
		queryKey: ['task-choices', project],
		queryFn: async () => {
			const types = (await listContentTypes(project)).filter((type) => may(mine.data, 'content.update', type.slug));
			return Promise.all(types.map(async (type) => ({ type, entries: await listEntries(project, type.slug).catch(() => [] as Summary[]) })));
		},
		enabled: creating && mine.data !== undefined
	}));
	let title = $state('');
	let pickedEntry = $state(''); // "collection/entry"
	let pickedPerson = $state(''); // '' = nobody
	let pickedRevision = $state(''); // '' = no version named
	let pickedRequest = $state(''); // '' = no publish request named
	// The picked entry's versions and the project's publish requests, for the optional links.
	const versions = createQuery(() => ({
		queryKey: ['task-versions', project, pickedEntry],
		queryFn: async (): Promise<Revision[]> => {
			const [collection, entry] = pickedEntry.split('/');
			if (collection === undefined || entry === undefined) return [];
			return listRevisions(project, collection, entry).catch(() => [] as Revision[]);
		},
		enabled: creating && pickedEntry !== ''
	}));
	const requests = createQuery(() => ({
		queryKey: ['changesets', project],
		queryFn: () => listChangesets(project),
		enabled: creating
	}));
	const request = (id: string): ChangesetSummary | undefined => requests.data?.find((item) => item.id === id);
	// One key per task being made: pressing Create again after a lost answer is safe.
	let createKey = newKey();

	async function create(event: SubmitEvent) {
		event.preventDefault();
		const [collection, entry] = pickedEntry.split('/');
		if (collection === undefined || entry === undefined || title.trim() === '') return;
		const ok = await act(async () => {
			const made = await createTask(
				project,
				{
					entry,
					title: title.trim(),
					...(pickedPerson !== '' ? { assignee: pickedPerson } : {}),
					...(pickedRevision !== '' ? { revision: pickedRevision } : {}),
					...(pickedRequest !== '' ? { changeset: pickedRequest } : {})
				},
				createKey
			);
			createKey = newKey();
			creating = false;
			open = made;
		}, 'create this task');
		if (ok) await refresh();
	}
	function openCreate(next: boolean) {
		creating = next;
		if (next) {
			title = '';
			pickedEntry = '';
			pickedPerson = '';
			pickedRevision = '';
			pickedRequest = '';
			error = '';
		}
	}

	// ---- The task itself -------------------------------------------------------
	let open = $state<EditorialTask | null>(null);
	// Its entry, for the comments' version and creator (and to know when it's deleted): a read that
	// ends null either way the read fails.
	const openEntry = createQuery(() => ({
		queryKey: ['task-entry', project, open?.collection ?? '', open?.entry_id ?? ''],
		queryFn: async () => {
			if (open === null) return null;
			try {
				return (await getEntry(project, open.collection, open.entry_id)).draft;
			} catch {
				return null;
			}
		},
		enabled: open !== null
	}));
	const entryGone = $derived(open !== null && openEntry.data === null && !openEntry.isPending);
	// Comments on the task's entry ask for `content.update` on it — with the entry gone, only the
	// collection-level grant can be checked (its creator isn't in the task read).
	const canDiscuss = $derived(
		open === null
			? false
			: openEntry.data != null
				? mayOn(mine.data, 'content.update', open.collection, who.data != null && openEntry.data.created_by === who.data.id)
				: may(mine.data, 'content.update', open.collection)
	);

	let editTitle = $state('');
	let moveKey = newKey();
	// Re-seed the form each time a different task opens.
	$effect(() => {
		if (open !== null) editTitle = open.title;
	});

	/** Changes the task on the etag it was read at; a `task_moved` answer refetches it first. */
	async function apply(update: TaskUpdate, doing: string): Promise<boolean> {
		if (open === null) return false;
		const task = open;
		const ok = await act(async () => {
			try {
				open = await updateTask(project, task.id, task.etag, update, moveKey);
			} catch (failure) {
				// `task_moved` (and a generic conflict) mean the read is stale: refetch before the
				// error shows, so what it complains about is what's on screen.
				if (failure instanceof ApiError && (failure.failure === 'conflict' || failure.reason === 'task_moved')) {
					open = await getTask(project, task.id).catch(() => open);
				}
				throw failure;
			}
			moveKey = newKey();
		}, doing);
		if (ok) await refresh();
		return ok;
	}
	async function saveTitle(event: SubmitEvent) {
		event.preventDefault();
		if (editTitle.trim() === '' || editTitle === open?.title) return;
		await apply({ title: editTitle.trim() }, 'rename this task');
	}
	/** Every choice is deliberate: '' sends `assignee: null`, a member their id — never omission.
	 *  A refused change puts the select back on the task's own value. */
	async function changePerson(event: Event) {
		const control = event.currentTarget as HTMLSelectElement;
		const value = control.value;
		const was = open?.assignee_id ?? '';
		if (value === was) return;
		const ok = await apply({ assignee: value === '' ? null : value }, value === '' ? 'unassign this task' : 'assign this task');
		if (!ok) control.value = was;
	}

	const entryHref = (task: EditorialTask) => resolve('/projects/[project]/[collection]/[entry]', { project, collection: task.collection, entry: task.entry_id });
	const statusBadge = (state: string) => (state === 'done' ? 'live' : state === 'cancelled' ? 'secondary' : 'pending') as 'live' | 'secondary' | 'pending';
</script>

<svelte:head><title>Tasks · Colophon Studio</title></svelte:head>

<PageHeader title="Tasks">
	{#snippet description()}
		What still has to be done before entries are ready — each one says what, who it's waiting on, and where it stands.
	{/snippet}
	{#snippet actions()}
		{#if makes}<Button onclick={() => openCreate(true)} data-testid="new-task"><PlusIcon data-icon="inline-start" />New task</Button>{/if}
	{/snippet}
</PageHeader>

<div class="mb-4 flex flex-wrap items-center justify-between gap-3">
	<Tabs.Root bind:value={status as string}>
		<Tabs.List aria-label="Show tasks">
			<Tabs.Trigger value="all" data-testid="filter-all">All</Tabs.Trigger>
			<Tabs.Trigger value="open" data-testid="filter-open">Open</Tabs.Trigger>
			<Tabs.Trigger value="done" data-testid="filter-done">Done</Tabs.Trigger>
			<Tabs.Trigger value="cancelled" data-testid="filter-cancelled">Cancelled</Tabs.Trigger>
		</Tabs.List>
	</Tabs.Root>
	<label class="flex items-center gap-2 text-sm text-muted-foreground">
		Waiting on
		<NativeSelect.Root bind:value={waiting} size="sm" data-testid="filter-assignee" aria-label="Waiting on">
			<NativeSelect.Option value="any">Anyone</NativeSelect.Option>
			<NativeSelect.Option value="me">Me</NativeSelect.Option>
			<NativeSelect.Option value="none">Nobody</NativeSelect.Option>
		</NativeSelect.Root>
	</label>
</div>

{#if tasks.isPending}
	<Skeleton class="h-48 w-full rounded-xl" />
{:else if tasks.isError}
	<Problem error={tasks.error} what="tasks" retry={() => tasks.refetch()} />
{:else if tasks.data.data.length === 0}
	<EmptyState icon={ListTodoIcon} title="No tasks here" testid="tasks-empty">
		{#if status === 'all' && waiting === 'any'}
			Give an entry something to finish — a task says what, who it's waiting on, and where it stands.
		{:else}
			Nothing matches — try another filter.
		{/if}
		{#snippet action()}
			{#if makes && status === 'all'}<Button variant="outline" onclick={() => openCreate(true)}>New task</Button>{/if}
		{/snippet}
	</EmptyState>
{:else}
	<div class="overflow-hidden rounded-xl border border-border bg-card">
		<Table.Root>
			<Table.Header>
				<Table.Row class="hover:bg-transparent">
					<Table.Head class="pl-4">Task</Table.Head>
					<Table.Head class="hidden md:table-cell">On</Table.Head>
					<Table.Head class="hidden sm:table-cell">Waiting on</Table.Head>
					<Table.Head>Status</Table.Head>
					<Table.Head class="pr-4 text-right">Updated</Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body data-testid="tasks" class="stagger">
				{#each tasks.data.data as task, index (task.id)}
					<Table.Row class="relative cursor-pointer" style="--i: {index}" data-testid="task" data-status={task.status}>
						<Table.Cell class="max-w-0 pl-4 sm:max-w-md">
							<button class="block w-full truncate text-left font-medium after:absolute after:inset-0" onclick={() => (open = task)} data-user-content>{task.title}</button>
						</Table.Cell>
						<Table.Cell class="hidden text-sm text-muted-foreground md:table-cell" data-user-content>{onWhat(task)}</Table.Cell>
						<Table.Cell class="hidden text-sm sm:table-cell" data-user-content>{person(task.assignee_id)}</Table.Cell>
						<Table.Cell><Badge variant={statusBadge(task.status)} data-testid="task-status">{task.status}</Badge></Table.Cell>
						<Table.Cell class="pr-4 text-right text-sm text-muted-foreground">{ago(task.updated_at)}</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	</div>
{/if}

<TechnicalDetails class="mt-8">
	<p>The API calls a task's state changes a <em>PATCH</em> that binds its etag; a task may also name a publish request.</p>
	<p class="font-mono break-all">/api/v1/projects/{project}/environments/production/tasks</p>
</TechnicalDetails>

<!-- Making one: the entry it belongs to, then who it's waiting on and what it's about. -->
<Dialog.Root bind:open={creating} onOpenChange={openCreate}>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-xl">
		<Dialog.Header>
			<Dialog.Title>New task</Dialog.Title>
			<Dialog.Description>Say what has to be done and which entry it's on; the rest can wait.</Dialog.Description>
		</Dialog.Header>
		<form onsubmit={create} class="space-y-4" data-testid="task-form">
			<FormRow label="What has to be done" for="task-title">
				<Input id="task-title" bind:value={title} maxlength={200} required placeholder="Check the numbers in the second table" data-testid="task-title" />
			</FormRow>
			<FormRow label="On entry" for="task-entry" hint="Only entries your role may change are listed.">
				{#if choices.isPending}
					<Skeleton class="h-8 w-full" />
				{:else if choices.isError}
					<Problem error={choices.error} what="entries" retry={() => choices.refetch()} />
				{:else}
					<NativeSelect.Root id="task-entry" bind:value={pickedEntry} size="sm" class="w-full" required data-testid="task-entry" onchange={() => (pickedRevision = '')}>
						<NativeSelect.Option value="" disabled>Choose an entry…</NativeSelect.Option>
						{#each choices.data ?? [] as group (group.type.slug)}
							<NativeSelect.OptGroup label={group.type.definition.name}>
								{#each group.entries as item (item.id)}
									<NativeSelect.Option value="{group.type.slug}/{item.id}" data-user-content>{shownTitle(item.revision.title)}</NativeSelect.Option>
								{/each}
							</NativeSelect.OptGroup>
						{/each}
					</NativeSelect.Root>
				{/if}
			</FormRow>
			<FormRow label="Waiting on" for="task-assignee" hint="A member of this project, or nobody yet.">
				<NativeSelect.Root id="task-assignee" bind:value={pickedPerson} size="sm" class="w-full" data-testid="task-assignee">
					<NativeSelect.Option value="">Nobody yet</NativeSelect.Option>
					{#each people.data ?? [] as member (member.identity_id)}
						<NativeSelect.Option value={member.identity_id} data-user-content>{member.identity_id === who.data?.id ? 'You' : (member.display_name ?? member.identity_id)}</NativeSelect.Option>
					{/each}
				</NativeSelect.Root>
			</FormRow>
			<FormRow label="On version" for="task-revision" hint="Optional: the exact version it's about.">
				<NativeSelect.Root
					id="task-revision"
					bind:value={pickedRevision}
					size="sm"
					class="w-full"
					disabled={pickedEntry === '' || versions.data === undefined || versions.data.length === 0}
					data-testid="task-revision"
				>
					<NativeSelect.Option value="">None</NativeSelect.Option>
					{#each versions.data ?? [] as revision (revision.id)}
						<NativeSelect.Option value={revision.id} data-user-content>{revision.title} — {ago(revision.created_at)}</NativeSelect.Option>
					{/each}
				</NativeSelect.Root>
			</FormRow>
			<FormRow label="On publish request" for="task-changeset" hint="Optional: the {PUBLISH_REQUESTS} it's about.">
				<NativeSelect.Root id="task-changeset" bind:value={pickedRequest} size="sm" class="w-full" disabled={requests.data === undefined || requests.data.length === 0} data-testid="task-changeset">
					<NativeSelect.Option value="">None</NativeSelect.Option>
					{#each requests.data ?? [] as item (item.id)}
						<NativeSelect.Option value={item.id} data-user-content>{item.title}</NativeSelect.Option>
					{/each}
				</NativeSelect.Root>
			</FormRow>
			{#if error !== ''}<p role="alert" class="text-sm text-destructive" data-testid="task-error">{error}</p>{/if}
			<Dialog.Footer>
				<Button type="submit" disabled={busy || title.trim() === '' || pickedEntry === ''} data-testid="task-create">{busy ? 'Creating…' : 'Create task'}</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>

<!-- The task itself: change its title, who it's waiting on (nobody sends null, never omission),
     and where it stands — each on the etag it was read at. Its entry's comments sit below. -->
<Dialog.Root open={open !== null} onOpenChange={(next) => !next && (open = null)}>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-2xl" data-testid="task-detail">
		{#if open !== null}
			<Dialog.Header>
				<Dialog.Title data-user-content>{open.title}</Dialog.Title>
				<Dialog.Description>
					On
					{#if entryGone}
						a deleted entry
					{:else}
						<a class="underline underline-offset-2" href={entryHref(open)} data-user-content>{onWhat(open)}</a>
					{/if}
					· made by <span data-user-content>{person(open.created_by).toLowerCase()}</span> {ago(open.created_at)}
				</Dialog.Description>
			</Dialog.Header>

			<div class="space-y-5">
				<div class="flex flex-wrap items-center gap-2">
					<Badge variant={statusBadge(open.status)} data-testid="detail-status">{open.status}</Badge>
					{#if open.status === 'done' && open.done_by !== null}
						<span class="text-xs text-muted-foreground">done by <span data-user-content>{person(open.done_by).toLowerCase()}</span>{open.done_at !== null ? ` ${ago(open.done_at)}` : ''}</span>
					{/if}
					{#if open.revision_id !== null}<Badge variant="outline">On one version</Badge>{/if}
					{#if open.changeset_id !== null}
						<Badge variant="outline">
							On <a class="ml-1 underline underline-offset-2" href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: open.changeset_id })} data-user-content>{request(open.changeset_id)?.title ?? 'a publish request'}</a>
						</Badge>
					{/if}
				</div>

				{#if makes}
					<form onsubmit={saveTitle} class="flex items-center gap-2">
						<Input bind:value={editTitle} maxlength={200} required aria-label="Task title" class="flex-1" data-testid="edit-title" />
						<Button type="submit" variant="outline" size="sm" disabled={busy || editTitle.trim() === '' || editTitle === open.title} data-testid="edit-save">Rename</Button>
					</form>
					<div class="flex flex-wrap items-center gap-3">
						<label class="flex items-center gap-2 text-sm">
							Waiting on
							<NativeSelect.Root
								value={open.assignee_id ?? ''}
								onchange={changePerson}
								size="sm"
								disabled={busy}
								data-testid="edit-assignee"
								aria-label="Waiting on"
							>
								<NativeSelect.Option value="">Unassigned</NativeSelect.Option>
								{#each people.data ?? [] as member (member.identity_id)}
									<NativeSelect.Option value={member.identity_id} data-user-content>{member.identity_id === who.data?.id ? 'You' : (member.display_name ?? member.identity_id)}</NativeSelect.Option>
								{/each}
							</NativeSelect.Root>
						</label>
						<span class="flex-1"></span>
						{#if open.status === 'open'}
							<Button variant="outline" size="sm" disabled={busy} onclick={() => apply({ status: 'done' }, 'finish this task')} data-testid="task-done">Mark done</Button>
							<Button variant="ghost" size="sm" disabled={busy} onclick={() => apply({ status: 'cancelled' }, 'cancel this task')} data-testid="task-cancel">Cancel task</Button>
						{:else}
							<Button variant="outline" size="sm" disabled={busy} onclick={() => apply({ status: 'open' }, 'reopen this task')} data-testid="task-reopen">Reopen</Button>
						{/if}
					</div>
				{/if}
				{#if error !== ''}<p role="alert" class="text-sm text-destructive" data-testid="task-error">{error}</p>{/if}

				<div class="border-t border-dotted border-border pt-4">
					<h3 class="mb-3 text-sm font-semibold">Comments</h3>
					<CommentsPanel
						{project}
						collection={open.collection}
						entry={open.entry_id}
						revision={open.revision_id ?? openEntry.data?.revision.id ?? null}
						canWrite={canDiscuss}
						live={!entryGone}
					/>
				</div>
			</div>
		{/if}
	</Dialog.Content>
</Dialog.Root>
