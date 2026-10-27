<!--
  A page rendered from a Markdown file in the repository (lib/server/repository-docs.ts): a design
  document, a milestone's release notes or an upgrade guide. Its title is the file's own first
  heading; the note under the label names the file, so a reader can see it is the file itself.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { TocEntry } from '#lib/markdown/structure.ts';
	import { blobUrl, type Capability } from '#lib/nav.ts';
	import PageFrame from './PageFrame.svelte';

	let {
		title,
		html,
		toc,
		source,
		section,
		capability,
		wide = false,
		explanation
	}: { title: string; html: string; toc: TocEntry[]; source: string; section: string; capability: Capability; wide?: boolean; explanation?: Snippet } = $props();
</script>

<PageFrame {title} {capability} {source} {toc} {section} {wide}>
	{#snippet note()}
		Rendered from <a class="font-mono text-foreground underline decoration-[color:var(--line)] underline-offset-4 hover:decoration-foreground" href={blobUrl(source)} rel="external"
			>{source}</a
		>.{#if explanation}{' '}{@render explanation()}{/if}
	{/snippet}
	<!-- The repository's own Markdown, rendered at build time (lib/server/repository-docs.ts). -->
	{@html html}
</PageFrame>
