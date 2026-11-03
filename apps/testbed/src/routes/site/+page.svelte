<script lang="ts">
	import { onMount } from 'svelte';
	import { HOME_PAGE, navigationSeed, SITE_COMPONENTS, SITE_TYPES, THEME_SEED } from '@colophon/site-template';
	import { call, csrfToken, detailsOf, envelopeOf, listProjects, type Reply } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
	}

	let projects = $state<Project[]>([]);
	let signedIn = $state<boolean | null>(null);
	let projectId = $state('');
	let message = $state('');
	let steps = $state<string[]>([]);
	let homeId = $state('');
	let navItem = $state('');
	let palette = $state('');

	const env = $derived(`/api/v1/projects/${projectId}/environments/production`);

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

	const entries = (collection: string) => call('GET', `${env}/collections/${collection}/entries?limit=100`);

	/** The same ordered install the Studio runs, told one step at a time. */
	async function install() {
		steps = [];
		homeId = '';
		navItem = '';
		palette = '';
		message = '';
		const types = await call('GET', `${env}/content-types`);
		const components = await call('GET', `${env}/components`);
		if (types.status !== 200 || components.status !== 200) {
			message = `read: ${describe(types.status === 200 ? components : types)}`;
			return;
		}
		const haveType = new Set(((types.body as { data: { slug: string }[] }).data ?? []).map((type) => type.slug));
		const haveComponent = new Set(((components.body as { data: { slug: string }[] }).data ?? []).map((item) => item.slug));
		for (const shared of SITE_COMPONENTS) {
			if (haveComponent.has(shared.slug)) {
				steps = [...steps, `${shared.slug}: already there`];
				continue;
			}
			const made = await call('POST', `${env}/components`, { headers: headers(), body: JSON.stringify({ slug: shared.slug, definition: shared.definition }) });
			if (made.status !== 201) {
				message = `component: ${describe(made)}`;
				return;
			}
			steps = [...steps, `${shared.slug}: added`];
		}
		for (const type of SITE_TYPES) {
			if (haveType.has(type.slug)) {
				steps = [...steps, `${type.slug}: already there`];
				continue;
			}
			const made = await call('POST', `${env}/content-types`, { headers: headers(), body: JSON.stringify({ slug: type.slug, definition: type.definition }) });
			if (made.status !== 201) {
				message = `type ${type.slug}: ${describe(made)}`;
				return;
			}
			steps = [...steps, `${type.slug}: added`];
		}
		const pages = await entries('pages');
		const found = (pages.body as { data: { id: string; revision: { title: string } }[] }).data?.find((entry) => entry.revision.title === HOME_PAGE.title);
		if (found !== undefined) {
			homeId = found.id;
			steps = [...steps, 'home page: already there'];
		} else {
			const made = await call('POST', `${env}/collections/pages/entries`, { headers: headers(), body: JSON.stringify(HOME_PAGE) });
			if (made.status !== 201) {
				message = `home page: ${describe(made)}`;
				return;
			}
			homeId = (made.body as { data: { id: string } }).data.id;
			steps = [...steps, 'home page: written'];
		}
		const nav = await entries('navigation');
		if (((nav.body as { data: unknown[] }).data ?? []).length === 0) {
			const made = await call('POST', `${env}/collections/navigation/entries`, { headers: headers(), body: JSON.stringify(navigationSeed(homeId)) });
			if (made.status !== 201) {
				message = `navigation entry: ${describe(made)}`;
				return;
			}
			steps = [...steps, 'navigation entry: written'];
		} else steps = [...steps, 'navigation entry: already there'];
		const theme = await entries('theme');
		if (((theme.body as { data: unknown[] }).data ?? []).length === 0) {
			const made = await call('POST', `${env}/collections/theme/entries`, { headers: headers(), body: JSON.stringify(THEME_SEED) });
			if (made.status !== 201) {
				message = `theme entry: ${describe(made)}`;
				return;
			}
			steps = [...steps, 'theme entry: written'];
		} else steps = [...steps, 'theme entry: already there'];
		message = 'installed';
		await verify();
	}

	/** Reads back what the install left: the nav link's page and a theme colour. */
	async function verify() {
		const nav = await call('GET', `${env}/collections/navigation/entries?limit=100`);
		const navEntry = (nav.body as { data: { id: string }[] }).data?.[0];
		if (navEntry !== undefined) {
			const draft = await call('GET', `${env}/collections/navigation/entries/${navEntry.id}`);
			const items = ((draft.body as { data: { content: { items: { label: string; page?: string; url?: string }[] } } }).data.content.items ?? []) as {
				label: string;
				page?: string;
				url?: string;
			}[];
			navItem = items.map((item) => `${item.label} → ${item.page ?? item.url ?? ''}`).join(', ');
		}
		const theme = await call('GET', `${env}/collections/theme/entries?limit=100`);
		const themeEntry = (theme.body as { data: { id: string }[] }).data?.[0];
		if (themeEntry !== undefined) {
			const draft = await call('GET', `${env}/collections/theme/entries/${themeEntry.id}`);
			const light = (draft.body as { data: { content: { light: Record<string, unknown> } } }).data.content.light;
			palette = `primary ${light.primary}, text ${light.text}, radius-md ${light.radius_md}px`;
		}
	}

	onMount(loadProjects);
</script>

<h1>Website starter</h1>
<p class="lede">
	Adds the site bundle to a project through the same API the Studio uses: the section-style component, the Pages, Navigation and Theme
	content types, and the starter entries — a Home page of two sections, the menu linking to it, and the default palette.
</p>

{#if signedIn === false}
	<p data-testid="signed-out">Sign in first (the <a href="/session">session page</a> has the form).</p>
{:else if projects.length === 0}
	<p data-testid="no-projects">No projects yet — make one first.</p>
{:else}
	<section>
		<label class="field">
			Project
			<select bind:value={projectId} data-testid="project">
				{#each projects as project (project.id)}<option value={project.id}>{project.name}</option>{/each}
			</select>
		</label>
		<p>
			<button type="button" onclick={install} data-testid="install">Install the website starter</button>
		</p>
		{#if message}<p>Result: <span data-testid="message">{message}</span></p>{/if}
		{#if steps.length > 0}
			<ol data-testid="steps">
				{#each steps as step (step)}<li>{step}</li>{/each}
			</ol>
		{/if}
		{#if homeId}<p>Home: <code data-testid="home-id">{homeId}</code></p>{/if}
		{#if navItem}<p>Menu: <span data-testid="nav-item">{navItem}</span></p>{/if}
		{#if palette}<p>Palette: <span data-testid="palette">{palette}</span></p>{/if}
	</section>
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
</style>
