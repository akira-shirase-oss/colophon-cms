<!--
  The design documents' index: what they are, which one wins when they disagree, and the list,
  grouped as in docs/. The list is read from the repository at build time (the root layout).
-->
<script lang="ts">
	import PageFrame from '#lib/components/PageFrame.svelte';
	import { REPOSITORY } from '#lib/nav.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const groups = $derived(
		(['Project', 'Modules', 'Formats', 'Brand'] as const).map((group) => ({ group, docs: data.design.filter((doc) => doc.group === group) }))
	);
	const toc = $derived(groups.filter(({ docs }) => docs.length > 0).map(({ group }) => ({ depth: 2 as const, id: group.toLowerCase(), text: group })));
</script>

<PageFrame
	title="About the design documents"
	description="The plan Colophon is built from, rendered from the repository's own Markdown at build time."
	capability={{ kind: 'overview' }}
	source="apps/docs/src/routes/design/+page.svelte"
	section="Design documents"
	{toc}
>
	<p>
		These pages are the files in <a href="{REPOSITORY}/tree/main/docs">docs/</a>, rendered when this site is built, so they never drift from the repository. They are
		<strong>design documents</strong>: each one records a plan and, as steps land, what was built. Read them for the reasons behind a behaviour; read the guides,
		whose labels say what exists, for what you can use today.
	</p>
	<p>
		When they disagree, the order of authority is the one in <code>AGENTS.md</code>: <a href="/design/decisions/">the decisions</a>, then the module documents, then
		the architecture and the end goal, then the master specification (kept in <code>docs/spec/</code> on GitHub and not rendered here).
	</p>
	{#each groups as { group, docs } (group)}
		{#if docs.length > 0}
			<h2 id={group.toLowerCase()}>{group}</h2>
			<ul>
				{#each docs as doc (doc.href)}
					<li><a href={doc.href}>{doc.title}</a> <span class="text-sm text-muted-foreground">({doc.source})</span></li>
				{/each}
			</ul>
		{/if}
	{/each}
</PageFrame>
