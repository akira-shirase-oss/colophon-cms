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
	let entryId = $state('');
	let siblingId = $state('');
	let group = $state('');
	let repeat = $state('');
	let frenchList = $state('');
	let message = $state('');

	// "Published in…": the environment's languages, posts published in a few of them, and the
	// delivery API read with ?locale= — anonymous, as any site reads it.
	interface Language {
		tag: string;
		label: string;
		default: boolean;
		fallback: string[];
	}
	interface Localization {
		requested: string;
		resolved: string;
		fallback: boolean;
	}
	interface Row {
		id: string;
		title: string;
		locale: string | null;
		localization?: Localization;
	}
	interface Read extends Row {
		requested_id?: string;
		alternates?: { id: string; locale: string | null; slug?: string }[];
	}

	let languages = $state('');
	let oldEtag = $state('');
	let stale = $state('');
	let published = $state('');
	let posts = $state<{ hello: string; morning: string; morningPt: string; english: string } | null>(null);
	let rows = $state<Row[]>([]);
	let contentLanguage = $state('');
	let entryRead = $state('');
	let alternates = $state('');
	let missing = $state('');

	const env = $derived(`/api/v1/projects/${projectId}/environments/production`);

	const NOTES = {
		name: 'Notes',
		title_field: 'title',
		fields: [
			{ id: 'title', label: 'Title', type: 'text', required: true },
			{ id: 'slug', label: 'Slug', type: 'slug' }
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

	async function loadProjects() {
		const reply = await listProjects<Project>();
		signedIn = reply.status === 200;
		projects = reply.projects;
		if (projectId === '' && projects.length > 0) projectId = projects[0]?.id ?? '';
	}

	/** The collection and its English entry, localized once so the group exists. */
	async function setUp() {
		const made = await call('POST', `${env}/content-types`, { headers: headers(), body: JSON.stringify({ slug: 'notes', definition: NOTES }) });
		if (made.status !== 201) {
			message = `type: ${describe(made)}`;
			return;
		}
		const created = await call('POST', `${env}/collections/notes/entries?locale=en`, {
			headers: headers(),
			body: JSON.stringify({ title: 'Welcome', slug: 'welcome' })
		});
		if (created.status !== 201) {
			message = `entry: ${describe(created)}`;
			return;
		}
		entryId = (created.body as { data: { id: string } }).data.id;
		const localized = await call('POST', `${env}/collections/notes/entries/${entryId}/localizations?locale=fr`, { headers: headers() });
		if (localized.status !== 201) {
			message = `localize: ${describe(localized)}`;
			return;
		}
		siblingId = (localized.body as { data: { id: string } }).data.id;
		await readGroup();
		message = 'set up';
	}

	/** The source's group, as `locale → id` lines. */
	async function readGroup() {
		const read = await call('GET', `${env}/collections/notes/entries/${entryId}`);
		const localizations = (read.body as { data: { localizations: { id: string; locale: string }[] } }).data.localizations;
		group = localizations.map((row) => `${row.locale} → ${row.id}`).join(', ');
	}

	/** A sibling in a locale the group already holds must be refused. */
	async function repeatIt() {
		const again = await call('POST', `${env}/collections/notes/entries/${entryId}/localizations?locale=fr`, { headers: headers() });
		repeat = describe(again);
	}

	/** The list's locale filter, keeping only the French row. */
	async function listFrench() {
		const listed = await call('GET', `${env}/collections/notes/entries?limit=100&locale=fr`);
		const rows = (listed.body as { data: { id: string; locale: string }[] }).data;
		frenchList = rows.map((row) => `${row.locale} → ${row.id}`).join(', ') || '(none)';
	}

	/** English is the default; Portuguese falls back to English, Brazilian Portuguese to
	 *  Portuguese and then English, French to English. */
	const LANGUAGES: Language[] = [
		{ tag: 'en', label: 'English', default: true, fallback: [] },
		{ tag: 'pt', label: 'Português', default: false, fallback: ['en'] },
		{ tag: 'pt-BR', label: 'Português (Brasil)', default: false, fallback: ['pt', 'en'] },
		{ tag: 'fr', label: 'Français', default: false, fallback: ['en'] }
	];

	const POSTS = {
		name: 'Posts',
		title_field: 'title',
		fields: [
			{ id: 'title', label: 'Title', type: 'text', required: true },
			{ id: 'slug', label: 'Slug', type: 'slug' }
		]
	};

	/** A failed step, described: the panel's message names it and the flow stops there. */
	class Refused extends Error {}

	function need(reply: Reply, status: number, what: string): Reply {
		if (reply.status !== status) throw new Refused(`${what}: ${describe(reply)}`);
		return reply;
	}

	/** Reads the list and its ETag, then saves the four languages on top of it. */
	async function setLanguages() {
		const current = await call('GET', `${env}/locales`);
		if (current.status !== 200) {
			message = `languages: ${describe(current)}`;
			return;
		}
		oldEtag = current.headers.get('etag') ?? '';
		const saved = await call('PUT', `${env}/locales`, {
			headers: headers({ 'If-Match': oldEtag }),
			body: JSON.stringify({ locales: LANGUAGES })
		});
		if (saved.status !== 200) {
			message = `languages: ${describe(saved)}`;
			return;
		}
		const list = (saved.body as { data: { version: number; locales: Language[] } }).data;
		const shown = list.locales.map((language) =>
			language.default ? `${language.tag} (default)` : `${language.tag} → ${language.fallback.join(', ') || 'nothing'}`
		);
		languages = `${shown.join('; ')} — version ${list.version}`;
	}

	/** The same save on top of the version it replaced: refused, the list moved on. */
	async function saveStale() {
		const again = await call('PUT', `${env}/locales`, {
			headers: headers({ 'If-Match': oldEtag }),
			body: JSON.stringify({ locales: LANGUAGES })
		});
		stale = describe(again);
	}

	/** Three posts: "Hello" in English, Portuguese and Brazilian Portuguese; "Good morning" in
	 *  English and Portuguese; "Only in English". Every member published in one release. */
	async function publishPosts() {
		try {
			need(await call('POST', `${env}/content-types`, { headers: headers(), body: JSON.stringify({ slug: 'posts', definition: POSTS }) }), 201, 'type');
			const create = async (title: string, slug: string) => {
				const made = need(
					await call('POST', `${env}/collections/posts/entries?locale=en`, { headers: headers(), body: JSON.stringify({ title, slug }) }),
					201,
					`post ${title}`
				);
				return (made.body as { data: { id: string } }).data.id;
			};
			// A sibling starts as a copy of the source's draft; its title is then translated.
			const translate = async (source: string, locale: string, title: string) => {
				const made = need(
					await call('POST', `${env}/collections/posts/entries/${source}/localizations?locale=${locale}`, { headers: headers() }),
					201,
					`localize ${locale}`
				);
				const draft = (made.body as { data: { id: string; content: Record<string, unknown> } }).data;
				need(
					await call('PUT', `${env}/collections/posts/entries/${draft.id}`, {
						headers: headers({ 'If-Match': made.headers.get('etag') ?? '' }),
						body: JSON.stringify({ ...draft.content, title })
					}),
					200,
					`translate ${locale}`
				);
				return draft.id;
			};
			const hello = await create('Hello', 'hello');
			const helloPt = await translate(hello, 'pt', 'Olá');
			const helloBr = await translate(hello, 'pt-BR', 'Oi');
			const morning = await create('Good morning', 'good-morning');
			const morningPt = await translate(morning, 'pt', 'Bom dia');
			const english = await create('Only in English', 'only-in-english');

			const made = need(
				await call('POST', `${env}/changesets`, {
					headers: headers(),
					body: JSON.stringify({ title: 'Posts in a few languages', entries: [hello, helloPt, helloBr, morning, morningPt, english] })
				}),
				201,
				'changeset'
			);
			const set = (made.body as { data: { id: string; hash: string } }).data;
			const ifMatch = { 'If-Match': `"${set.hash}"` };
			need(await call('POST', `${env}/changesets/${set.id}/submit`, { headers: headers(ifMatch) }), 200, 'submit');
			need(await call('POST', `${env}/changesets/${set.id}/reviews`, { headers: headers(ifMatch), body: JSON.stringify({ decision: 'approve' }) }), 200, 'review');
			const releases = await call('GET', `${env}/releases`);
			const base = Number(releases.headers.get('colophon-generation') ?? '0');
			const release = need(
				await call('POST', `${env}/changesets/${set.id}/publish`, { headers: headers(ifMatch), body: JSON.stringify({ base_generation: base }) }),
				201,
				'publish'
			);
			posts = { hello, morning, morningPt, english };
			published = `published as generation ${(release.body as { data: { generation: number } }).data.generation}`;
		} catch (error) {
			if (!(error instanceof Refused)) throw error;
			message = error.message;
		}
	}

	/** A delivery read with no cookie — what any reader on the internet gets. */
	async function deliver(path: string): Promise<{ status: number; headers: Headers; body: unknown }> {
		const response = await fetch(`/server/content/v1/${projectId}/production/posts${path}`, { credentials: 'omit', cache: 'no-store' });
		return { status: response.status, headers: response.headers, body: await response.json() };
	}

	/** The list in pt-BR: one row per post, each its best published version for pt-BR → pt → en. */
	async function readBrazilian() {
		const read = await deliver('?locale=pt-BR');
		if (read.status !== 200) {
			message = `list: HTTP ${read.status} ${envelopeOf(read.body)?.code ?? ''}`;
			return;
		}
		rows = (read.body as { data: Row[] }).data;
		contentLanguage = read.headers.get('content-language') ?? '(none)';
	}

	/** "Good morning" asked for in pt-BR: there is no Brazilian version, so its Portuguese
	 *  sibling answers, naming the English one that was asked for and its other versions. */
	async function readMorning() {
		if (posts === null) return;
		const read = await deliver(`/${posts.morning}?locale=pt-BR`);
		if (read.status !== 200) {
			entryRead = `HTTP ${read.status} ${envelopeOf(read.body)?.code ?? ''}`;
			return;
		}
		const entry = (read.body as { data: Read }).data;
		const how = entry.localization;
		entryRead = `${entry.requested_id ?? '?'} → ${entry.id} (${how?.resolved ?? entry.locale ?? '?'}${how?.fallback ? ', fallback' : ''})`;
		alternates = (entry.alternates ?? []).map((other) => `${other.locale} ${other.slug ?? ''}`.trim()).join(', ') || '(none)';
	}

	/** "Only in English" asked for in French with no fallback: not published in that language. */
	async function readMissing() {
		if (posts === null) return;
		const read = await deliver(`/${posts.english}?locale=fr&fallback=none`);
		const error = envelopeOf(read.body);
		const details = (read.body as { error?: { details?: { reason?: string; available?: string[] } } }).error?.details;
		missing = `${read.status} ${error?.code ?? ''}${details?.reason ? ` (${details.reason})` : ''}; published in: ${(details?.available ?? []).join(', ') || 'nothing'}`;
	}

	onMount(loadProjects);
</script>

<h1>Locales</h1>
<p class="lede">
	Entries carry a locale tag, and a localization group holds one entry per tag (D-087): an English note, its French sibling made
	by copying the draft, the group's list on each member, the refused duplicate, and the list's locale filter — all through the
	real API. Below, the environment's languages and their fallbacks, and what the delivery API serves in each.
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
				<button type="button" onclick={setUp}>Set up an English note and its French version</button>
				<button type="button" onclick={repeatIt} disabled={!siblingId}>Localize it in French again</button>
				<button type="button" onclick={listFrench} disabled={!siblingId}>List the French entries</button>
			</p>
			{#if entryId}<p>English: <code data-testid="entry-id">{entryId}</code></p>{/if}
			{#if siblingId}<p>French: <code data-testid="sibling-id">{siblingId}</code></p>{/if}
			{#if group}<p>Group: <span data-testid="group">{group}</span></p>{/if}
			{#if repeat}<p>Again: <span data-testid="repeat">{repeat}</span></p>{/if}
			{#if frenchList}<p>French list: <span data-testid="french-list">{frenchList}</span></p>{/if}
		</section>

		<section>
			<h2>Published in…</h2>
			<p class="note">
				The environment lists its languages, each with the ones it falls back to. Posts are published in a few of them; an anonymous
				read with <code>?locale=</code> gets each post's best published version, says which language answered, and marks a
				fallback.
			</p>
			<p>
				<button type="button" onclick={setLanguages}>Set the languages</button>
				<button type="button" onclick={saveStale} disabled={!languages}>Save them again on the old version</button>
				<button type="button" onclick={publishPosts} disabled={!languages || posts !== null}>Publish posts in a few languages</button>
			</p>
			{#if languages}<p>Languages: <span data-testid="languages">{languages}</span></p>{/if}
			{#if stale}<p>Old version: <span data-testid="stale">{stale}</span></p>{/if}
			{#if published}<p data-testid="published">{published}</p>{/if}
			{#if posts}
				<p>
					Good morning: en <code data-testid="morning-en">{posts.morning}</code>, pt <code data-testid="morning-pt">{posts.morningPt}</code>
				</p>
				<p>
					<button type="button" onclick={readBrazilian}>Read the posts in pt-BR</button>
					<button type="button" onclick={readMorning}>Read Good morning in pt-BR</button>
					<button type="button" onclick={readMissing}>Read Only in English in French, no fallback</button>
				</p>
			{/if}
			{#if contentLanguage}
				<p>Content-Language: <code data-testid="content-language">{contentLanguage}</code></p>
				<ul class="rows" data-testid="published-rows">
					{#each rows as row (row.id)}
						<li data-testid="published-row">
							{row.title}: <code>{row.localization?.requested ?? '?'}</code> → <code>{row.localization?.resolved ?? row.locale ?? '?'}</code>
							{#if row.localization?.fallback}<span class="badge" data-testid="fallback-badge">fallback</span>{/if}
						</li>
					{/each}
				</ul>
			{/if}
			{#if entryRead}<p>Entry: <span data-testid="entry-read">{entryRead}</span></p>{/if}
			{#if alternates}<p>Also published in: <span data-testid="alternates">{alternates}</span></p>{/if}
			{#if missing}<p>French: <span data-testid="missing">{missing}</span></p>{/if}
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
	h2 {
		font-size: 1.1rem;
		margin: 0 0 0.25rem;
	}
	.note {
		color: var(--muted);
		max-width: 46rem;
		margin-top: 0;
	}
	.rows {
		padding-left: 1.2rem;
	}
	.rows li {
		margin: 0.2rem 0;
	}
	.badge {
		display: inline-block;
		margin-left: 0.4rem;
		padding: 0 0.4rem;
		border-radius: 999px;
		font-size: 0.75rem;
		color: var(--warn);
		background: var(--warn-bg);
	}
</style>
