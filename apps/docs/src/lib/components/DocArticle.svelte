<!--
  mdsvex's layout for every Markdown page. The title, description and capability label come
  from the site map (lib/nav.ts), not the page, so a page missing from the map fails the build
  instead of appearing unlabelled. The table of contents comes from the page's own headings.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { page } from '$app/state';
	import type { TocEntry } from '#lib/markdown/structure.ts';
	import { pageAt } from '#lib/nav.ts';
	import { within } from '#lib/paths.ts';
	import PageFrame from './PageFrame.svelte';

	let { toc = [], children }: { toc?: TocEntry[]; children: Snippet } = $props();

	const entry = $derived.by(() => {
		const found = pageAt(within(page.url.pathname));
		if (!found) throw new Error(`${page.url.pathname} is not in lib/nav.ts: add it there with its status`);
		return found;
	});
</script>

<PageFrame title={entry.title} description={entry.description} capability={entry.capability} source={entry.source} {toc}>
	{@render children()}
</PageFrame>
