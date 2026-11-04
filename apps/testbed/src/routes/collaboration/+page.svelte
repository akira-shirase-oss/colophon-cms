<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, listProjects, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}
	interface Member {
		identity_id: string;
		display_name: string | null;
	}
	interface Listed {
		id: string;
		revision: { id: string; title: string };
	}
	interface Comment {
		id: string;
		revision_id: string;
		field: string | null;
		body: string;
		author_id: string;
		resolved_by: string | null;
		resolved_at: string | null;
		created_at: string;
		etag: string;
	}
	interface Task {
		id: string;
		title: string;
		status: string;
		assignee_id: string | null;
		revision_id: string | null;
		changeset_id: string | null;
		done_by: string | null;
		done_at: string | null;
		etag: string;
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let members = $state<Member[]>([]);
	let entries = $state<Listed[]>([]);
	let entryId = $state('');
	const entry = $derived(entries.find((item) => item.id === entryId));
	let comments = $state<Comment[]>([]);
	let tasks = $state<Task[]>([]);
	let body = $state('');
	let taskTitle = $state('');
	let taskAssignee = $state('');
	let pin = $state(false);
	// The comment and task being looked at, with the etag their actions send in If-Match —
	// editable, so a stale one can be tried.
	let openComment = $state<Comment | null>(null);
	let commentEtag = $state('');
	let openTask = $state<Task | null>(null);
	let openEtag = $state('');
	let openAssignee = $state('keep');
	let message = $state('');

	const base = $derived(`/api/v1/projects/${projectId}/environments/production`);
	const entryPath = $derived(`${base}/collections/articles/entries/${entryId}`);
	const who = (id: string | null) =>
		id === null ? 'nobody' : (members.find((member) => member.identity_id === id)?.display_name ?? id);

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason})` : ''}`.trim();
	}
	const write = () => ({
		'Content-Type': 'application/json',
		'X-CSRF-Token': csrfToken(),
		'Idempotency-Key': crypto.randomUUID()
	});

	async function loadProjects() {
		const reply = await listProjects<Project>();
		signedIn = reply.status === 200;
		projects = reply.projects;
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
		if (projectId !== '') await loadProject();
	}

	async function loadProject() {
		const found = await call('GET', `/api/v1/projects/${projectId}/members`);
		members = found.status === 200 ? (found.body as { data: Member[] }).data : [];
		const reply = await call('GET', `${base}/collections/articles/entries?limit=100`);
		entries = reply.status === 200 ? (reply.body as { data: Listed[] }).data : [];
		if (entryId === '' && entries.length > 0) entryId = entries[0]?.id ?? '';
		await loadEntry();
	}

	async function loadEntry() {
		if (entryId === '') {
			comments = [];
			tasks = [];
			openComment = null;
			openTask = null;
			return;
		}
		const list = await call('GET', `${entryPath}/comments?limit=100`);
		comments = list.status === 200 ? (list.body as { data: Comment[] }).data : [];
		const all = await call('GET', `${base}/tasks?entry=${entryId}&limit=100`);
		tasks = all.status === 200 ? (all.body as { data: Task[] }).data : [];
		if (openComment !== null) openComment = comments.find((item) => item.id === openComment?.id) ?? null;
		commentEtag = openComment?.etag ?? '';
		if (openTask !== null) openTask = tasks.find((item) => item.id === openTask?.id) ?? null;
		openEtag = openTask?.etag ?? '';
	}

	function focusComment(comment: Comment) {
		openComment = comment;
		commentEtag = comment.etag;
	}
	function focusTask(task: Task) {
		openTask = task;
		openEtag = task.etag;
		openAssignee = 'keep';
	}

	async function addComment(event: SubmitEvent) {
		event.preventDefault();
		if (entry === undefined) return;
		const reply = await call('POST', `${entryPath}/comments`, {
			headers: write(),
			body: JSON.stringify({ revision: entry.revision.id, body })
		});
		message = reply.status === 201 ? 'comment added' : describe(reply);
		if (reply.status === 201) body = '';
		await loadEntry();
	}

	async function answer(resolve: boolean) {
		if (openComment === null) return;
		const reply = await call('POST', `${entryPath}/comments/${openComment.id}/${resolve ? 'resolve' : 'reopen'}`, {
			headers: { 'X-CSRF-Token': csrfToken(), 'If-Match': `"${commentEtag}"`, 'Idempotency-Key': crypto.randomUUID() }
		});
		message = reply.status === 200 ? (resolve ? 'resolved' : 'reopened') : describe(reply);
		await loadEntry();
	}

	async function createTask(event: SubmitEvent) {
		event.preventDefault();
		if (entry === undefined) return;
		const reply = await call('POST', `${base}/tasks`, {
			headers: write(),
			body: JSON.stringify({
				entry: entryId,
				title: taskTitle,
				...(taskAssignee === '' ? {} : { assignee: taskAssignee }),
				...(pin ? { revision: entry.revision.id } : {})
			})
		});
		message = reply.status === 201 ? 'task created' : describe(reply);
		if (reply.status === 201) {
			taskTitle = '';
			taskAssignee = '';
			pin = false;
		}
		await loadEntry();
	}

	/** PATCHes the open task on the etag on screen; `assignee` 'none' sends null, 'keep' is absent. */
	async function updateTask(update: { title?: string; status?: string; assignee?: string | null }) {
		if (openTask === null) return;
		const reply = await call('PATCH', `${base}/tasks/${openTask.id}`, {
			headers: { ...write(), 'If-Match': `"${openEtag}"` },
			body: JSON.stringify(update)
		});
		message = reply.status === 200 ? 'task updated' : describe(reply);
		await loadEntry();
	}

	async function assign() {
		// 'none' is a real value (nobody) and is sent as null — never left out, which would keep it.
		if (openAssignee === 'keep') return;
		await updateTask({ assignee: openAssignee === 'none' ? null : openAssignee });
		openAssignee = 'keep';
	}

	onMount(loadProjects);
