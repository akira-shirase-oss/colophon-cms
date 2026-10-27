<!--
  A design document from docs/, rendered from the file at build time. Its title is the file's own
  first heading; the label says what kind of page this is, because a design document mixes what
  is built with what is planned.
-->
<script lang="ts">
	import PageFrame from '#lib/components/PageFrame.svelte';
	import { blobUrl } from '#lib/nav.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<PageFrame
	title={data.title}
	capability={{ kind: 'design' }}
	source={data.source}
	toc={data.toc}
	section="Design documents · {data.group}"
	wide
>
	{#snippet note()}
		Rendered from <a class="font-mono text-foreground underline decoration-border underline-offset-4 hover:decoration-foreground" href={blobUrl(data.source)} rel="external">{data.source}</a>. It records the plan and what is built; the guides' labels say which is which.
	{/snippet}
	<!-- The repository's own Markdown, rendered at build time (lib/server/design-docs.ts). -->
	{@html data.html}
</PageFrame>
