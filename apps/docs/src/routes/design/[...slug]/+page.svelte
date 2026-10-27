<!--
  A design document from docs/, rendered from the file at build time. The label says what kind of
  page this is, because a design document mixes what is built with what is planned.
-->
<script lang="ts">
	import RepoDocPage from '#lib/components/RepoDocPage.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<RepoDocPage title={data.title} html={data.html} toc={data.toc} source={data.source} section="Design documents · {data.group}" capability={{ kind: 'design' }} wide>
	{#snippet explanation()}It records the plan and what is built (main's version); the guides' labels say which is which.{/snippet}
</RepoDocPage>
