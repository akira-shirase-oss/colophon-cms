<!-- One upgrade guide, from docs/upgrading/. -->
<script lang="ts">
	import RepoDocPage from '#lib/components/RepoDocPage.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<RepoDocPage title={data.title} html={data.html} toc={data.toc} source={data.source} section="Upgrade guides" capability={data.capability} />
