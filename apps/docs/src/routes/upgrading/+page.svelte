<!--
  The upgrade guides, one per range of database migrations, newest first. Before v0.1 there are no
  versions to upgrade between, so a guide covers the migrations a milestone added; from v0.1 there
  will be one per minor version (docs/RELEASES.md).
-->
<script lang="ts">
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import CapabilityLabel from '#lib/components/CapabilityLabel.svelte';
	import PageFrame from '#lib/components/PageFrame.svelte';
	import { blobUrl, pageAt } from '#lib/nav.ts';
	import { href } from '#lib/paths.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	const self = pageAt('/upgrading/');
</script>

<PageFrame title={self?.title ?? 'Upgrade guides'} description={self?.description} capability={{ kind: 'overview' }} source="docs/upgrading">
	<p>
		Colophon's database changes only through numbered migrations, applied in order (see <a href={href('/operations/upgrades/')}>upgrades and migrations</a>). Each guide
		covers one range, written from the <a href={blobUrl('docs/templates/upgrade-guide.md')} rel="external">template</a> the project will use for versions: whether you are
		affected, what changes for editors, developers and operators, the steps, how to check it worked with Colophon's own receipts, and when rolling back is safe.
	</p>
	<ul class="cards">
		{#each data.guides as guide (guide.href)}
			<li>
				<a href={href(guide.href)} class="card" data-testid="upgrade-guide">
					<CapabilityLabel capability={guide.unreleased ? { kind: 'building', at: 'branches' } : { kind: 'done' }} size="small" />
					<span class="title">{guide.title}</span>
					<span class="when">{guide.range ? `Migrations ${guide.range}` : ''}{guide.range ? ' · ' : ''}{guide.source}</span>
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
