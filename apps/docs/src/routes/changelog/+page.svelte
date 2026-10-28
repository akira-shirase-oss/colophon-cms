<!--
  The changelog, rendered at build time from CHANGELOG.md, the one place an entry is written
  (docs/RELEASES.md); the website renders the same file at /changelog/. One anchor per milestone
  (`#p3-prove-it-reached-the-site`) and per step (`#step-3-5`), an Atom feed of merged steps, and
  each entry's changes grouped by type, as Keep a Changelog groups them.
-->
<script lang="ts">
	import RssIcon from '@lucide/svelte/icons/rss';
	import CapabilityLabel from '#lib/components/CapabilityLabel.svelte';
	import PageFrame from '#lib/components/PageFrame.svelte';
	import { pageAt, REPOSITORY, type Capability } from '#lib/nav.ts';
	import { href } from '#lib/paths.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const self = pageAt('/changelog/');
	const sections = $derived(data.changelog.sections);
	const count = $derived(sections.reduce((total, section) => total + section.entries.length, 0));
	const toc = $derived(sections.map((section) => ({ depth: 2 as const, id: section.id, text: section.unreleased ? 'Unreleased' : section.title })));

	const status = (entry: (typeof sections)[number]['entries'][number]): Capability =>
		entry.status === 'merged' ? { kind: 'done', at: entry.step } : { kind: 'building', at: entry.branch ?? entry.step };
	const LONG_DATE = new Intl.DateTimeFormat('en-GB', { day: 'numeric', month: 'long', year: 'numeric', timeZone: 'UTC' });
	const longDate = (date: string) => LONG_DATE.format(new Date(`${date}T00:00:00Z`));
</script>

<PageFrame title={self?.title ?? 'Changelog'} description={self?.description} capability={{ kind: 'overview' }} source="CHANGELOG.md" {toc}>
	<!-- CHANGELOG.md's introduction, rendered at build time with raw HTML escaped. -->
	{@html data.changelog.introHtml}
	<p>
		{count} entries. Milestones P0 to P3 also have <a href={href('/releases/')}>release notes</a>, and each range of database migrations an
		<a href={href('/upgrading/')}>upgrade guide</a>. Follow merged steps with the
		<a href={href('/changelog/feed.xml')} data-sveltekit-reload><RssIcon class="inline size-4 -translate-y-px" aria-hidden="true" /> Atom feed</a>.
	</p>

	{#each sections as section (section.id)}
		<h2 id={section.id}>{section.unreleased ? 'Unreleased' : section.title}</h2>
		{#if section.unreleased}
			<p>On branches, not merged into <code>main</code>: none of this is in the code yet, and any of it can change before it is.</p>
		{/if}
		<ol class="timeline">
			{#each section.entries as entry (entry.id)}
				<li class="entry" data-testid="changelog-entry" data-step={entry.step} data-status={entry.status}>
					<h3 id={entry.id} data-testid="changelog-title">{entry.step} · {entry.title}</h3>
					<div class="meta">
						<CapabilityLabel capability={status(entry)} size="small" />
						{#if entry.date}<time datetime={entry.date}>{longDate(entry.date)}</time>{/if}
						{#each entry.commits as commit (commit)}
							<a class="font-mono" href="{REPOSITORY}/commit/{commit}" rel="external">{commit}</a>
						{/each}
						<!-- The Decisions and Proof lines of CHANGELOG.md, rendered at build time. -->
						{#if entry.decisions}<span>{@html entry.decisions}</span>{/if}
						{#if entry.buildLog}<a href={entry.buildLog} rel="external">Build-log post</a>{/if}
					</div>
					{#if entry.introHtml}{@html entry.introHtml}{/if}
					{#each entry.groups as group (group.type)}
						<p class="type">{group.type}</p>
						<!-- One Keep a Changelog group of CHANGELOG.md, rendered at build time. -->
						{@html group.html}
					{/each}
					{#if entry.proof}<p class="proof"><strong>Proof:</strong> {@html entry.proof}</p>{/if}
				</li>
			{/each}
		</ol>
	{/each}
</PageFrame>

<style>
	.timeline {
		list-style: none;
		padding-left: 0;
		margin-top: 1.25rem;
		display: grid;
		grid-template-columns: minmax(0, 1fr);
		gap: 1rem;
	}
	.entry {
		position: relative;
		min-width: 0;
		border-radius: 1.5rem;
		background: var(--card);
		box-shadow: inset 0 0 0 1px var(--line);
		padding: 1.25rem 1.4rem 1.35rem;
	}
	.entry + .entry {
		margin-top: 0;
	}
	.entry :global(h3) {
		margin-top: 0;
		font-size: 1.125rem;
	}
	.meta {
		margin-top: 0.5rem;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.35rem 0.85rem;
		font-size: 0.8125rem;
		color: var(--muted-foreground);
	}
	.meta a {
		font-weight: 400;
		color: var(--muted-foreground);
	}
	.type {
		margin-top: 1rem;
		font-size: 0.75rem;
		line-height: 1rem;
		font-weight: 600;
		letter-spacing: 0.04em;
		text-transform: uppercase;
		color: var(--eyebrow);
	}
	.type + :global(ul) {
		margin-top: 0.4rem;
	}
	.entry :global(li),
	.entry :global(p) {
		font-size: 0.9375rem;
	}
	.proof {
		margin-top: 1rem;
		padding-top: 0.75rem;
		border-top: 1px solid var(--line);
		color: var(--muted-foreground);
	}
</style>
