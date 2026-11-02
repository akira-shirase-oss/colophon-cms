<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, listProjects, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let ada = $state('');
	let grace = $state('');
	let postId = $state('');
	let etag = $state('');
	let adaPosts = $state('');
	let gracePosts = $state('');
	let wrongInverse = $state('');
	let message = $state('');

	const env = $derived(`/api/v1/projects/${projectId}/environments/production`);

	const POSTS = {
		name: 'Posts',
		title_field: 'headline',
		fields: [{ id: 'headline', label: 'Headline', type: 'text', required: true }]
	};
	const WRITERS = {
		name: 'Writers',
		title_field: 'name',
		fields: [
			{ id: 'name', label: 'Name', type: 'text', required: true },
			{ id: 'posts', label: 'Posts', type: 'reference', to: 'posts', many: true, max: 10 }
		]
	};
	const linked = (inverse: string) => ({
		name: 'Posts',
		title_field: 'headline',
		fields: [
			{ id: 'headline', label: 'Headline', type: 'text', required: true },
			{ id: 'writer', label: 'Writer', type: 'reference', to: 'writers', inverse }
		]
	});

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason}${details.pointer ? ` at ${details.pointer}` : ''})` : ''}`.trim();
	}

	function headers(extra: Record<string, string> = {}): Record<string, string> {
		return { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), 'Idempotency-Key': crypto.randomUUID(), ...extra };
	}

	async function loadProjects() {
		const reply = await listProjects<Project>();
		signedIn = reply.status === 200;
		projects = reply.projects;
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
	}

	/** The two collections, the mirrored link, and two writers. A wrong inverse is tried first. */
	async function setUp() {
		for (const [slug, definition] of [['posts', POSTS], ['writers', WRITERS]] as const) {
			const made = await call('POST', `${env}/content-types`, { headers: headers(), body: JSON.stringify({ slug, definition }) });
			if (made.status !== 201) {
				message = `type ${slug}: ${describe(made)}`;
				return;
			}
		}
		const refused = await call('PUT', `${env}/content-types/posts`, { headers: headers({ 'If-Match': '"v1"' }), body: JSON.stringify(linked('name')) });
		wrongInverse = describe(refused);
		if (refused.status !== 422) {
			message = `bad inverse: ${describe(refused)}`;
			return;
		}
		const linkedPosts = await call('PUT', `${env}/content-types/posts`, { headers: headers({ 'If-Match': '"v1"' }), body: JSON.stringify(linked('posts')) });
		if (linkedPosts.status !== 200) {
			message = `link: ${describe(linkedPosts)}`;
			return;
		}
		for (const name of ['Ada', 'Grace'] as const) {
			const made = await call('POST', `${env}/collections/writers/entries`, { headers: headers(), body: JSON.stringify({ name }) });
			if (made.status !== 201) {
				message = `writer ${name}: ${describe(made)}`;
				return;
			}
			const id = (made.body as { data: { id: string } }).data.id;
			if (name === 'Ada') ada = id;
			else grace = id;
		}
		message = 'set up';
	}

	/** What each writer's mirrored `posts` holds, for the page to show. */
	async function readMirrors() {
		for (const [writer, show] of [
			[ada, (text: string) => (adaPosts = text)],
			[grace, (text: string) => (gracePosts = text)]
		] as const) {
			const read = await call('GET', `${env}/collections/writers/entries/${writer}`);
			const content = (read.body as { data: { content: { posts?: string[] } } }).data.content;
			show((content.posts ?? []).join(', ') || '(none)');
		}
	}

	async function linkPost() {
		const made = await call('POST', `${env}/collections/posts/entries`, { headers: headers(), body: JSON.stringify({ headline: 'Compilers', writer: ada }) });
		if (made.status !== 201) {
			message = describe(made);
			return;
		}
		postId = (made.body as { data: { id: string } }).data.id;
		etag = made.headers.get('etag') ?? '';
		await readMirrors();
		message = 'linked';
	}

	async function movePost() {
		const moved = await call('PUT', `${env}/collections/posts/entries/${postId}`, {
			headers: headers({ 'If-Match': etag }),
			body: JSON.stringify({ headline: 'Compilers', writer: grace })
		});
		if (moved.status !== 200) {
			message = describe(moved);
			return;
		}
		etag = moved.headers.get('etag') ?? '';
		await readMirrors();
		message = 'moved';
	}

	async function deletePost() {
		const deleted = await call('DELETE', `${env}/collections/posts/entries/${postId}`, { headers: headers({ 'If-Match': etag }) });
		if (deleted.status !== 200) {
			message = describe(deleted);
			return;
		}
		await readMirrors();
		message = 'deleted';
	}

	onMount(loadProjects);
</script>

<h1>Mirrored references</h1>
<p class="lede">
	A reference field can name the field on the other side that keeps its half of the link (D-085): posts name their writer, and
	writers' posts fill in — and empty — as posts are linked, moved and deleted, through the real API.
</p>

{#if signedIn === false}
	<p data-testid="signed-out">Not signed in. <a href={resolve('/session')}>Sign in on the Session page</a> first.</p>
{:else if signedIn}
	{#if message}<p data-testid="message">{message}</p>{/if}
	{#if projects.length > 0}
		<section>
			<label class="field">
				Project
				<select bind:value={projectId} name="project">
					{#each projects as project (project.id)}
						<option value={project.id}>{project.name} ({project.slug})</option>
					{/each}
				</select>
			</label>
			<p>
				<button type="button" onclick={setUp}>Set up posts and writers</button>
				<button type="button" onclick={linkPost} disabled={!grace}>Link a post to Ada</button>
				<button type="button" onclick={movePost} disabled={!postId}>Move it to Grace</button>
				<button type="button" onclick={deletePost} disabled={!postId}>Delete it</button>
			</p>
			{#if wrongInverse}<p>Wrong inverse: <span data-testid="wrong-inverse">{wrongInverse}</span></p>{/if}
			{#if postId}<p>Post: <code data-testid="post-id">{postId}</code></p>{/if}
			{#if adaPosts}<p>Ada's posts: <span data-testid="ada-posts">{adaPosts}</span></p>{/if}
			{#if gracePosts}<p>Grace's posts: <span data-testid="grace-posts">{gracePosts}</span></p>{/if}
		</section>
	{/if}
{/if}

<style>
	.lede {
		color: var(--muted);
		max-width: 46rem;
	}
	section {
		margin-top: 1.5rem;
	}
	.field {
		display: grid;
		gap: 0.25rem;
		font-size: 0.875rem;
		color: var(--muted);
		max-width: 46rem;
	}
	select {
		font: inherit;
		padding: 0.4rem 0.5rem;
	}
	code {
		word-break: break-all;
	}
	button {
		font: inherit;
		padding: 0.5rem 1rem;
		border-radius: 8px;
		border: 1px solid var(--line);
		background: var(--fg);
		color: var(--bg);
		cursor: pointer;
	}
	button:disabled {
		opacity: 0.5;
		cursor: default;
	}
</style>
