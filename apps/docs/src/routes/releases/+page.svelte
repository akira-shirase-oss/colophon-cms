<!--
  The release notes, one per milestone, newest first. Before v0.1 a milestone is the unit people
  can follow; from v0.1 there will be one per version (docs/RELEASES.md).
-->
<script lang="ts">
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import CapabilityLabel from '#lib/components/CapabilityLabel.svelte';
	import PageFrame from '#lib/components/PageFrame.svelte';
	import { blobUrl, pageAt } from '#lib/nav.ts';
	import { href } from '#lib/paths.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	const self = pageAt('/releases/');
</script>

<PageFrame title={self?.title ?? 'Release notes'} description={self?.description} capability={{ kind: 'overview' }} source="docs/releases">
	<p>
		Colophon has no versions yet: v0.1 is not out. Until it is, each milestone gets release notes when its proof gate passes, written from the
		<a href={blobUrl('docs/templates/release-notes.md')} rel="external">template</a> the project will use for versions. Each has an upgrade at a glance, what changed shape, highlights, every change (the
		milestone's part of the <a href={href('/changelog/')}>changelog</a>), known limits and the evidence that ran.
	</p>
	<ul class="cards">
		{#each data.notes as note (note.href)}
			<li>
				<a href={href(note.href)} class="card" data-testid="release-note">
					<CapabilityLabel capability={note.released ? { kind: 'done', at: note.milestone } : { kind: 'building', at: note.milestone }} size="small" />
					<span class="title">{note.title}</span>
					<span class="when">{note.released ? `Released ${note.released}` : 'In progress'} · {note.source}</span>
					<ArrowRightIcon class="arrow size-4" aria-hidden="true" />
				</a>
			</li>
		{/each}
	</ul>
</PageFrame>

<style>
	.cards {
		list-style: none;
		padding-left: 0;
		display: grid;
		gap: 0.75rem;
	}
	.card {
		position: relative;
		display: grid;
		gap: 0.35rem;
		justify-items: start;
		border-radius: 1.5rem;
		background: var(--chip);
		padding: 1.1rem 3rem 1.15rem 1.25rem;
		text-decoration: none;
		transition: background-color 200ms ease;
	}
	.card:hover {
		background: var(--chip-hover);
	}
	.title {
		font-weight: 600;
		letter-spacing: -0.015em;
		color: var(--foreground);
	}
	.when {
		font-size: 0.8125rem;
		font-weight: 400;
		color: var(--muted-foreground);
	}
	.card :global(.arrow) {
		position: absolute;
		right: 1.25rem;
		top: 50%;
		translate: 0 -50%;
		color: var(--muted-foreground);
	}
</style>
