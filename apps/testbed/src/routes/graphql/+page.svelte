<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, listProjects, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}

	interface GraphQLError {
		message: string;
		extensions?: { code?: string; pointer?: string };
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let message = $state('');
	let titles = $state<string[]>([]);
	let author = $state('');
	let entryTitle = $state('');
	let appliedQuery = $state('');
	let queryHash = $state('');
	let restHash = $state('');
	let refusedError = $state('');
	let mutationError = $state('');

	const env = $derived(`/api/v1/projects/${projectId}/environments/production`);
	const endpoint = $derived(`${env}/collections/posts/graphql`);

	const STAFF = {
		name: 'Staff',
		title_field: 'name',
		fields: [{ id: 'name', label: 'Name', type: 'text', required: true }]
	};
	const POSTS = {
		name: 'Posts',
		title_field: 'headline',
		fields: [
			{ id: 'headline', label: 'Headline', type: 'text', required: true, index: true },
			{ id: 'section', label: 'Section', type: 'enum', values: ['News', 'Guides'], index: true },
			{ id: 'author', label: 'Author', type: 'reference', to: 'staff' }
		]
	};

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason}${details.pointer ? ` at ${details.pointer}` : ''})` : ''}`.trim();
	}

	/** A GraphQL error as `message (code at pointer)` — the values REST's details hold. */
	function show(error: GraphQLError): string {
		const code = error.extensions?.code ?? '';
		const pointer = error.extensions?.pointer ?? '';
		return `${error.message}${code === '' ? '' : ` (${code}${pointer === '' ? '' : ` at ${pointer}`})`}`;
	}

	function headers(): Record<string, string> {
		return { 'X-CSRF-Token': csrfToken(), 'Idempotency-Key': crypto.randomUUID(), 'Content-Type': 'application/json' };
	}

	async function loadProjects() {
		const reply = await listProjects<Project>();
		signedIn = reply.status === 200;
		projects = reply.projects;
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
	}

	/** `POST …/graphql` with `document`; the `{data, errors}` body. */
	async function gql(document: string): Promise<{ data?: Record<string, unknown>; errors?: GraphQLError[] }> {
		const reply = await call('POST', endpoint, { headers: headers(), body: JSON.stringify({ query: document }) });
		if (reply.status !== 200) {
			message = `graphql: ${describe(reply)}`;
			return {};
		}
		return reply.body as { data?: Record<string, unknown>; errors?: GraphQLError[] };
	}

	/** staff + posts, one writer and two posts by her. */
	async function setUp() {
		for (const [slug, definition] of [
			['staff', STAFF],
			['posts', POSTS]
		] as const) {
			const made = await call('POST', `${env}/content-types`, {
				headers: headers(),
				body: JSON.stringify({ slug, definition })
			});
			if (made.status !== 201) {
				message = `type ${slug}: ${describe(made)}`;
				return;
			}
		}
		const writer = await call('POST', `${env}/collections/staff/entries`, {
			headers: headers(),
			body: JSON.stringify({ name: 'Ada' })
		});
		if (writer.status !== 201) {
			message = `staff: ${describe(writer)}`;
			return;
		}
		const ada = (writer.body as { data: { id: string } }).data.id;
		for (const content of [
			{ headline: 'Alpha news', section: 'News', author: ada },
			{ headline: 'Beta guide', section: 'Guides', author: ada }
		]) {
			const made = await call('POST', `${env}/collections/posts/entries`, {
				headers: headers(),
				body: JSON.stringify(content)
			});
			if (made.status !== 201) {
				message = `post: ${describe(made)}`;
				return;
			}
		}
		message = 'set up';
	}

	/** `entries` with a filter, a nested `author`, and `applied` — the REST twin beside it. */
	async function runEntries() {
		const body = await gql(
			'{ entries(where: {section: {eq: "News"}}, orderBy: [{field: "headline"}]) { ' +
				'nodes { title author { name } } pageInfo { hasNextPage } applied { query queryHash } } }'
		);
		const errors = body.errors ?? [];
		if (errors.length > 0) {
			message = `entries: ${show(errors[0]!)}`;
			return;
		}
		const entries = (body.data?.entries ?? {}) as {
			nodes?: { title: string; author?: { name?: string } | string }[];
			applied?: { query?: unknown; queryHash?: string };
		};
		titles = (entries.nodes ?? []).map((node) => node.title);
		const first = entries.nodes?.[0]?.author;
		author = typeof first === 'object' && first !== null ? (first.name ?? '') : '';
		appliedQuery = JSON.stringify(entries.applied?.query ?? null);
		queryHash = entries.applied?.queryHash ?? '';
		// The REST twin: the same canonical document `entries/query` would run.
		const twin = await call('POST', `${env}/collections/posts/entries/query`, {
			headers: headers(),
			body: JSON.stringify({
				filters: [{ field: 'section', op: 'eq', value: 'News' }],
				sort: [{ field: 'headline' }],
				fields: ['author'],
				depth: 1
			})
		});
		if (twin.status !== 200) {
			message = `twin: ${describe(twin)}`;
			return;
		}
		restHash = (twin.body as { applied: { query_hash: string } }).applied.query_hash;
	}

	/** `entry(id)` — the first post's own read. */
	async function runEntry() {
		const listed = await call('POST', `${env}/collections/posts/entries/query`, {
			headers: headers(),
			body: JSON.stringify({ fields: ['headline'], limit: 1 })
		});
		if (listed.status !== 200) {
			message = `list: ${describe(listed)}`;
			return;
		}
		const id = (listed.body as { data: { id: string }[] }).data[0]?.id ?? '';
		const body = await gql(`{ entry(id: "${id}") { title headline } }`);
		const errors = body.errors ?? [];
		if (errors.length > 0) {
			message = `entry: ${show(errors[0]!)}`;
			return;
		}
		entryTitle = ((body.data?.entry ?? {}) as { title?: string }).title ?? '';
	}

	/** `first: 0` compiles to `limit: 0`: the document's own refusal, as a GraphQL error. */
	async function runRefused() {
		const body = await gql('{ entries(first: 0) { nodes { title } } }');
		const errors = body.errors ?? [];
		refusedError = errors.length > 0 ? show(errors[0]!) : 'no error';
	}

	/** A `mutation` names a root the schema doesn't have — writes stay REST's. */
	async function runMutation() {
		const body = await gql('mutation { entries { nodes { title } } }');
		const errors = body.errors ?? [];
		mutationError = errors.length > 0 ? show(errors[0]!) : 'no error';
	}

	onMount(loadProjects);
