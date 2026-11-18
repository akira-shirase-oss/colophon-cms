<!--
  A collection's entry count in the sidebar, counted by lib/entryState.ts as the overview and the
  list count them: the badge says how many entries, its title how many of them are live (live with
  unpublished changes included) and how many are drafts.
-->
<script lang="ts">
	import { createQuery } from '@tanstack/svelte-query';
	import * as Sidebar from '@colophon/ui/ui/sidebar';
	import { tally, tallyText } from '#lib/entryState.ts';
	import { entriesQuery } from '#lib/queries.ts';

	let { project, collection }: { project: string; collection: string } = $props();
	const entries = createQuery(() => entriesQuery(project, collection));
	const more = $derived(entries.data?.next_cursor != null);
	const counts = $derived(tally(entries.data?.data ?? []));
</script>

{#if entries.data && counts.total > 0}<Sidebar.MenuBadge class="tabular" title={tallyText(counts, more)} data-testid="collection-count"
		>{counts.total.toLocaleString()}{more ? '+' : ''}</Sidebar.MenuBadge
	>{/if}
