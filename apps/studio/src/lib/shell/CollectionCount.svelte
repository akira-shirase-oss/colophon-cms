<script lang="ts">
	import { createQuery } from '@tanstack/svelte-query';
	import * as Sidebar from '@colophon/ui/ui/sidebar';
	import { count, entriesQuery } from '#lib/queries.ts';

	let { project, collection }: { project: string; collection: string } = $props();
	const entries = createQuery(() => entriesQuery(project, collection));
</script>

{#if entries.data}<Sidebar.MenuBadge class="tabular">{count(entries.data)}</Sidebar.MenuBadge>{/if}