</script>

<h1>GraphQL</h1>
<p class="lede">
	<code>POST …/graphql</code> runs the bounded query language's reads through a per-collection schema (DX.10, D-115):
	<code>entries(where, orderBy, first, after)</code> compiles to the same canonical document <code>entries/query</code>
	runs — filters and sorts on indexed fields only, the selection the document's <code>fields</code> and nested
	references its <code>depth</code> — and <code>applied</code> echoes the canonical document and its hash. Refusals
	arrive as GraphQL errors carrying the query's own <code>code</code>/<code>pointer</code>; mutations stay REST.
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
				<button type="button" onclick={setUp}>Set up staff and posts</button>
				<button type="button" onclick={runEntries} disabled={message === ''}>Query entries</button>
				<button type="button" onclick={runEntry} disabled={message === ''}>Read one entry</button>
				<button type="button" onclick={runRefused} disabled={message === ''}>Ask for zero entries</button>
				<button type="button" onclick={runMutation} disabled={message === ''}>Send a mutation</button>
			</p>
			{#if titles.length > 0}
				<p>
					Nodes:
					<span data-testid="titles">{titles.join(', ')}</span>
					{#if author}(author: <span data-testid="author">{author}</span>){/if}
				</p>
				<p>Applied: <code data-testid="applied">{appliedQuery}</code></p>
				<p>
					Hash <code data-testid="query-hash">{queryHash}</code> — REST twin
					<code data-testid="rest-hash">{restHash}</code>
					{#if restHash !== ''}<span data-testid="hash-match">{queryHash === restHash ? 'same document' : 'DIFFERS'}</span>{/if}
				</p>
			{/if}
			{#if entryTitle}<p>Entry: <span data-testid="entry-title">{entryTitle}</span></p>{/if}
			{#if refusedError}<p>Refused: <span data-testid="refused">{refusedError}</span></p>{/if}
			{#if mutationError}<p>Mutation: <span data-testid="mutation">{mutationError}</span></p>{/if}
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
		font-size: 0.875rem;
	}
</style>
