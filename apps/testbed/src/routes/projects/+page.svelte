<script lang="ts">
	import { call, envelopeOf } from '#lib/api.ts';

	interface Project {
		id: string;
		slug: string;
		name: string;
		role_name: string;
	}

	let search = $state('');
	let limit = $state('2');
	let shown = $state<Project[]>([]);
	let next = $state<string | null>(null);
	let message = $state('');

	async function load(cursor: string | null) {
		const query = new URLSearchParams({ limit });
		if (search.trim() !== '') query.set('q', search);
		if (cursor !== null) query.set('cursor', cursor);
		const reply = await call('GET', `/api/v1/projects?${query.toString()}`);
		if (reply.status !== 200) {
			message = `${reply.status} ${envelopeOf(reply.body)?.message ?? ''}`.trim();
			shown = [];
			next = null;
			return;
		}
		const page = reply.body as { data: Project[]; next_cursor: string | null };
		message = `${page.data.length} on this page`;
		shown = page.data;
		next = page.next_cursor;
	}
</script>

<h1>Projects</h1>
<p class="lede">
	<code>GET /api/v1/projects</code> a page at a time: by name, <code>q</code> matching a name or slug in any case, and a
	cursor to the next page.
</p>

<form
	onsubmit={(event) => {
		event.preventDefault();
		void load(null);
	}}
>
	<label>Search projects <input bind:value={search} /></label>
	<label
		>Page size
		<select bind:value={limit}>
			<option value="2">2</option>
			<option value="20">20</option>
			<option value="100">100</option>
		</select>
	</label>
	<button type="submit">Load</button>
</form>

<p role="status">{message}</p>
<ul data-testid="projects">
	{#each shown as project (project.id)}
		<li>{project.name} <small>{project.slug} · {project.role_name}</small></li>
	{/each}
</ul>
{#if next !== null}
	<button type="button" onclick={() => load(next)}>Next page</button>
{/if}

<style>
	.lede {
		color: var(--muted);
		max-width: 46rem;
	}
	form {
		display: flex;
		gap: 0.75rem;
		align-items: end;
		flex-wrap: wrap;
	}
	small {
		color: var(--muted);
	}
</style>
