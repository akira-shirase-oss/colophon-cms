<script lang="ts">
	import { onMount } from 'svelte';
	import { resolve } from '$app/paths';
	import { call, csrfToken, detailsOf, envelopeOf, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let author = $state('');
	let pageId = $state('');
	let stored = $state('');
	let referencedBy = $state('');
	let message = $state('');

	const env = $derived(`/api/v1/projects/${projectId}/environments/production`);

	const PEOPLE = { name: 'People', title_field: 'name', fields: [{ id: 'name', label: 'Name', type: 'text', required: true }] };
	const PAGES = {
		name: 'Pages',
		title_field: 'title',
		fields: [
			{ id: 'title', label: 'Title', type: 'text', required: true },
			{ id: 'hero', label: 'Hero', type: 'object', fields: [{ id: 'heading', label: 'Heading', type: 'text', required: true }] },
			{ id: 'tags', label: 'Tags', type: 'list', of: { type: 'text' }, max: 3 },
			{
				id: 'sections',
				label: 'Sections',
				type: 'blocks',
				blocks: [
					{ name: 'text_block', label: 'Text', fields: [{ id: 'body', label: 'Body', type: 'markdown', required: true }] },
					{
						name: 'quote',
						label: 'Quote',
						fields: [
							{ id: 'text', label: 'Quote', type: 'text', required: true },
							{ id: 'by', label: 'By', type: 'reference', to: 'people' }
						]
					}
				]
			}
		]
	};

	function describe(reply: Reply): string {
		const error = envelopeOf(reply.body);
		const details = detailsOf(reply.body);
		return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason}${details.pointer ? ` at ${details.pointer}` : ''})` : ''}`.trim();
	}

	function headers(extra: Record<string, string> = {}): Record<string, string> {
		return { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken(), 'Idempotency-Key': crypto.randomUUID(), ...extra };
	}

	function page(by: string) {
		return {
			title: 'Home',
			hero: { heading: 'Welcome' },
			tags: ['news'],
			sections: [
				{ body: '# Hello', _block: 'text_block' },
				{ by, text: 'Ship it', _block: 'quote' }
			]
		};
	}

	async function loadProjects() {
		const reply = await call('GET', '/api/v1/projects');
		signedIn = reply.status === 200;
		projects = signedIn ? (reply.body as { data: Project[] }).data : [];
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
	}

	async function setUp() {
		for (const [slug, definition] of [['people', PEOPLE], ['pages', PAGES]] as const) {
			const made = await call('POST', `${env}/content-types`, { headers: headers(), body: JSON.stringify({ slug, definition }) });
			if (made.status !== 201) {
				message = `type ${slug}: ${describe(made)}`;
				return;
			}
		}
		const person = await call('POST', `${env}/collections/people/entries`, { headers: headers(), body: JSON.stringify({ name: 'Ada' }) });
		author = person.status === 201 ? (person.body as { data: { id: string } }).data.id : '';
		message = author ? 'set up' : describe(person);
	}

	async function savePage() {
		const reply = await call('POST', `${env}/collections/pages/entries`, { headers: headers(), body: JSON.stringify(page(author)) });
		if (reply.status === 201) {
			pageId = (reply.body as { data: { id: string } }).data.id;
			const read = await call('GET', `${env}/collections/pages/entries/${pageId}`);
			stored = JSON.stringify((read.body as { data: { content: unknown } }).data.content);
			message = 'saved';
		} else {
			message = describe(reply);
		}
	}

	async function saveBroken() {
		const reply = await call('POST', `${env}/collections/pages/entries`, {
			headers: headers(),
			body: JSON.stringify(page('01a18f43-a955-755e-ab9c-1ded4406683c'))
		});
		message = describe(reply);
	}

	/** Makes, submits, approves and publishes a changeset of `entries`; the answer, described. */
	async function publish(entries: string[]) {
		const made = await call('POST', `${env}/changesets`, { headers: headers(), body: JSON.stringify({ title: 'Testbed', entries }) });
		if (made.status !== 201) {
			message = describe(made);
			return;
		}
		const set = (made.body as { data: { id: string; hash: string } }).data;
		const ifMatch = { 'If-Match': `"${set.hash}"` };
		await call('POST', `${env}/changesets/${set.id}/submit`, { headers: headers(ifMatch) });
		await call('POST', `${env}/changesets/${set.id}/reviews`, { headers: headers(ifMatch), body: JSON.stringify({ decision: 'approve' }) });
		const releases = await call('GET', `${env}/releases`);
		const generation = Number(releases.headers.get('colophon-generation') ?? '0');
		const published = await call('POST', `${env}/changesets/${set.id}/publish`, {
			headers: headers(ifMatch),
			body: JSON.stringify({ base_generation: generation })
		});
		message = published.status === 201 ? 'published' : describe(published);
	}

	async function readAuthor() {
		const read = await call('GET', `${env}/collections/people/entries/${author}`);
		const list = (read.body as { data: { referenced_by: { collection: string; field: string }[] } }).data.referenced_by;
		referencedBy = list.map((item) => `${item.collection} ${item.field}`).join(', ');
	}

	onMount(loadProjects);
</script>

<h1>Content modelling</h1>
<p class="lede">
	Structures and references through the real API (steps M.1 to M.3, D-064 to D-066): a page with an object, a list and blocks whose
	quote names a person. A reference to nobody is refused where it is; a page can't be published before the person it names; the
	person's read lists what references them.
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
				<button type="button" onclick={setUp}>Set up types and Ada</button>
				<button type="button" onclick={savePage} disabled={!author}>Save a page quoting Ada</button>
				<button type="button" onclick={saveBroken} disabled={!author}>Save a quote by nobody</button>
			</p>
			<p>
				<button type="button" onclick={() => publish([pageId])} disabled={!pageId}>Publish the page alone</button>
				<button type="button" onclick={() => publish([pageId, author])} disabled={!pageId}>Publish the page and Ada</button>
				<button type="button" onclick={readAuthor} disabled={!pageId}>Read Ada</button>
			</p>
			{#if stored}<p>Stored: <code data-testid="stored">{stored}</code></p>{/if}
			{#if referencedBy}<p>Referenced by: <span data-testid="referenced-by">{referencedBy}</span></p>{/if}
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
		font-size: 0.85rem;
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