</script>

<h1>Collaboration</h1>
<p class="lede">
	Comments and tasks through the real API (E1, D-102): a comment binds the revision on screen and is kept verbatim —
	only resolved or reopened, each against its etag in <code>If-Match</code>. Edit an etag below to see a stale one
	refused <code>comment_moved</code> or <code>task_moved</code>. Waiting on nobody is sent as <code>assignee: null</code>.
</p>

{#if signedIn === false}
	<p data-testid="signed-out">Not signed in. <a href={resolve('/session')}>Sign in on the Session page</a> first.</p>
{:else if signedIn}
	<label class="field">
		Project
		<select bind:value={projectId} onchange={loadProject} name="project">
			{#each projects as project (project.id)}
				<option value={project.id}>{project.name} ({project.slug})</option>
			{/each}
		</select>
	</label>
	<label class="field">
		Entry
		<select bind:value={entryId} onchange={loadEntry} name="entry">
			{#each entries as item (item.id)}
				<option value={item.id}>{item.revision.title}</option>
			{/each}
		</select>
	</label>

	{#if message}<p data-testid="message">{message}</p>{/if}

	{#if entry}
		<section data-testid="comments">
			<h2>Comments</h2>
			<form onsubmit={addComment}>
				<label class="field">New comment <input bind:value={body} name="comment-body" /></label>
				<button type="submit" data-testid="add-comment">Add comment</button>
			</form>
			<ol>
				{#each comments as comment (comment.id)}
					<li data-testid="comment" data-resolved={comment.resolved_at !== null}>
						<button class="plain" onclick={() => focusComment(comment)}>{comment.body}</button>
						<span class="muted">{comment.resolved_at === null ? 'open' : 'resolved'} — on revision {comment.revision_id.slice(0, 8)}</span>
					</li>
				{:else}
					<li class="muted">No comments on this entry yet.</li>
				{/each}
			</ol>
			{#if openComment}
				<div class="focus" data-testid="open-comment">
					<p><strong>{openComment.body}</strong> <code data-testid="comment-state">{openComment.resolved_at === null ? 'open' : 'resolved'}</code></p>
					<label class="field">If-Match etag <input bind:value={commentEtag} name="comment-etag" /></label>
					<p class="row">
						<button onclick={() => answer(true)} data-testid="resolve">Resolve</button>
						<button onclick={() => answer(false)} data-testid="reopen">Reopen</button>
					</p>
				</div>
			{/if}
		</section>

		<section data-testid="tasks">
			<h2>Tasks</h2>
			<form onsubmit={createTask}>
				<label class="field">Task title <input bind:value={taskTitle} name="task-title" /></label>
				<label class="field">
					Waiting on
					<select bind:value={taskAssignee} name="task-assignee">
						<option value="">nobody</option>
						{#each members as member (member.identity_id)}
							<option value={member.identity_id}>{member.display_name ?? member.identity_id}</option>
						{/each}
					</select>
				</label>
				<label><input type="checkbox" bind:checked={pin} name="pin" /> About the revision on screen</label>
				<button type="submit" data-testid="create-task">Create task</button>
			</form>
			<ol data-testid="task-list">
				{#each tasks as task (task.id)}
					<li data-testid="task" data-status={task.status}>
						<button class="plain" onclick={() => focusTask(task)}>{task.title}</button>
						<span class="muted">{task.status}, waiting on {who(task.assignee_id)}</span>
					</li>
				{:else}
					<li class="muted">No tasks on this entry yet.</li>
				{/each}
			</ol>
			{#if openTask}
				<div class="focus" data-testid="open-task">
					<p>
						<strong>{openTask.title}</strong> <code data-testid="task-status">{openTask.status}</code>
						<span class="muted">waiting on <span data-testid="task-waiting">{who(openTask.assignee_id)}</span></span>
					</p>
					<label class="field">If-Match etag <input bind:value={openEtag} name="task-etag" /></label>
					<label class="field">
						Reassign to
						<select bind:value={openAssignee} name="reassign">
							<option value="keep">— choose —</option>
							<option value="none">nobody</option>
							{#each members as member (member.identity_id)}
								<option value={member.identity_id}>{member.display_name ?? member.identity_id}</option>
							{/each}
						</select>
					</label>
					<p class="row">
						<button onclick={assign} data-testid="assign">Assign</button>
						<button onclick={() => updateTask({ status: 'done' })} data-testid="done">Mark done</button>
						<button onclick={() => updateTask({ status: 'cancelled' })} data-testid="cancel">Cancel task</button>
						<button onclick={() => updateTask({ status: 'open' })} data-testid="reopen-task">Reopen</button>
					</p>
				</div>
			{/if}
		</section>
	{/if}
{/if}

<style>
	.lede {
		color: var(--muted);
		max-width: 46rem;
	}
	.muted {
		color: var(--muted);
	}
	form,
	section,
	.focus {
		display: grid;
		gap: 0.75rem;
		max-width: 46rem;
		margin-top: 1rem;
	}
	.focus {
		border: 1px solid var(--line);
		border-radius: 8px;
		padding: 0.75rem;
	}
	.field {
		display: grid;
		gap: 0.25rem;
		font-size: 0.875rem;
		color: var(--muted);
	}
	input,
	select {
		font: inherit;
		padding: 0.4rem 0.5rem;
	}
	.row {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
	}
	button {
		font: inherit;
		padding: 0.5rem 1rem;
		border-radius: 8px;
		border: 1px solid var(--line);
		background: var(--fg);
		color: var(--bg);
		cursor: pointer;
		justify-self: start;
	}
	button.plain {
		background: none;
		border: none;
		padding: 0;
		text-align: left;
		text-decoration: underline;
	}
</style>
